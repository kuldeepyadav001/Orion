//! Document ingestion: turn an arbitrary file on disk into retrievable chunks.
//!
//! The pipeline is deliberately two-stage:
//!
//! 1. `extract` — format-specific parsing into a flat `Vec<Block>`. This is
//!    the only code that knows what a PDF or a spreadsheet is.
//! 2. `chunker` — format-agnostic assembly of blocks into `Chunk`s sized for
//!    a small local model's context window.
//!
//! Keeping them apart means adding a new file format never touches chunking
//! logic, and improving chunking never risks breaking a parser.
//!
//! **Everything here treats input as hostile.** Documents arrive from email
//! attachments, downloads and shared drives; a parser that panics takes the
//! app down, and text that reaches the model can carry prompt injection.

pub mod chunker;
pub mod extract;

use std::path::Path;

use crate::error::{OrionError, Result};

/// Hard ceiling on the size of a file we will open. Matches the extractor's
/// internal cap; checked here first so we never read the bytes at all.
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

pub use chunker::{chunk_blocks, Block, Chunk, ChunkConfig};
pub use extract::{extract, Format};

/// A document that has been read and split, ready to be embedded and stored.
#[derive(Debug, Clone)]
pub struct IngestedDocument {
    /// Stable identifier derived from the absolute path, so re-ingesting the
    /// same file replaces its chunks instead of duplicating them.
    pub id: String,
    /// Display name shown in citations.
    pub name: String,
    pub format: Format,
    pub chunks: Vec<Chunk>,
}

/// Read, parse and chunk a file in one call.
///
/// The file is refused before it is read if it is implausibly large, so a
/// hostile or corrupt file cannot exhaust memory during ingestion.
pub fn ingest_path(path: &Path, config: &ChunkConfig) -> Result<IngestedDocument> {
    let format = Format::from_path(path).ok_or_else(|| {
        OrionError::Config(format!(
            "unsupported file type: {}",
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("(no extension)")
        ))
    })?;

    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(OrionError::Config(format!(
            "file is {} MB, which exceeds the {} MB ingestion limit",
            meta.len() / 1024 / 1024,
            MAX_FILE_BYTES / 1024 / 1024
        )));
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    let bytes = std::fs::read(path)?;
    let blocks = extract(format, &bytes, &name)?;
    let chunks = chunk_blocks(&blocks, config);

    Ok(IngestedDocument {
        id: document_id(path),
        name,
        format,
        chunks,
    })
}

/// Deterministic document id.
///
/// We hash rather than store the path verbatim because the id is handed to
/// the webview in citations, and the absolute path can leak the user's name,
/// employer or directory layout.
pub fn document_id(path: &Path) -> String {
    let canonical = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let digest = crate::hashing::sha256_hex(canonical.as_bytes());
    digest[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn document_id_is_stable_and_opaque() {
        let p = PathBuf::from("/home/alice/secret-project/plan.md");
        let a = document_id(&p);
        let b = document_id(&p);
        assert_eq!(a, b, "same path must give the same id");
        assert_eq!(a.len(), 32);
        assert!(!a.contains("alice"), "the id must not leak the path");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_paths_give_different_ids() {
        assert_ne!(
            document_id(&PathBuf::from("/a/one.md")),
            document_id(&PathBuf::from("/a/two.md"))
        );
    }
}
