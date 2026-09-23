//! Two-domain separation model (M5).
//!
//! ## Architectural Invariant
//!
//! An AI agent executing arbitrary tool actions while reading untrusted external
//! inputs (documents, emails, web pages) is fundamentally susceptible to indirect
//! prompt injection.
//!
//! To prevent arbitrary code execution and unauthorized data access:
//!
//! 1. **Trusted Domain:** Input originating directly from the authentic user
//!    (keystrokes in the UI, speech arriving from the local microphone).
//!    Only Trusted Domain directives may request capability execution.
//!
//! 2. **Untrusted Domain:** Content ingested from outside sources (RAG document
//!    chunks, email bodies, web pages). Untrusted Domain inputs have **strictly
//!    zero capability/tool access**. They can inform text generation, but can
//!    never trigger a tool invocation or configuration change.

use serde::{Deserialize, Serialize};

use crate::error::{OrionError, Result};

/// Trust classification for an instruction or data payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    /// Direct user interaction (keyboard, local mic, internal system timer).
    Trusted,
    /// Ingested or external data (documents, emails, web pages, remote sockets).
    Untrusted,
}

impl std::fmt::Display for Domain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trusted => write!(f, "trusted"),
            Self::Untrusted => write!(f, "untrusted"),
        }
    }
}

/// The concrete source of an input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DomainSource {
    /// Keystrokes directly entered by the user in the desktop UI.
    UserKeyboard,
    /// Local microphone audio transcribed by the onboard Whisper engine.
    VerifiedVoice,
    /// Internal app event (e.g. idle watchdog timer, startup sweep).
    SystemInternal,
    /// Ingested document chunk retrieved via RAG search.
    DocumentChunk {
        doc_id: String,
        doc_name: String,
    },
    /// External email body retrieved via IMAP (M7).
    EmailBody {
        sender: String,
    },
    /// External webpage scraped or snapshotted via browser (M8).
    WebPage {
        url: String,
    },
}

impl DomainSource {
    /// Returns the trust domain associated with this source.
    pub fn domain(&self) -> Domain {
        match self {
            Self::UserKeyboard | Self::VerifiedVoice | Self::SystemInternal => Domain::Trusted,
            Self::DocumentChunk { .. } | Self::EmailBody { .. } | Self::WebPage { .. } => {
                Domain::Untrusted
            }
        }
    }

    /// True if the source is within the trusted domain.
    pub fn is_trusted(&self) -> bool {
        self.domain() == Domain::Trusted
    }
}

/// A directive requesting an action, bound to its source domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directive {
    pub id: String,
    pub source: DomainSource,
    pub intention: String,
}

impl Directive {
    /// Create a new directive from a verified user input.
    pub fn from_user(intention: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source: DomainSource::UserKeyboard,
            intention: intention.into(),
        }
    }

    /// Create a new directive from speech input.
    pub fn from_voice(intention: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source: DomainSource::VerifiedVoice,
            intention: intention.into(),
        }
    }

    /// Create a directive associated with an untrusted document chunk.
    pub fn from_document(doc_id: &str, doc_name: &str, intention: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source: DomainSource::DocumentChunk {
                doc_id: doc_id.to_string(),
                doc_name: doc_name.to_string(),
            },
            intention: intention.into(),
        }
    }

    /// Verify whether this directive is allowed to trigger tool capabilities.
    pub fn assert_trusted_for_tools(&self) -> Result<()> {
        if !self.source.is_trusted() {
            return Err(OrionError::Security(format!(
                "permission denied: action requested from untrusted domain source ({:?})",
                self.source
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_sources_pass_domain_check() {
        let user = Directive::from_user("create a note");
        assert!(user.source.is_trusted());
        assert!(user.assert_trusted_for_tools().is_ok());

        let voice = Directive::from_voice("open documents");
        assert!(voice.source.is_trusted());
        assert!(voice.assert_trusted_for_tools().is_ok());
    }

    #[test]
    fn untrusted_document_fails_domain_check() {
        let doc = Directive::from_document("doc-1", "invoice.pdf", "rm -rf /");
        assert!(!doc.source.is_trusted());
        let res = doc.assert_trusted_for_tools();
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("permission denied"));
    }
}
