//! Central error type.
//!
//! Errors that cross into the webview are serialised as plain strings. We keep
//! them descriptive but never include paths to secrets, tokens, or the
//! sidecar's port — the renderer is the least trusted part of the app.

use serde::{Serialize, Serializer};

pub type Result<T> = std::result::Result<T, OrionError>;

#[derive(Debug, thiserror::Error)]
pub enum OrionError {
    #[error("engine error: {0}")]
    Engine(String),

    #[error("database error: {0}")]
    Db(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("no model is installed")]
    NoModel,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl Serialize for OrionError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
