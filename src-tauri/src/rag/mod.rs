//! Retrieval-augmented generation.
//!
//! Pipeline: `ingest` produces chunks → `embed` vectorises them → `store`
//! persists and searches → `search` fuses rankings → `context` assembles a
//! grounded, injection-hardened prompt and maps the answer back to citations.
//!
//! The design answers the three things Serina got wrong:
//! * fixed-size chunks that cut sentences in half → structure-aware chunking,
//! * dense-only retrieval that missed exact identifiers → hybrid BM25 + vectors,
//! * answers with no provenance → mandatory citation markers.

pub mod context;
pub mod embed;
pub mod eval;
pub mod search;
pub mod store;

pub use context::{build_context, Citation, GroundedContext, GROUNDED_SYSTEM_PROMPT};
pub use embed::{EmbedConfig, EmbedKind, Embedder, EMBED_DIM, EMBED_MODEL_ID};
pub use search::Hit;
pub use store::RagStore;
