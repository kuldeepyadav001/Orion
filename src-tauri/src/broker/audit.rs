//! Append-only audit logger and undo journal (M5).
//!
//! ## Audit Invariant
//!
//! Every capability request — whether permitted, rejected, confirmed or denied —
//! is written synchronously to `broker_audit_log`. The agent itself has no SQL
//! permissions or tool capabilities to delete, truncate, or alter audit records.
//!
//! ## Undo Journal Invariant
//!
//! Reversible modifications (T1 writes, creations) save their pre-state image to
//! `broker_undo_journal`. Users can invoke single-click rollback from the UI to
//! restore the file to its exact pre-execution bytes.

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::{OrionError, Result};
use super::domain::Domain;
use super::tier::CapabilityTier;

/// Decision outcome for an evaluated capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuditDecision {
    Allowed,
    Rejected,
    Confirmed,
    Denied,
}

impl AuditDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allowed => "ALLOWED",
            Self::Rejected => "REJECTED",
            Self::Confirmed => "CONFIRMED",
            Self::Denied => "DENIED",
        }
    }
}

/// An entry in the append-only broker audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: String,
    pub timestamp: String,
    pub domain: Domain,
    pub action: String,
    pub target: String,
    pub tier: CapabilityTier,
    pub decision: AuditDecision,
    pub reason: String,
}

/// An entry in the rollback undo journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoRecord {
    pub id: String,
    pub timestamp: String,
    pub action: String,
    pub target_path: String,
    pub has_backup: bool,
    pub is_undone: bool,
}

/// Append a capability evaluation record to the database audit trail.
pub fn log_audit(
    conn: &Connection,
    domain: Domain,
    action: &str,
    target: &str,
    tier: CapabilityTier,
    decision: AuditDecision,
    reason: &str,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO broker_audit_log (id, timestamp, domain, action, target, tier, decision, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            now,
            domain.to_string(),
            action,
            target,
            tier.as_str(),
            decision.as_str(),
            reason
        ],
    )
    .map_err(|e| OrionError::Db(format!("failed to append audit record: {e}")))?;

    Ok(id)
}

/// Record a pre-image snapshot into the undo journal before executing a modification.
pub fn record_undo_snapshot(
    conn: &Connection,
    action: &str,
    target_path: &Path,
    backup_data: Option<&[u8]>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let path_str = target_path.to_string_lossy().to_string();

    conn.execute(
        "INSERT INTO broker_undo_journal (id, timestamp, action, target_path, backup_data, is_undone)
         VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![id, now, action, path_str, backup_data],
    )
    .map_err(|e| OrionError::Db(format!("failed to record undo snapshot: {e}")))?;

    Ok(id)
}

/// Revert an action recorded in the undo journal.
pub fn rollback_undo_entry(conn: &Connection, journal_id: &str) -> Result<PathBuf> {
    let (target_path_str, backup_data, is_undone): (String, Option<Vec<u8>>, i32) = conn
        .query_row(
            "SELECT target_path, backup_data, is_undone FROM broker_undo_journal WHERE id = ?1",
            params![journal_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| OrionError::Db(format!("undo journal entry not found: {e}")))?;

    if is_undone != 0 {
        return Err(OrionError::Security("operation has already been undone".into()));
    }

    let target = PathBuf::from(&target_path_str);

    match backup_data {
        Some(bytes) => {
            // Restore previous file contents
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&target, bytes).map_err(|e| {
                OrionError::Io(std::io::Error::new(
                    e.kind(),
                    format!("failed to restore file at '{}': {e}", target.display()),
                ))
            })?;
        }
        None => {
            // The file did not exist prior to this action; rollback deletes the newly created file.
            if target.is_file() {
                let _ = std::fs::remove_file(&target);
            }
        }
    }

    // Mark as undone
    conn.execute(
        "UPDATE broker_undo_journal SET is_undone = 1 WHERE id = ?1",
        params![journal_id],
    )
    .map_err(|e| OrionError::Db(format!("cannot update undo status: {e}")))?;

    // Log the rollback in the audit trail
    log_audit(
        conn,
        Domain::Trusted,
        "undo_rollback",
        &target_path_str,
        CapabilityTier::T1Reversible,
        AuditDecision::Allowed,
        "restored pre-action state via user undo request",
    )?;

    Ok(target)
}

/// Retrieve the most recent audit log entries.
pub fn list_recent_audits(conn: &Connection, limit: usize) -> Result<Vec<AuditRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, timestamp, domain, action, target, tier, decision, reason
             FROM broker_audit_log
             ORDER BY timestamp DESC
             LIMIT ?1",
        )
        .map_err(|e| OrionError::Db(format!("failed to prepare audit list: {e}")))?;

    let rows = stmt
        .query_map(params![limit as i64], |row| {
            let domain_str: String = row.get(2)?;
            let tier_str: String = row.get(5)?;
            let decision_str: String = row.get(6)?;

            let domain = if domain_str == "trusted" {
                Domain::Trusted
            } else {
                Domain::Untrusted
            };

            let tier = match tier_str.as_str() {
                "T0" => CapabilityTier::T0Read,
                "T1" => CapabilityTier::T1Reversible,
                "T2" => CapabilityTier::T2Destructive,
                "T3" => CapabilityTier::T3System,
                _ => CapabilityTier::Blocked,
            };

            let decision = match decision_str.as_str() {
                "ALLOWED" => AuditDecision::Allowed,
                "CONFIRMED" => AuditDecision::Confirmed,
                "DENIED" => AuditDecision::Denied,
                _ => AuditDecision::Rejected,
            };

            Ok(AuditRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                domain,
                action: row.get(3)?,
                target: row.get(4)?,
                tier,
                decision,
                reason: row.get(7)?,
            })
        })
        .map_err(|e| OrionError::Db(format!("failed to query audit records: {e}")))?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| OrionError::Db(format!("failed reading audit row: {e}")))?);
    }
    Ok(out)
}

/// Retrieve the most recent undo journal entries.
pub fn list_recent_undos(conn: &Connection, limit: usize) -> Result<Vec<UndoRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, timestamp, action, target_path, (backup_data IS NOT NULL) AS has_backup, is_undone
             FROM broker_undo_journal
             ORDER BY timestamp DESC
             LIMIT ?1",
        )
        .map_err(|e| OrionError::Db(format!("failed to prepare undo list: {e}")))?;

    let rows = stmt
        .query_map(params![limit as i64], |row| {
            let is_undone_int: i32 = row.get(5)?;
            Ok(UndoRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                action: row.get(2)?,
                target_path: row.get(3)?,
                has_backup: row.get(4)?,
                is_undone: is_undone_int != 0,
            })
        })
        .map_err(|e| OrionError::Db(format!("failed to query undo records: {e}")))?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| OrionError::Db(format!("failed reading undo row: {e}")))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE broker_audit_log (
                id          TEXT PRIMARY KEY,
                timestamp   TEXT NOT NULL,
                domain      TEXT NOT NULL CHECK (domain IN ('trusted','untrusted')),
                action      TEXT NOT NULL,
                target      TEXT NOT NULL,
                tier        TEXT NOT NULL CHECK (tier IN ('T0','T1','T2','T3','BLOCKED')),
                decision    TEXT NOT NULL CHECK (decision IN ('ALLOWED','REJECTED','CONFIRMED','DENIED')),
                reason      TEXT NOT NULL
            );

            CREATE TABLE broker_undo_journal (
                id          TEXT PRIMARY KEY,
                timestamp   TEXT NOT NULL,
                action      TEXT NOT NULL,
                target_path TEXT NOT NULL,
                backup_data BLOB,
                is_undone   INTEGER NOT NULL DEFAULT 0 CHECK (is_undone IN (0, 1))
            );
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn logs_and_retrieves_audit_records() {
        let conn = in_memory_conn();
        let id = log_audit(
            &conn,
            Domain::Trusted,
            "read_file",
            "/workspace/doc.md",
            CapabilityTier::T0Read,
            AuditDecision::Allowed,
            "authorized read within bounds",
        )
        .unwrap();

        assert!(!id.is_empty());
        let records = list_recent_audits(&conn, 10).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].action, "read_file");
        assert_eq!(records[0].decision, AuditDecision::Allowed);
    }

    #[test]
    fn records_undo_and_reverts_file() {
        let conn = in_memory_conn();
        let tmp_file = std::env::temp_dir().join("orion-undo-test.txt");
        let initial_bytes = b"original text";
        std::fs::write(&tmp_file, initial_bytes).unwrap();

        let id = record_undo_snapshot(&conn, "write_draft", &tmp_file, Some(initial_bytes)).unwrap();

        // Mutate file
        std::fs::write(&tmp_file, b"corrupted overwrite").unwrap();
        assert_eq!(std::fs::read(&tmp_file).unwrap(), b"corrupted overwrite");

        // Rollback
        let restored_path = rollback_undo_entry(&conn, &id).unwrap();
        assert_eq!(restored_path, tmp_file);
        assert_eq!(std::fs::read(&tmp_file).unwrap(), initial_bytes);

        // Cannot rollback twice
        let second_res = rollback_undo_entry(&conn, &id);
        assert!(second_res.is_err());

        let _ = std::fs::remove_file(&tmp_file);
    }
}
