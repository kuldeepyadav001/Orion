//! The Capability Broker and Security Boundary (M5).
//!
//! Orchestrates domain isolation, capability tier gating, path containment,
//! append-only audit logging, and user confirmation tickets.

pub mod audit;
pub mod domain;
pub mod path;
pub mod tier;
#[cfg(test)]
pub mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::db::Db;
use crate::error::{OrionError, Result};

pub use audit::{
    list_recent_audits, list_recent_undos, log_audit, record_undo_snapshot, rollback_undo_entry,
    AuditDecision, AuditRecord, UndoRecord,
};
pub use domain::{Directive, Domain, DomainSource};
pub use path::{is_blocked_system_path, resolve_beneath};
pub use tier::{ActionKind, CapabilityTier};

/// Lifespan of an interactive confirmation ticket (seconds).
const TICKET_TTL_SECS: i64 = 120;

/// Pending user confirmation ticket for high-tier (T2/T3) operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationTicket {
    pub id: String,
    pub action: ActionKind,
    pub tier: CapabilityTier,
    pub prompt: String,
    pub created_at: String,
    pub expires_at: String,
}

/// The outcome of evaluating an action request through the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EvaluationResult {
    /// Safe to execute immediately (T0 read or T1 reversible).
    Allowed { action: ActionKind },
    /// Requires user prompt confirmation (T2 destructive or T3 system).
    RequiresConfirmation {
        ticket_id: String,
        tier: CapabilityTier,
        prompt: String,
    },
    /// Permanently blocked or domain security violation.
    Blocked { reason: String },
}

/// The centralized Capability Broker coordinating security rules across Orion.
pub struct CapabilityBroker {
    db: Arc<Mutex<Db>>,
    workspace_root: PathBuf,
    tickets: Mutex<HashMap<String, (ConfirmationTicket, chrono::DateTime<Utc>)>>,
}

impl CapabilityBroker {
    /// Create a new Capability Broker rooted at `workspace_root`.
    pub fn new(db: Arc<Mutex<Db>>, workspace_root: PathBuf) -> Self {
        // Ensure workspace directory exists
        let _ = std::fs::create_dir_all(&workspace_root);

        Self {
            db,
            workspace_root,
            tickets: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the active workspace root directory.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Evaluate an action against the source domain and capability policy.
    pub async fn evaluate(
        &self,
        directive: &Directive,
        action: ActionKind,
    ) -> Result<EvaluationResult> {
        let domain = directive.source.domain();
        let target = action.target_summary();
        let tier = action.default_tier();

        // 1. DOMAIN BOUNDARY CHECK (Non-negotiable firewall)
        // Untrusted sources (documents, email bodies, scraped web pages) cannot invoke any tools.
        if !directive.source.is_trusted() {
            let reason = format!(
                "untrusted domain source ({}) attempted to invoke capability tier {}",
                directive.source.domain(),
                tier
            );
            let db = self.db.lock().await;
            let _ = log_audit(
                db.conn(),
                domain,
                &action.target_summary(),
                &target,
                tier,
                AuditDecision::Rejected,
                &reason,
            );
            return Ok(EvaluationResult::Blocked { reason });
        }

        // 2. HARD-CODED BLOCKED TIERS (Credentials, system configs)
        if tier.is_blocked() {
            let reason = format!("target '{target}' is permanently blocked by security policy");
            let db = self.db.lock().await;
            let _ = log_audit(
                db.conn(),
                domain,
                &action.target_summary(),
                &target,
                CapabilityTier::Blocked,
                AuditDecision::Denied,
                &reason,
            );
            return Ok(EvaluationResult::Blocked { reason });
        }

        // 3. PATH CONTAINMENT VERIFICATION for path-based actions
        match &action {
            ActionKind::ReadFile { path }
            | ActionKind::ListDirectory { path }
            | ActionKind::WriteDraft { path, .. }
            | ActionKind::OverwriteFile { path, .. }
            | ActionKind::DeleteFile { path } => {
                if let Err(e) = resolve_beneath(&self.workspace_root, path) {
                    let reason = format!("path validation failed: {e}");
                    let db = self.db.lock().await;
                    let _ = log_audit(
                        db.conn(),
                        domain,
                        &action.target_summary(),
                        &target,
                        tier,
                        AuditDecision::Denied,
                        &reason,
                    );
                    return Ok(EvaluationResult::Blocked { reason });
                }
            }
            ActionKind::RenameFile { from, to } => {
                if let Err(e) = resolve_beneath(&self.workspace_root, from) {
                    let reason = format!("source path validation failed: {e}");
                    let db = self.db.lock().await;
                    let _ = log_audit(
                        db.conn(),
                        domain,
                        &action.target_summary(),
                        &target,
                        tier,
                        AuditDecision::Denied,
                        &reason,
                    );
                    return Ok(EvaluationResult::Blocked { reason });
                }
                if let Err(e) = resolve_beneath(&self.workspace_root, to) {
                    let reason = format!("destination path validation failed: {e}");
                    let db = self.db.lock().await;
                    let _ = log_audit(
                        db.conn(),
                        domain,
                        &action.target_summary(),
                        &target,
                        tier,
                        AuditDecision::Denied,
                        &reason,
                    );
                    return Ok(EvaluationResult::Blocked { reason });
                }
            }
            _ => {}
        }

        // 4. USER CONFIRMATION GATING (T2/T3)
        if tier.requires_confirmation() {
            let ticket_id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            let expires_at = now + Duration::seconds(TICKET_TTL_SECS);

            let ticket = ConfirmationTicket {
                id: ticket_id.clone(),
                action: action.clone(),
                tier,
                prompt: action.confirmation_prompt(),
                created_at: now.to_rfc3339(),
                expires_at: expires_at.to_rfc3339(),
            };

            let prompt = ticket.prompt.clone();
            self.tickets.lock().await.insert(ticket_id.clone(), (ticket, expires_at));

            let db = self.db.lock().await;
            let _ = log_audit(
                db.conn(),
                domain,
                &action.target_summary(),
                &target,
                tier,
                AuditDecision::Confirmed,
                "held pending explicit user confirmation ticket",
            );

            return Ok(EvaluationResult::RequiresConfirmation {
                ticket_id,
                tier,
                prompt,
            });
        }

        // 5. T0/T1 AUTO-APPROVAL
        let db = self.db.lock().await;
        let _ = log_audit(
            db.conn(),
            domain,
            &action.target_summary(),
            &target,
            tier,
            AuditDecision::Allowed,
            "authorized execution within boundaries",
        );

        Ok(EvaluationResult::Allowed { action })
    }

    /// Confirm and claim an action ticket issued during a T2/T3 evaluation.
    pub async fn confirm_ticket(&self, ticket_id: &str) -> Result<ActionKind> {
        let mut map = self.tickets.lock().await;
        let (ticket, expires_at) = map
            .remove(ticket_id)
            .ok_or_else(|| OrionError::Security(format!("confirmation ticket '{ticket_id}' not found")))?;

        if Utc::now() > expires_at {
            return Err(OrionError::Security(format!(
                "confirmation ticket '{ticket_id}' has expired"
            )));
        }

        let db = self.db.lock().await;
        let _ = log_audit(
            db.conn(),
            Domain::Trusted,
            &ticket.action.target_summary(),
            &ticket.action.target_summary(),
            ticket.tier,
            AuditDecision::Allowed,
            "user explicitly confirmed ticket execution",
        );

        Ok(ticket.action)
    }

    /// Reject and discard a pending confirmation ticket.
    pub async fn reject_ticket(&self, ticket_id: &str) -> Result<()> {
        let mut map = self.tickets.lock().await;
        let entry = map.remove(ticket_id);
        if let Some((ticket, _)) = entry {
            let db = self.db.lock().await;
            let _ = log_audit(
                db.conn(),
                Domain::Trusted,
                &ticket.action.target_summary(),
                &ticket.action.target_summary(),
                ticket.tier,
                AuditDecision::Denied,
                "user explicitly cancelled confirmation ticket",
            );
        }
        Ok(())
    }

    /// Safely execute a T1 workspace write action with automatic undo journal snapshot.
    pub async fn execute_t1_write(
        &self,
        directive: &Directive,
        rel_path: &Path,
        content: &str,
    ) -> Result<PathBuf> {
        let action = ActionKind::WriteDraft {
            path: rel_path.to_path_buf(),
            content: content.to_string(),
        };

        // Gated through broker evaluation
        match self.evaluate(directive, action).await? {
            EvaluationResult::Allowed { .. } => {}
            EvaluationResult::RequiresConfirmation { .. } => {
                return Err(OrionError::Security("unexpected confirmation required for T1".into()));
            }
            EvaluationResult::Blocked { reason } => {
                return Err(OrionError::Security(format!("T1 write blocked: {reason}")));
            }
        }

        let full_path = resolve_beneath(&self.workspace_root, rel_path)?;

        // Pre-image snapshot for rollback
        let pre_bytes = if full_path.is_file() {
            Some(std::fs::read(&full_path).map_err(|e| {
                OrionError::Io(std::io::Error::new(
                    e.kind(),
                    format!("cannot read existing file for undo snapshot: {e}"),
                ))
            })?)
        } else {
            None
        };

        let db = self.db.lock().await;
        let _ = record_undo_snapshot(
            db.conn(),
            "write_draft",
            &full_path,
            pre_bytes.as_deref(),
        )?;

        // Write content
        if let Some(parent) = full_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&full_path, content.as_bytes()).map_err(|e| {
            OrionError::Io(std::io::Error::new(
                e.kind(),
                format!("failed writing draft to '{}': {e}", full_path.display()),
            ))
        })?;

        tracing::info!(path = %full_path.display(), "T1 draft written with undo snapshot");
        Ok(full_path)
    }

    /// Roll back an action from the undo journal.
    pub async fn rollback(&self, journal_id: &str) -> Result<PathBuf> {
        let db = self.db.lock().await;
        rollback_undo_entry(db.conn(), journal_id)
    }

    /// Query the recent audit log records.
    pub async fn audit_log(&self, limit: usize) -> Result<Vec<AuditRecord>> {
        let db = self.db.lock().await;
        list_recent_audits(db.conn(), limit)
    }

    /// Query the recent undo journal records.
    pub async fn undo_history(&self, limit: usize) -> Result<Vec<UndoRecord>> {
        let db = self.db.lock().await;
        list_recent_undos(db.conn(), limit)
    }
}
