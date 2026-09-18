//! Persistence for documents, chunks, vectors and the FTS index.
//!
//! Everything lives in the same SQLite file as chat history. One file means
//! one thing to back up, one thing to encrypt, and — importantly for an
//! offline product — nothing to configure.
//!
//! The vector search here is a brute-force scan in Rust rather than an ANN
//! index. That is a deliberate, measured choice: at 384 dimensions a dot
//! product over 100k chunks is ~38M multiply-adds, single-digit milliseconds.
//! A personal document corpus does not reach the size where an approximate
//! index pays for its complexity, and exact search removes a whole class of
//! recall bugs. Revisit only if a real corpus proves otherwise.

use rusqlite::{params, Connection};

use crate::error::{OrionError, Result};
use crate::ingest::IngestedDocument;
use crate::rag::embed::{from_blob, to_blob, EMBED_DIM, EMBED_MODEL_ID};
use crate::rag::search::{reciprocal_rank_fusion, sanitize_fts_query, vector_rank, Hit, Ranked};

/// How many candidates each retriever contributes before fusion. Wider than
/// the final result count on purpose — fusion can only reorder what it is
/// given, so starving either side defeats the point of hybrid search.
pub const CANDIDATES_PER_RETRIEVER: usize = 40;

/// Relative trust in each retriever. Slightly favouring vectors reflects that
/// most questions are paraphrases; BM25 earns its place on the minority of
/// queries containing exact identifiers, where it wins outright anyway.
pub const WEIGHT_VECTOR: f32 = 1.0;
pub const WEIGHT_BM25: f32 = 0.8;

/// Schema version for the RAG tables, tracked separately from the chat schema
/// so the two can evolve independently.
const RAG_SCHEMA_VERSION: i32 = 1;

pub struct RagStore<'a> {
    conn: &'a Connection,
}

impl<'a> RagStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create the RAG tables. Separate from the chat migration ladder because
    /// a user who never opens a document should not pay for these tables.
    pub fn migrate(&self) -> Result<()> {
        let current: i32 = self
            .conn
            .query_row(
                "SELECT COALESCE((SELECT CAST(value AS INTEGER) FROM settings
                                  WHERE key = 'rag_schema_version'), 0)",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if current >= RAG_SCHEMA_VERSION {
            return Ok(());
        }

        if current < 1 {
            self.conn
                .execute_batch(
                    r#"
                    CREATE TABLE documents (
                        id            TEXT PRIMARY KEY,
                        name          TEXT NOT NULL,
                        format        TEXT NOT NULL,
                        chunk_count   INTEGER NOT NULL DEFAULT 0,
                        indexed_at    TEXT NOT NULL,
                        -- Which embedding model produced this document's
                        -- vectors. Changing models invalidates them.
                        embed_model   TEXT NOT NULL
                    );

                    CREATE TABLE chunks (
                        id            INTEGER PRIMARY KEY AUTOINCREMENT,
                        document_id   TEXT NOT NULL
                                      REFERENCES documents(id) ON DELETE CASCADE,
                        ordinal       INTEGER NOT NULL,
                        body          TEXT NOT NULL,
                        breadcrumb    TEXT NOT NULL DEFAULT '',
                        page          INTEGER,
                        embedding     BLOB
                    );

                    CREATE INDEX idx_chunks_document ON chunks(document_id, ordinal);

                    -- `content=''` makes this a contentless FTS table: the text
                    -- is stored once in `chunks`, not duplicated here. Halves
                    -- the database size on a large corpus.
                    CREATE VIRTUAL TABLE chunks_fts USING fts5(
                        text,
                        content='',
                        tokenize='unicode61 remove_diacritics 2'
                    );
                    "#,
                )
                .map_err(|e| OrionError::Db(format!("rag migration v1 failed: {e}")))?;
        }

        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES ('rag_schema_version', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![RAG_SCHEMA_VERSION.to_string()],
            )
            .map_err(|e| OrionError::Db(format!("cannot record rag schema version: {e}")))?;

        Ok(())
    }

    /// Insert a document and its chunks, replacing any previous version.
    ///
    /// `embeddings` must be parallel to `doc.chunks`. Re-ingesting is a
    /// delete-then-insert so an edited file cannot leave stale chunks behind —
    /// stale chunks are worse than missing ones, because they get cited.
    pub fn upsert_document(
        &self,
        doc: &IngestedDocument,
        embeddings: &[Vec<f32>],
    ) -> Result<usize> {
        if embeddings.len() != doc.chunks.len() {
            return Err(OrionError::Db(format!(
                "embedding/chunk mismatch: {} vs {}",
                embeddings.len(),
                doc.chunks.len()
            )));
        }
        for e in embeddings {
            if e.len() != EMBED_DIM {
                return Err(OrionError::Db(format!(
                    "embedding has {} dims, expected {EMBED_DIM}",
                    e.len()
                )));
            }
        }

        self.delete_document(&doc.id)?;

        self.conn
            .execute(
                "INSERT INTO documents (id, name, format, chunk_count, indexed_at, embed_model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    doc.id,
                    doc.name,
                    doc.format.label(),
                    doc.chunks.len() as i64,
                    chrono::Utc::now().to_rfc3339(),
                    EMBED_MODEL_ID,
                ],
            )
            .map_err(|e| OrionError::Db(format!("cannot insert document: {e}")))?;

        for (chunk, embedding) in doc.chunks.iter().zip(embeddings) {
            self.conn
                .execute(
                    "INSERT INTO chunks
                         (document_id, ordinal, body, breadcrumb, page, embedding)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        doc.id,
                        chunk.index as i64,
                        chunk.body,
                        chunk.breadcrumb(),
                        chunk.page.map(|p| p as i64),
                        to_blob(embedding),
                    ],
                )
                .map_err(|e| OrionError::Db(format!("cannot insert chunk: {e}")))?;

            let rowid = self.conn.last_insert_rowid();

            // The FTS row is keyed to the chunk rowid so the two stay joined.
            // We index `chunk.text` (which carries the heading prefix), not
            // `body`, so a query matching a section title finds the section.
            self.conn
                .execute(
                    "INSERT INTO chunks_fts (rowid, text) VALUES (?1, ?2)",
                    params![rowid, chunk.text],
                )
                .map_err(|e| OrionError::Db(format!("cannot index chunk text: {e}")))?;
        }

        Ok(doc.chunks.len())
    }

    /// Remove a document, its chunks and its FTS rows.
    pub fn delete_document(&self, document_id: &str) -> Result<()> {
        // Contentless FTS tables do not cascade, so delete their rows first
        // while we can still read the chunk ids.
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM chunks WHERE document_id = ?1")
            .map_err(|e| OrionError::Db(format!("cannot list chunks: {e}")))?;

        let ids: Vec<i64> = stmt
            .query_map(params![document_id], |r| r.get(0))
            .map_err(|e| OrionError::Db(format!("cannot list chunks: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        for id in ids {
            // 'delete' is the documented incantation for removing a row from a
            // contentless FTS5 table.
            let _ = self.conn.execute(
                "INSERT INTO chunks_fts (chunks_fts, rowid, text) VALUES ('delete', ?1, '')",
                params![id],
            );
        }

        self.conn
            .execute(
                "DELETE FROM chunks WHERE document_id = ?1",
                params![document_id],
            )
            .map_err(|e| OrionError::Db(format!("cannot delete chunks: {e}")))?;
        self.conn
            .execute("DELETE FROM documents WHERE id = ?1", params![document_id])
            .map_err(|e| OrionError::Db(format!("cannot delete document: {e}")))?;

        Ok(())
    }

    pub fn document_count(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .map_err(|e| OrionError::Db(format!("cannot count documents: {e}")))
    }

    pub fn chunk_count(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .map_err(|e| OrionError::Db(format!("cannot count chunks: {e}")))
    }

    /// BM25 candidates from FTS5.
    fn bm25_candidates(&self, query: &str, limit: usize) -> Result<Vec<Ranked>> {
        let fts_query = sanitize_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rowid FROM chunks_fts
                 WHERE chunks_fts MATCH ?1
                 ORDER BY bm25(chunks_fts) ASC
                 LIMIT ?2",
            )
            .map_err(|e| OrionError::Db(format!("cannot prepare fts query: {e}")))?;

        let rows: Vec<i64> = stmt
            .query_map(params![fts_query, limit as i64], |r| r.get(0))
            .map_err(|e| OrionError::Db(format!("fts query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(rank, chunk_id)| Ranked { chunk_id, rank })
            .collect())
    }

    /// Vector candidates by exact cosine similarity.
    fn vector_candidates(&self, query_vec: &[f32], limit: usize) -> Result<Vec<Ranked>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL")
            .map_err(|e| OrionError::Db(format!("cannot prepare vector scan: {e}")))?;

        let corpus: Vec<(i64, Vec<f32>)> = stmt
            .query_map([], |r| {
                let id: i64 = r.get(0)?;
                let blob: Vec<u8> = r.get(1)?;
                Ok((id, blob))
            })
            .map_err(|e| OrionError::Db(format!("vector scan failed: {e}")))?
            .filter_map(|r| r.ok())
            // A corrupt vector is skipped, not fatal: one bad row must not
            // make the whole library unsearchable.
            .filter_map(|(id, blob)| from_blob(&blob).ok().map(|v| (id, v)))
            .filter(|(_, v)| v.len() == EMBED_DIM)
            .collect();

        Ok(vector_rank(query_vec, &corpus, limit))
    }

    /// Hybrid search. `query_vec` is the embedded question; `query` is its
    /// raw text for keyword matching.
    pub fn search(&self, query: &str, query_vec: &[f32], limit: usize) -> Result<Vec<Hit>> {
        let bm25 = self.bm25_candidates(query, CANDIDATES_PER_RETRIEVER)?;
        let vectors = self.vector_candidates(query_vec, CANDIDATES_PER_RETRIEVER)?;

        let fused = reciprocal_rank_fusion(
            &[
                ("bm25", bm25, WEIGHT_BM25),
                ("vector", vectors, WEIGHT_VECTOR),
            ],
            limit,
        );

        let mut hits = Vec::with_capacity(fused.len());
        for (chunk_id, score, sources) in fused {
            if let Some(mut hit) = self.load_hit(chunk_id)? {
                hit.score = score;
                hit.sources = sources.iter().map(|s| s.to_string()).collect();
                hits.push(hit);
            }
        }
        Ok(hits)
    }

    fn load_hit(&self, chunk_id: i64) -> Result<Option<Hit>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT c.id, c.document_id, d.name, c.body, c.page, c.breadcrumb
                 FROM chunks c
                 JOIN documents d ON d.id = c.document_id
                 WHERE c.id = ?1",
            )
            .map_err(|e| OrionError::Db(format!("cannot prepare hit load: {e}")))?;

        let mut rows = stmt
            .query(params![chunk_id])
            .map_err(|e| OrionError::Db(format!("cannot load hit: {e}")))?;

        if let Some(row) = rows
            .next()
            .map_err(|e| OrionError::Db(format!("cannot read hit: {e}")))?
        {
            Ok(Some(Hit {
                chunk_id: row.get(0).unwrap_or(chunk_id),
                document_id: row.get(1).unwrap_or_default(),
                document_name: row.get(2).unwrap_or_default(),
                text: row.get(3).unwrap_or_default(),
                page: row
                    .get::<_, Option<i64>>(4)
                    .ok()
                    .flatten()
                    .map(|p| p as u32),
                breadcrumb: row.get(5).unwrap_or_default(),
                score: 0.0,
                sources: Vec::new(),
            }))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::chunker::Chunk;
    use crate::ingest::Format;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        RagStore::new(&conn).migrate().unwrap();
        conn
    }

    fn chunk(index: usize, body: &str, heading: &str) -> Chunk {
        Chunk {
            text: if heading.is_empty() {
                body.to_string()
            } else {
                format!("{heading}\n{body}")
            },
            body: body.to_string(),
            page: Some(1),
            headings: if heading.is_empty() {
                vec![]
            } else {
                vec![heading.to_string()]
            },
            index,
        }
    }

    fn doc(id: &str, name: &str, chunks: Vec<Chunk>) -> IngestedDocument {
        IngestedDocument {
            id: id.into(),
            name: name.into(),
            format: Format::Markdown,
            chunks,
        }
    }

    fn vec_for(seed: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; EMBED_DIM];
        v[0] = seed;
        v[1] = 1.0 - seed;
        crate::rag::embed::normalize(v)
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = setup();
        RagStore::new(&conn).migrate().unwrap();
        RagStore::new(&conn).migrate().unwrap();
        assert_eq!(RagStore::new(&conn).document_count().unwrap(), 0);
    }

    #[test]
    fn upsert_stores_documents_and_chunks() {
        let conn = setup();
        let store = RagStore::new(&conn);
        let d = doc(
            "doc1",
            "contract.md",
            vec![chunk(0, "Thirty days notice.", "Termination")],
        );
        let n = store.upsert_document(&d, &[vec_for(1.0)]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(store.document_count().unwrap(), 1);
        assert_eq!(store.chunk_count().unwrap(), 1);
    }

    #[test]
    fn reingesting_replaces_rather_than_duplicates() {
        let conn = setup();
        let store = RagStore::new(&conn);

        let v1 = doc("doc1", "notes.md", vec![chunk(0, "Old content here.", "")]);
        store.upsert_document(&v1, &[vec_for(1.0)]).unwrap();

        let v2 = doc(
            "doc1",
            "notes.md",
            vec![chunk(0, "New content here.", ""), chunk(1, "And more.", "")],
        );
        store
            .upsert_document(&v2, &[vec_for(1.0), vec_for(0.5)])
            .unwrap();

        assert_eq!(store.document_count().unwrap(), 1);
        assert_eq!(store.chunk_count().unwrap(), 2, "stale chunks must be gone");

        // The old text must not be findable — a stale chunk would get cited.
        let hits = store.search("Old content", &vec_for(1.0), 10).unwrap();
        assert!(
            hits.iter().all(|h| !h.text.contains("Old content")),
            "stale chunk survived re-ingestion"
        );
    }

    #[test]
    fn deleting_a_document_removes_it_from_search() {
        let conn = setup();
        let store = RagStore::new(&conn);
        let d = doc("doc1", "x.md", vec![chunk(0, "Unique marker phrase.", "")]);
        store.upsert_document(&d, &[vec_for(1.0)]).unwrap();
        assert!(!store
            .search("Unique marker", &vec_for(1.0), 5)
            .unwrap()
            .is_empty());

        store.delete_document("doc1").unwrap();
        assert_eq!(store.chunk_count().unwrap(), 0);
        assert!(
            store
                .search("Unique marker", &vec_for(1.0), 5)
                .unwrap()
                .is_empty(),
            "FTS rows leaked after delete"
        );
    }

    #[test]
    fn mismatched_embedding_count_is_rejected() {
        let conn = setup();
        let store = RagStore::new(&conn);
        let d = doc("d", "x.md", vec![chunk(0, "a", ""), chunk(1, "b", "")]);
        assert!(store.upsert_document(&d, &[vec_for(1.0)]).is_err());
    }

    #[test]
    fn wrong_dimension_embedding_is_rejected() {
        let conn = setup();
        let store = RagStore::new(&conn);
        let d = doc("d", "x.md", vec![chunk(0, "a", "")]);
        assert!(store.upsert_document(&d, &[vec![0.1, 0.2]]).is_err());
    }

    #[test]
    fn search_returns_citation_metadata() {
        let conn = setup();
        let store = RagStore::new(&conn);
        let d = doc(
            "doc1",
            "handbook.md",
            vec![chunk(0, "Employees accrue leave monthly.", "Leave Policy")],
        );
        store.upsert_document(&d, &[vec_for(1.0)]).unwrap();

        let hits = store.search("leave accrual", &vec_for(1.0), 5).unwrap();
        assert!(!hits.is_empty());
        let h = &hits[0];
        assert_eq!(h.document_name, "handbook.md");
        assert_eq!(h.breadcrumb, "Leave Policy");
        assert_eq!(h.page, Some(1));
        assert!(!h.sources.is_empty(), "provenance must be recorded");
    }

    #[test]
    fn heading_only_query_finds_the_section() {
        // The whole point of prefixing the heading into the indexed text.
        let conn = setup();
        let store = RagStore::new(&conn);
        let d = doc(
            "doc1",
            "contract.md",
            vec![chunk(
                0,
                "Either party may end this with 30 days notice.",
                "Termination",
            )],
        );
        store.upsert_document(&d, &[vec_for(0.0)]).unwrap();

        // Query the heading word, which never appears in the body.
        let hits = store.search("termination", &vec_for(1.0), 5).unwrap();
        assert!(
            hits.iter().any(|h| h.breadcrumb == "Termination"),
            "heading trail is not being indexed"
        );
    }

    #[test]
    fn search_survives_hostile_query_syntax() {
        let conn = setup();
        let store = RagStore::new(&conn);
        let d = doc("doc1", "x.md", vec![chunk(0, "Normal text.", "")]);
        store.upsert_document(&d, &[vec_for(1.0)]).unwrap();

        for evil in ["\" OR 1=1 --", "NEAR(a b)", "*", "((((", "col:x", "'"] {
            let r = store.search(evil, &vec_for(1.0), 5);
            assert!(r.is_ok(), "query {evil:?} caused an error: {r:?}");
        }
    }

    #[test]
    fn search_on_an_empty_library_is_empty_not_an_error() {
        let conn = setup();
        let store = RagStore::new(&conn);
        assert!(store
            .search("anything", &vec_for(1.0), 5)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn search_respects_the_limit() {
        let conn = setup();
        let store = RagStore::new(&conn);
        let chunks: Vec<Chunk> = (0..20)
            .map(|i| {
                chunk(
                    i,
                    &format!("Document paragraph number {i} about policy."),
                    "",
                )
            })
            .collect();
        let embeds: Vec<Vec<f32>> = (0..20).map(|i| vec_for(i as f32 / 20.0)).collect();
        store
            .upsert_document(&doc("d", "big.md", chunks), &embeds)
            .unwrap();

        assert_eq!(store.search("policy", &vec_for(0.5), 3).unwrap().len(), 3);
    }

    #[test]
    fn a_corrupt_vector_row_does_not_break_search() {
        let conn = setup();
        let store = RagStore::new(&conn);
        let d = doc("doc1", "x.md", vec![chunk(0, "Findable text.", "")]);
        store.upsert_document(&d, &[vec_for(1.0)]).unwrap();

        // Simulate disk corruption on the vector column.
        conn.execute("UPDATE chunks SET embedding = X'0102' WHERE id = 1", [])
            .unwrap();

        let hits = store.search("Findable", &vec_for(1.0), 5).unwrap();
        assert_eq!(hits.len(), 1, "BM25 should still find it via the FTS index");
    }

    #[test]
    fn documents_from_different_files_are_isolated() {
        let conn = setup();
        let store = RagStore::new(&conn);
        store
            .upsert_document(
                &doc("a", "a.md", vec![chunk(0, "alpha unique term", "")]),
                &[vec_for(1.0)],
            )
            .unwrap();
        store
            .upsert_document(
                &doc("b", "b.md", vec![chunk(0, "beta unique term", "")]),
                &[vec_for(0.0)],
            )
            .unwrap();

        store.delete_document("a").unwrap();
        assert_eq!(store.chunk_count().unwrap(), 1);
        let hits = store.search("beta", &vec_for(0.0), 5).unwrap();
        assert_eq!(hits[0].document_name, "b.md");
    }
}
