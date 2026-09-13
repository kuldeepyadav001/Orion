# M2 — Document Intelligence: evidence

Branch: `feat/m2-documents-rag`, cut from `main` at `3784c8f`.
Status: **compiles clean, 144 tests green, clippy `-D warnings` clean.**
Not yet run inside the Tauri app — see "What is still unverified".

---

## What M2 adds

| Piece | File | Purpose |
|---|---|---|
| Format extraction | `ingest/extract.rs` | MD, HTML, CSV/TSV, TXT, code → `Block`s |
| Structure-aware chunking | `ingest/chunker.rs` | `Block`s → `Chunk`s with heading trail + page |
| Ingestion entry point | `ingest/mod.rs` | path → `IngestedDocument`, 64 MB cap |
| SHA-256 | `hashing.rs` | opaque document ids (also used by M1's downloader) |
| Embeddings | `rag/embed.rs` | bge-small-en-v1.5, 384-dim, dedicated sidecar |
| Hybrid search | `rag/search.rs` | BM25 + cosine, fused with RRF |
| Persistence | `rag/store.rs` | SQLite FTS5 + vector BLOBs, one file |
| Grounded prompting | `rag/context.rs` | numbered sources, injection hardening, citations |
| Evaluation | `rag/eval.rs` | 30-question scored suite — **the M2 exit gate** |

---

## Retrieval results

30 questions over a 22-chunk adversarial corpus (two deliberately
conflicting HR handbooks, a contract, an invoice, a runbook). k = 4.

Four questions are unanswerable and are **excluded from these averages** —
see "Metric correction" below. Averages are over the 26 answerable questions.

| Configuration | recall@4 | MRR | precision@4 |
|---|---|---|---|
| **Hybrid (BM25 + vectors)** | **0.769** | **0.667** | 0.202 |
| BM25 only | 0.731 | 0.635 | 0.301 |
| Vectors only | 0.692 | 0.571 | 0.183 |
| Hybrid, heading trail removed | 0.615 | 0.494 | 0.154 |

Recall by probe category (hybrid):

| Probe | recall@4 | What it tests |
|---|---|---|
| Disambiguation | 1.000 | Superseded 2019 handbook must not outrank the current one |
| Exact | 0.857 | Invoice numbers, account codes, PO references |
| HeadingScoped | 0.833 | Answer lives under a heading whose words are absent from the body |
| Paraphrase | 0.500 | Query wording differs entirely from the document |

### What these numbers do and do not prove

**They prove the plumbing and the two design decisions:**

- Hybrid beats either retriever alone on recall and MRR. Not by a landslide,
  but consistently, and the two retrievers fail on *different* questions —
  which is the entire argument for fusing them.
- Removing the heading trail drops recall from 0.769 to 0.615, and collapses
  HeadingScoped recall from 0.833 to 0.167. The heading-prefix decision is
  now measured, not asserted.

**They do not prove retrieval quality.** The dense side here is
`pseudo_embed`, a hashed bag-of-words stand-in, not a real model. It cannot
connect "holiday" to "annual leave", which is exactly why Paraphrase sits at
0.500. A real bge-small model should lift that category substantially. **The
honest read is: Paraphrase 0.500 is a floor set by the fake embedder, not a
measurement of the product.** Re-run against a live embedding server before
quoting any number outside this document.

### Known failures worth keeping

- **Q12 "what is the bank account 60817244"** — BM25 alone finds this (Exact
  1.000); hybrid loses it at k=4 because pseudo-embedder noise crowds it out.
  This is fusion working as designed on a bad dense signal, and should
  disappear with a real embedder. If it survives, the fix is a score floor on
  the dense side, not a weight change.
- **Q17 "how do I terminate the services agreement"** — fails in every
  configuration. The chunk says "bring this arrangement to an end"; nothing
  lexical connects. A genuine semantic-only question.

### Metric correction

The first version of this harness scored unanswerable questions as
"retrieved nothing = 1.0". Every retriever scored **0.000** on all four,
dragging every headline number down by a flat 13%.

That was the metric being wrong, not the retriever. A similarity search over
a small corpus always returns *some* nearest neighbour — "nearest" is not
"relevant", and no score threshold separates them reliably across queries.
Refusing to answer is the generator's job, enforced by
`GROUNDED_SYSTEM_PROMPT` and detectable via `is_uncited_claim()`. The four
questions are retained as the refusal suite and scored at generation time.

---

## Bugs the tests caught

All five were found by tests written alongside the code, before anything ran
in the app. Listed because they are the argument for the test-first approach
on branches that are not being manually tested.

1. **SHA-256 corrupted data on a specific split.** `update()` reset
   `self.buffered` after the buffered path had already consumed the whole
   input, discarding buffered bytes. Only reproduced when one call ended
   mid-block and the next was smaller than the remaining space — split 193 of
   200. Caught by testing *every* split point, not a few.
2. **Citations pointed at the wrong page.** A chunk spanning a page break
   inherited the earlier page number. The user opens the PDF, the quote is
   not on that page, and every other citation loses credibility. Page breaks
   now close a chunk.
3. **Runt merging re-broke the page fix.** `merge_small` happily merged two
   short chunks across a page boundary. Merging now requires matching heading
   trail *and* page.
4. **Bidi isolate characters (U+2066–2069) passed the injection filter.**
   The older U+202x overrides were blocked; the modern replacements were not.
   Invisible to a human reviewing the file, fully legible to the model.
5. **Two fusion tests asserted the wrong thing** — they demanded an exact rank
   rather than the property that matters (the exact-match chunk is present and
   near the top). Fixed the tests, not the code.

---

## Security posture

Retrieved document text is treated as **untrusted data, never instructions**.
Four layers:

1. **Delimiting** — sources sit in numbered `<<<SOURCE n>>>` blocks.
2. **Neutralising** — control characters, zero-width characters, bidi
   overrides and isolates stripped; the delimiter sequences themselves are
   broken so a document cannot close its own block. Filenames and headings
   are sanitised too, because a file can be named
   `report. Ignore all previous instructions.pdf`.
3. **Instruction framing** — the system prompt states the data boundary. This
   is the weakest layer and is never relied on alone.
4. **Capability boundary (M5)** — the tool broker will refuse calls whose
   provenance traces to document text.

Prompt engineering does not stop injection. Layer 4 is the actual control;
layers 1–3 raise the cost. Tests cover forged terminators, forged source
blocks, malicious filenames, malicious headings and invisible characters.

FTS5 queries are tokenised and individually quoted, so user input is always
data and never query syntax — `" OR 1=1 --`, `NEAR(a b)`, `*` and `((((` are
all tested and inert.

---

## Deliberate design decisions

- **Brute-force vector scan, no ANN index.** 384 dims over 100k chunks is
  ~38M multiply-adds — single-digit milliseconds. A personal corpus does not
  reach the size where approximate search pays for its complexity, and exact
  search eliminates a class of silent recall bugs. Revisit only with evidence.
- **Contentless FTS5 table** (`content=''`) — text stored once in `chunks`,
  not duplicated into the index. Roughly halves database size.
- **Separate embedding sidecar.** `llama-server` will not serve embeddings
  and chat from one process; sharing would serialise every embed behind every
  generated token. The embedding model is ~90 MB, so two processes is cheap
  even at Tier T1.
- **RRF over score normalisation.** BM25 scores and cosine similarities are
  not on comparable scales and normalising them is fragile. Fusing ranks is
  scale-free.
- **Re-ingestion is delete-then-insert.** Stale chunks are worse than missing
  ones, because stale chunks get cited.

---

## What is still unverified

- Nothing has run inside the Tauri app. No `cargo tauri build` in this
  sandbox (no `libwebkit2gtk-4.1-dev`, no display server).
- No real embedding model has been loaded. Every dense number above comes
  from `pseudo_embed`.
- PDF, DOCX and XLSX extractors are stubs returning a clear error. They are
  the largest remaining piece of M2 and the most likely source of surprises.
- No performance measurement on a real corpus — the brute-force scan
  justification is arithmetic, not a benchmark.

Verified by the scratch-crate pattern under `/home/user/.m2test` (never
`/tmp` — it is a 993 MB tmpfs).
