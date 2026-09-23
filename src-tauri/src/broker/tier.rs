//! Capability permission tiers (M5).
//!
//! Every action in Orion is assigned to a strict capability tier that dictates
//! its execution gate, audit obligations, and user confirmation requirements.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Capability permission levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityTier {
    /// Safe, read-only operations (list directory, read allowlisted file, library status).
    /// Executed automatically without blocking the user.
    T0Read,

    /// Low-risk, fully reversible modifications (write draft note, scratchpad create).
    /// Executed with non-blocking user notification and automatic pre-image undo logging.
    T1Reversible,

    /// High-risk or destructive actions (file delete, overwrite, mass rename).
    /// Requires explicit interactive user confirmation before execution.
    T2Destructive,

    /// Shell execution or outbound network activity (running commands, external APIs).
    /// Strict per-call confirmation ticket with ephemeral single-use lifetime.
    T3System,

    /// Permanently blocked actions (self-config modification, credential theft, system paths).
    /// Invariant: permanently forbidden regardless of user prompts or configurations.
    Blocked,
}

impl CapabilityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::T0Read => "T0",
            Self::T1Reversible => "T1",
            Self::T2Destructive => "T2",
            Self::T3System => "T3",
            Self::Blocked => "BLOCKED",
        }
    }

    /// True if the tier requires explicit user approval before running.
    pub fn requires_confirmation(&self) -> bool {
        matches!(self, Self::T2Destructive | Self::T3System)
    }

    /// True if the tier is permanently forbidden.
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked)
    }
}

impl std::fmt::Display for CapabilityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// The specific action requested by a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ActionKind {
    /// Read file content within allowlisted workspace.
    ReadFile { path: PathBuf },
    /// List entries in an allowlisted directory.
    ListDirectory { path: PathBuf },
    /// Query internal RAG library index status.
    LibraryStats,

    /// Create or append a draft/scratchpad document.
    WriteDraft { path: PathBuf, content: String },

    /// Overwrite an existing document in the workspace.
    OverwriteFile { path: PathBuf, content: String },
    /// Move a document to the OS recycle bin/trash.
    DeleteFile { path: PathBuf },
    /// Rename a document within the workspace.
    RenameFile { from: PathBuf, to: PathBuf },

    /// Execute an external executable with strictly separated arguments.
    ExecuteCommand { program: String, args: Vec<String> },
    /// Make an outbound network request.
    NetworkRequest { url: String, method: String },

    /// Modification of Orion's internal config, allowlist, or binary.
    ModifyOrionConfig { target: String },
    /// Access to system credentials (SSH, AWS, GPG, .env).
    AccessCredentials { target: String },
    /// Access to OS system root or system binaries.
    AccessSystemPath { target: String },
}

impl ActionKind {
    /// Returns the target identifier (path, URL, or resource name) for auditing.
    pub fn target_summary(&self) -> String {
        match self {
            Self::ReadFile { path }
            | Self::ListDirectory { path }
            | Self::WriteDraft { path, .. }
            | Self::OverwriteFile { path, .. }
            | Self::DeleteFile { path } => path.to_string_lossy().to_string(),
            Self::RenameFile { from, to } => {
                format!("{} -> {}", from.display(), to.display())
            }
            Self::LibraryStats => "library".to_string(),
            Self::ExecuteCommand { program, args } => {
                format!("{} {}", program, args.join(" "))
            }
            Self::NetworkRequest { url, method } => format!("{method} {url}"),
            Self::ModifyOrionConfig { target }
            | Self::AccessCredentials { target }
            | Self::AccessSystemPath { target } => target.clone(),
        }
    }

    /// Default baseline tier classification.
    pub fn default_tier(&self) -> CapabilityTier {
        match self {
            Self::ReadFile { .. } | Self::ListDirectory { .. } | Self::LibraryStats => {
                CapabilityTier::T0Read
            }
            Self::WriteDraft { .. } => CapabilityTier::T1Reversible,
            Self::OverwriteFile { .. } | Self::DeleteFile { .. } | Self::RenameFile { .. } => {
                CapabilityTier::T2Destructive
            }
            Self::ExecuteCommand { .. } | Self::NetworkRequest { .. } => {
                CapabilityTier::T3System
            }
            Self::ModifyOrionConfig { .. }
            | Self::AccessCredentials { .. }
            | Self::AccessSystemPath { .. } => CapabilityTier::Blocked,
        }
    }

    /// Returns human-readable description for confirmation dialogs.
    pub fn confirmation_prompt(&self) -> String {
        match self {
            Self::DeleteFile { path } => {
                format!("Permanently move '{}' to trash?", path.display())
            }
            Self::OverwriteFile { path, .. } => {
                format!("Overwrite existing file '{}'?", path.display())
            }
            Self::RenameFile { from, to } => {
                format!("Rename '{}' to '{}'?", from.display(), to.display())
            }
            Self::ExecuteCommand { program, args } => {
                format!("Run external process '{}' with arguments {:?}?", program, args)
            }
            Self::NetworkRequest { url, method } => {
                format!("Send outbound {method} request to '{url}'?")
            }
            _ => format!("Authorize action on '{}'?", self.target_summary()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_tier_confirmations() {
        assert!(!CapabilityTier::T0Read.requires_confirmation());
        assert!(!CapabilityTier::T1Reversible.requires_confirmation());
        assert!(CapabilityTier::T2Destructive.requires_confirmation());
        assert!(CapabilityTier::T3System.requires_confirmation());
        assert!(CapabilityTier::Blocked.is_blocked());
    }

    #[test]
    fn classifies_actions_accurately() {
        let read = ActionKind::ReadFile {
            path: PathBuf::from("/workspace/notes.md"),
        };
        assert_eq!(read.default_tier(), CapabilityTier::T0Read);

        let draft = ActionKind::WriteDraft {
            path: PathBuf::from("/workspace/draft.md"),
            content: "hello".into(),
        };
        assert_eq!(draft.default_tier(), CapabilityTier::T1Reversible);

        let delete = ActionKind::DeleteFile {
            path: PathBuf::from("/workspace/report.pdf"),
        };
        assert_eq!(delete.default_tier(), CapabilityTier::T2Destructive);

        let exec = ActionKind::ExecuteCommand {
            program: "curl".into(),
            args: vec!["https://example.com".into()],
        };
        assert_eq!(exec.default_tier(), CapabilityTier::T3System);

        let creds = ActionKind::AccessCredentials {
            target: "~/.ssh/id_rsa".into(),
        };
        assert_eq!(creds.default_tier(), CapabilityTier::Blocked);
    }
}
