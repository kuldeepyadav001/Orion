//! Structure-aware text chunking.
//!
//! Serina sliced text into fixed 500-character windows per page. That cuts
//! sentences mid-word, destroys tables, and produces chunks that retrieve
//! badly because half of each one is unrelated to the other half.
//!
//! This chunker respects document structure instead:
//!
//! * split on **paragraph** boundaries first, then **sentences**, and only
//!   fall back to a hard character cut for pathological input (a single
//!   50 KB line with no punctuation)
//! * carry the **heading trail** into every chunk, so a chunk from under
//!   "3.2 Termination" retrieves for a query about termination even when the
//!   word never appears in the body text
//! * keep **page and section metadata** for citations
//! * never emit a chunk that is only whitespace or punctuation

use serde::{Deserialize, Serialize};

/// A unit of text extracted from a document, ready to embed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chunk {
    /// Text that gets embedded. Includes the heading trail as a prefix.
    pub text: String,
    /// Body text without the heading prefix, for display.
    pub body: String,
    /// 1-based page number when the source has pages.
    pub page: Option<u32>,
    /// Heading trail, outermost first, e.g. ["3. Terms", "3.2 Termination"].
    pub headings: Vec<String>,
    /// Position within the document, used for stable ordering.
    pub index: usize,
}

impl Chunk {
    /// Heading trail rendered for display, e.g. "3. Terms › 3.2 Termination".
    pub fn breadcrumb(&self) -> String {
        self.headings.join(" › ")
    }
}

/// An input block produced by a format extractor.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph(String),
    /// Pre-rendered table row; kept whole so columns stay together.
    TableRow(String),
    /// Code or other content where line structure matters.
    Pre(String),
    PageBreak(u32),
}

#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Target chunk size in characters.
    pub target: usize,
    /// Never exceed this, even mid-sentence.
    pub max: usize,
    /// Merge any chunk below this into its neighbour.
    pub min: usize,
    /// Characters of trailing context repeated into the next chunk.
    pub overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        // The text that gets embedded is the heading trail plus the body,
        // and it has to fit the embedding model's window: bge-small encodes
        // 512 tokens and refuses anything longer. `MAX_EMBED_CHARS` is the
        // character budget that is guaranteed to fit, measured against the
        // real model rather than guessed at — see `rag::embed`.
        //
        // The previous budget of 1800 characters was described as "roughly
        // 300 tokens". Measured, that was true of prose and badly wrong of
        // a numeric table, where WordPiece spends nearly a token per
        // character: one dense chunk was enough to make the server refuse
        // the whole batch, and the document silently lost its semantic
        // index.
        let max = crate::rag::embed::MAX_EMBED_CHARS;
        Self {
            // Four fifths of the budget, so that a chunk plus its heading
            // trail still lands inside it.
            target: max * 4 / 5,
            max,
            min: 120,
            overlap: 150,
        }
    }
}

/// Split blocks into chunks.
pub fn chunk_blocks(blocks: &[Block], cfg: &ChunkConfig) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();

    // Active heading trail, indexed by level.
    let mut trail: Vec<(u8, String)> = Vec::new();
    let mut page: Option<u32> = None;

    // Accumulator for the current chunk.
    let mut buf = String::new();
    let mut buf_page: Option<u32> = None;
    let mut buf_trail: Vec<String> = Vec::new();

    macro_rules! flush {
        () => {
            let text = buf.trim().to_string();
            if !text.is_empty() && has_content(&text) {
                // The heading trail is prepended to the body when the chunk
                // is built, so it counts against the embedding budget.
                let budget = cfg
                    .max
                    .saturating_sub(trail_prefix_len(&buf_trail))
                    .max(cfg.min);
                if text.len() > budget {
                    for piece in split_preserving_rows(&text, budget) {
                        push_chunk(&mut out, &piece, &buf_trail, buf_page);
                    }
                } else {
                    push_chunk(&mut out, &text, &buf_trail, buf_page);
                }
            }
            buf.clear();
        };
    }

    for block in blocks {
        match block {
            Block::PageBreak(n) => {
                // Close the current chunk at a page boundary. Without this a
                // chunk that straddles pages 7 and 8 is cited as page 7, and
                // the user opens the PDF to find the quote is not there —
                // which destroys trust in every other citation too.
                flush!();
                page = Some(*n);
            }

            Block::Heading { level, text } => {
                // A heading starts a new section, so close the current chunk.
                flush!();
                trail.retain(|(l, _)| *l < *level);
                trail.push((*level, text.clone()));
            }

            Block::Paragraph(p) | Block::Pre(p) | Block::TableRow(p) => {
                let p = p.trim();
                if p.is_empty() {
                    continue;
                }

                // Starting a fresh chunk: capture where it began.
                if buf.is_empty() {
                    buf_page = page;
                    buf_trail = trail.iter().map(|(_, t)| t.clone()).collect();
                }

                // Would adding this block overflow the target?
                if !buf.is_empty() && buf.len() + p.len() + 2 > cfg.target {
                    let tail = overlap_tail(&buf, cfg.overlap);
                    flush!();
                    buf_page = page;
                    buf_trail = trail.iter().map(|(_, t)| t.clone()).collect();
                    if !tail.is_empty() {
                        buf.push_str(&tail);
                        buf.push_str("\n\n");
                    }
                }

                // A single block larger than max must be split internally.
                if p.len() > cfg.max {
                    let pieces = if is_table_row(p) {
                        // A table row is a unit: cutting inside a cell
                        // produces a fragment that means nothing.
                        cut_row_at_cells(p, cfg.max)
                    } else {
                        split_long(p, cfg)
                    };
                    for piece in pieces {
                        if !buf.is_empty() && buf.len() + piece.len() + 2 > cfg.target {
                            flush!();
                            buf_page = page;
                            buf_trail = trail.iter().map(|(_, t)| t.clone()).collect();
                        }
                        if !buf.is_empty() {
                            buf.push_str("\n\n");
                        }
                        buf.push_str(&piece);
                    }
                } else {
                    if !buf.is_empty() {
                        buf.push_str("\n\n");
                    }
                    buf.push_str(p);
                }
            }
        }
    }
    flush!();

    // Merge runts into their neighbour so we never embed a 20-char fragment.
    merge_small(&mut out, cfg.min);

    // Both the heading trail and the runt-merge grow a chunk after it was
    // sized, so re-check every chunk against the budget and split whatever
    // no longer fits. `chunk.text` is what gets embedded, so the invariant
    // that matters is on `text`, not on `body`.
    let merged = std::mem::take(&mut out);
    let mut out: Vec<Chunk> = Vec::with_capacity(merged.len());
    for chunk in merged {
        if chunk.text.len() <= cfg.max {
            out.push(chunk);
            continue;
        }
        let budget = cfg
            .max
            .saturating_sub(trail_prefix_len(&chunk.headings))
            .max(cfg.min);
        for piece in split_preserving_rows(&chunk.body, budget) {
            push_chunk(&mut out, &piece, &chunk.headings, chunk.page);
        }
    }

    for (i, c) in out.iter_mut().enumerate() {
        c.index = i;
    }
    out
}

/// The number of characters the heading trail adds to a chunk's text.
///
/// `push_chunk` renders the trail as `headings.join(" › ")` followed by a
/// blank line, so the trail is part of the text that gets embedded and
/// therefore part of the budget.
fn trail_prefix_len(headings: &[String]) -> usize {
    if headings.is_empty() {
        0
    } else {
        headings.join(" › ").len() + 2
    }
}

fn push_chunk(out: &mut Vec<Chunk>, body: &str, headings: &[String], page: Option<u32>) {
    // Prefixing the heading trail is what lets a chunk retrieve on terms that
    // appear only in its section title.
    let text = if headings.is_empty() {
        body.to_string()
    } else {
        format!("{}\n\n{}", headings.join(" › "), body)
    };

    out.push(Chunk {
        text,
        body: body.to_string(),
        page,
        headings: headings.to_vec(),
        index: out.len(),
    });
}

/// True when the text contains something worth embedding, rather than being
/// page furniture like "— 12 —" or a row of dots.
fn has_content(s: &str) -> bool {
    s.chars().filter(|c| c.is_alphanumeric()).count() >= 8
}

/// True when a line is a Markdown table row.
///
/// A row is a unit. Its columns only mean anything together, and a chunk
/// that begins in the middle of a cell — "tion** (primary development
/// hardware: …" — retrieves for nothing, for a keyword engine or a
/// sentence encoder alike. Real documentation is full of tables, so the
/// chunker has to know about them.
fn is_table_row(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('|') && t.matches('|').count() >= 2
}

/// Split text into pieces of at most `budget` characters, keeping table
/// rows whole.
///
/// Tables are split between rows and never inside one. A row longer than
/// the whole budget is cut at a cell boundary, which is the only place a
/// row can be cut without destroying it.
fn split_preserving_rows(text: &str, budget: usize) -> Vec<String> {
    let budget = budget.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();

    for line in text.lines() {
        let row = is_table_row(line);

        if row && line.len() + 1 > budget {
            // The row alone exceeds the budget: cut it at cell boundaries.
            if !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.extend(cut_row_at_cells(line, budget));
            continue;
        }

        if !row && line.len() + 1 > budget {
            // A very long prose line: cut it on sentence boundaries.
            if !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let tight = ChunkConfig {
                target: budget,
                max: budget,
                ..ChunkConfig::default()
            };
            out.extend(split_long(line, &tight));
            continue;
        }

        if cur.len() + line.len() + 1 > budget && !cur.trim().is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }

    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Cut one oversized table row into pieces of at most `budget` characters,
/// at cell boundaries.
///
/// Each piece keeps the pipes that open it, so a reader can still see that
/// it is a fragment of a table rather than a sentence.
fn cut_row_at_cells(row: &str, budget: usize) -> Vec<String> {
    let budget = budget.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cell = String::new();

    for ch in row.chars() {
        cell.push(ch);
        if ch != '|' {
            continue;
        }
        if cur.len() + cell.len() > budget && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push_str(&cell);
        cell.clear();
    }

    if !cell.is_empty() {
        if cur.len() + cell.len() > budget && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push_str(&cell);
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Take the last whole sentence(s) up to `n` characters, for overlap.
fn overlap_tail(s: &str, n: usize) -> String {
    if n == 0 || s.is_empty() {
        return String::new();
    }
    let start = s.len().saturating_sub(n);
    // Align to a char boundary, then prefer to start at a sentence break.
    let mut start = start;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    // If the tail would begin in the middle of a table row, move it to the
    // start of the next line. A fragment that starts mid-cell carries a
    // piece of a sentence with no beginning, which is worse than no
    // overlap at all.
    let start = row_aligned_start(s, start);
    let tail = &s[start..];
    match tail.find(". ") {
        Some(i) if i + 2 < tail.len() => tail[i + 2..].trim().to_string(),
        _ => tail.trim().to_string(),
    }
}

/// Move a byte offset forward to the start of the next line when it falls
/// inside a table row.
fn row_aligned_start(s: &str, start: usize) -> usize {
    let line_start = s[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = s[start..].find('\n').map(|i| start + i).unwrap_or(s.len());
    let line = &s[line_start..line_end];
    if is_table_row(line) && start > line_start {
        return (line_end + 1).min(s.len());
    }
    start
}

/// Split an oversized block on sentence boundaries, falling back to a hard
/// character cut only when there is no punctuation at all.
fn split_long(s: &str, cfg: &ChunkConfig) -> Vec<String> {
    let sentences = split_sentences(s);
    let mut out = Vec::new();
    let mut cur = String::new();

    for sentence in sentences {
        if sentence.len() > cfg.max {
            // A single monstrous "sentence" — hard-cut it on char boundaries.
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let mut rest = sentence.as_str();
            while rest.len() > cfg.max {
                let mut cut = cfg.max;
                while cut > 0 && !rest.is_char_boundary(cut) {
                    cut -= 1;
                }
                // Prefer a space near the cut so we do not split a word.
                let window = &rest[..cut];
                let cut = window.rfind(' ').map(|i| i + 1).unwrap_or(cut);
                out.push(rest[..cut].trim().to_string());
                rest = &rest[cut..];
            }
            if !rest.trim().is_empty() {
                cur = rest.trim().to_string();
            }
            continue;
        }

        if !cur.is_empty() && cur.len() + sentence.len() + 1 > cfg.target {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&sentence);
    }

    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Sentence splitter that tolerates abbreviations and decimals.
///
/// Not linguistically perfect — deliberately. A heavyweight NLP dependency
/// is not worth it when the failure mode is a slightly odd chunk boundary.
pub fn split_sentences(s: &str) -> Vec<String> {
    const ABBREV: &[&str] = &[
        "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "etc", "e.g", "i.e", "vs", "fig", "no",
        "vol", "pp", "al", "inc", "ltd", "co", "approx", "dept", "est",
    ];

    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '.' || c == '!' || c == '?' {
            // Look ahead: a sentence end needs whitespace after it.
            let next = bytes.get(i + 1).map(|b| *b as char);
            let ends = match next {
                None => true,
                Some(n) if n.is_whitespace() => true,
                // Handle ".\"" and ".)"
                Some('"') | Some('\'') | Some(')') => bytes
                    .get(i + 2)
                    .map(|b| (*b as char).is_whitespace())
                    .unwrap_or(true),
                _ => false,
            };

            if ends && c == '.' {
                // Decimal number? "3.14" — digit before and after.
                let prev_digit = i > 0 && (bytes[i - 1] as char).is_ascii_digit();
                let next_digit = bytes
                    .get(i + 1)
                    .map(|b| (*b as char).is_ascii_digit())
                    .unwrap_or(false);
                if prev_digit && next_digit {
                    i += 1;
                    continue;
                }

                // Known abbreviation immediately before the dot?
                //
                // `rfind` returns a byte offset, and adding 1 to it assumes
                // the whitespace character is one byte wide. Unicode
                // whitespace is not: U+00A0 (no-break space) is two bytes,
                // U+2003 (em space) is three. `start + p + 1` then lands in
                // the middle of the character and the slice below panics on
                // a non-char boundary. Ingested documents are
                // attacker-supplied, so this was remotely triggerable.
                let word_start = s[start..i]
                    .char_indices()
                    .rev()
                    .find(|(_, ch)| ch.is_whitespace())
                    .map(|(p, ch)| start + p + ch.len_utf8())
                    .unwrap_or(start);
                let word =
                    s[word_start..i].trim_matches(|c: char| !c.is_alphanumeric() && c != '.');
                if ABBREV.contains(&word.to_ascii_lowercase().as_str()) {
                    i += 1;
                    continue;
                }

                // Single initial, as in "J. Smith".
                if word.len() == 1 && word.chars().all(|c| c.is_uppercase()) {
                    i += 1;
                    continue;
                }
            }

            if ends {
                let mut end = i + 1;
                while end < bytes.len() && matches!(bytes[end] as char, '"' | '\'' | ')') {
                    end += 1;
                }
                let piece = s[start..end].trim();
                if !piece.is_empty() {
                    out.push(piece.to_string());
                }
                start = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }

    let tail = s[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

/// Fold chunks below `min` into a neighbour, preferring the previous one.
/// Two chunks may only be merged when they share both a heading trail and a
/// page. Merging across a page boundary would produce a chunk whose cited
/// page is wrong for half its text.
fn mergeable(a: &Chunk, b: &Chunk) -> bool {
    a.headings == b.headings && a.page == b.page
}

fn merge_small(chunks: &mut Vec<Chunk>, min: usize) {
    let mut i = 0;
    while i < chunks.len() {
        if chunks[i].body.len() < min && chunks.len() > 1 {
            if i > 0 && mergeable(&chunks[i - 1], &chunks[i]) {
                let moved = chunks.remove(i);
                let prev = &mut chunks[i - 1];
                prev.body.push_str("\n\n");
                prev.body.push_str(&moved.body);
                prev.text.push_str("\n\n");
                prev.text.push_str(&moved.body);
                continue;
            } else if i + 1 < chunks.len() && mergeable(&chunks[i + 1], &chunks[i]) {
                let moved = chunks.remove(i);
                let next = &mut chunks[i];
                next.body = format!("{}\n\n{}", moved.body, next.body);
                next.text = format!("{}\n\n{}", moved.body, next.text);
                continue;
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(s: &str) -> Block {
        Block::Paragraph(s.to_string())
    }
    fn heading(level: u8, s: &str) -> Block {
        Block::Heading {
            level,
            text: s.to_string(),
        }
    }

    /* ---------- sentence splitting ---------- */

    #[test]
    fn splits_basic_sentences() {
        let s = split_sentences("One thing. Two things! Three? Yes.");
        assert_eq!(s.len(), 4);
        assert_eq!(s[0], "One thing.");
        assert_eq!(s[2], "Three?");
    }

    #[test]
    fn does_not_split_decimals() {
        let s = split_sentences("Pi is 3.14 exactly. Really.");
        assert_eq!(s.len(), 2, "3.14 must not split: {s:?}");
    }

    #[test]
    fn does_not_split_common_abbreviations() {
        let s = split_sentences("Dr. Smith met Mrs. Jones. They agreed.");
        assert_eq!(s.len(), 2, "abbreviations must not split: {s:?}");
    }

    #[test]
    fn does_not_split_initials() {
        let s = split_sentences("Written by J. R. Smith. It was long.");
        assert_eq!(s.len(), 2, "initials must not split: {s:?}");
    }

    #[test]
    fn unicode_whitespace_before_a_period_does_not_panic() {
        // U+00A0 is two bytes but matches `char::is_whitespace`. The
        // abbreviation scan used to add 1 to the byte offset of the
        // whitespace, landing inside the character and panicking on a
        // non-char-boundary slice. Documents are user-supplied, so the
        // input is attacker-controlled.
        let s = split_sentences("Hello\u{00A0}. Next sentence here.");
        assert_eq!(s.len(), 2, "expected two sentences, got {s:?}");
        assert!(s[0].contains('\u{00A0}'));

        // Three-byte whitespace too.
        let s = split_sentences("Wide\u{2003}gap. Another one.");
        assert_eq!(s.len(), 2, "expected two sentences, got {s:?}");
    }

    #[test]
    fn keeps_trailing_quote_with_its_sentence() {
        let s = split_sentences("He said \"stop.\" Then he left.");
        assert_eq!(s.len(), 2);
        assert!(s[0].ends_with('"'));
    }

    #[test]
    fn handles_text_with_no_terminator() {
        let s = split_sentences("no punctuation here");
        assert_eq!(s.len(), 1);
    }

    /* ---------- chunking ---------- */

    #[test]
    fn empty_input_yields_no_chunks() {
        assert!(chunk_blocks(&[], &ChunkConfig::default()).is_empty());
    }

    #[test]
    fn every_chunk_fits_the_embedding_window() {
        // `chunk.text` is what gets embedded. The embedding model refuses
        // anything that does not fit its token window, and one refused
        // chunk used to cost the whole document its semantic index.
        let mut blocks = vec![heading(1, "Termination and notice")];
        for i in 0..40 {
            blocks.push(para(&format!(
                "Sentence number {i} about termination notice periods and how they are counted, \
                 repeated so that the paragraph is comfortably long. "
            )));
        }
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert!(c.len() > 1, "the input should produce several chunks");
        for chunk in &c {
            assert!(
                chunk.text.len() <= crate::rag::embed::MAX_EMBED_CHARS,
                "chunk {} is {} characters, over the {} budget",
                chunk.index,
                chunk.text.len(),
                crate::rag::embed::MAX_EMBED_CHARS
            );
        }
    }

    #[test]
    fn a_long_heading_trail_does_not_push_a_chunk_over_budget() {
        // The trail is embedded with the body, so it eats into the budget.
        let long_title = "A section title that is long enough to eat a large part of the \
                          embedding budget on its own, with more words after it";
        let blocks = vec![
            heading(1, long_title),
            para("A paragraph of body text that would fit comfortably on its own."),
        ];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 1);
        assert!(c[0].text.len() <= crate::rag::embed::MAX_EMBED_CHARS);
        assert!(
            c[0].text.starts_with(long_title),
            "the heading trail must still lead the chunk"
        );
    }

    #[test]
    fn a_table_row_is_never_cut_in_half() {
        // A row cut mid-cell is not a retrievable unit. The row below is
        // longer than the budget, so it has to be split somewhere: at a
        // cell boundary, never inside a cell.
        let row = format!(
            "| {} |",
            (0..120)
                .map(|i| format!("cell-{i:03}"))
                .collect::<Vec<_>>()
                .join(" | ")
        );
        let blocks = vec![Block::TableRow(row.clone())];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert!(c.len() > 1, "a row longer than the budget must be split");
        for chunk in &c {
            for line in chunk.body.lines() {
                if is_table_row(line) {
                    assert!(
                        line.trim_end().ends_with('|'),
                        "a table row was cut inside a cell: {line:?}"
                    );
                }
            }
        }
        // Nothing was lost.
        let rejoined: String = c.iter().map(|x| x.body.clone()).collect();
        for i in 0..120 {
            assert!(rejoined.contains(&format!("cell-{i:03}")), "cell {i} was lost");
        }
    }

    #[test]
    fn a_table_that_fits_stays_in_one_chunk() {
        // The tier table from the master plan: eight short rows.
        let rows: Vec<String> = (0..8)
            .map(|i| format!("| Tier {i} | Qwen3 4B Q4_K_M | 3.0 GiB | 45 tok/s |"))
            .collect();
        let blocks: Vec<Block> = rows
            .iter()
            .map(|r| Block::TableRow(r.clone()))
            .collect();
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 1, "a small table must not be scattered");
        for i in 0..8 {
            assert!(c[0].body.contains(&format!("Tier {i}")));
        }
    }

    #[test]
    fn the_overlap_does_not_start_in_the_middle_of_a_row() {
        let rows: Vec<String> = (0..6)
            .map(|i| format!("| Row {i} | some reasonably long cell value here |"))
            .collect();
        let blocks: Vec<Block> = rows
            .iter()
            .map(|r| Block::TableRow(r.clone()))
            .collect();
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        for chunk in &c {
            for line in chunk.body.lines() {
                if is_table_row(line) {
                    assert!(
                        line.trim_start().starts_with('|'),
                        "a chunk begins in the middle of a row: {line:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_long_paragraph_is_split_not_dropped() {
        // Content must survive: splitting is the fix, truncation is not.
        let body = "This is a sentence that repeats itself for a long time. ".repeat(60);
        let blocks = vec![para(&body)];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert!(c.len() > 1);
        let rejoined: String = c.iter().map(|x| x.body.clone()).collect();
        for sentence in body.split(". ").take(5) {
            assert!(
                rejoined.contains(sentence),
                "content was lost while splitting"
            );
        }
    }

    #[test]
    fn short_document_is_one_chunk() {
        let blocks = vec![para(
            "This is a short document about contract termination rules.",
        )];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn heading_trail_is_carried_into_chunks() {
        let blocks = vec![
            heading(1, "3. Terms"),
            heading(2, "3.2 Termination"),
            para("Either party may end this agreement with thirty days notice in writing."),
        ];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].headings, vec!["3. Terms", "3.2 Termination"]);
        assert!(
            c[0].text.contains("3.2 Termination"),
            "heading must be embedded with the body so it is searchable"
        );
        assert!(
            !c[0].body.contains("3.2 Termination"),
            "display body should not repeat the heading"
        );
        assert_eq!(c[0].breadcrumb(), "3. Terms › 3.2 Termination");
    }

    #[test]
    fn sibling_heading_pops_the_deeper_level() {
        let blocks = vec![
            heading(1, "A"),
            heading(2, "A.1"),
            para("First section body text that is long enough to survive the minimum."),
            heading(2, "A.2"),
            para("Second section body text that is long enough to survive as well."),
        ];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].headings, vec!["A", "A.1"]);
        assert_eq!(
            c[1].headings,
            vec!["A", "A.2"],
            "A.1 must not leak into A.2"
        );
    }

    #[test]
    fn a_heading_starts_a_new_chunk() {
        let cfg = ChunkConfig::default();
        let blocks = vec![
            para("Body of the first section, reasonably long so it is kept."),
            heading(1, "New Section"),
            para("Body of the second section, also reasonably long so it is kept."),
        ];
        let c = chunk_blocks(&blocks, &cfg);
        assert_eq!(
            c.len(),
            2,
            "a heading must not be glued to the previous text"
        );
    }

    #[test]
    fn page_numbers_are_tracked() {
        let blocks = vec![
            Block::PageBreak(1),
            para("Content that belongs to the first page of this document."),
            Block::PageBreak(2),
            para("Content that belongs to the second page of this document."),
        ];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].page, Some(1));
        assert_eq!(c[1].page, Some(2));
    }

    #[test]
    fn short_chunks_are_not_merged_across_a_page_boundary() {
        // Merging these would cite page 1 for text that is on page 2.
        let blocks = vec![
            Block::PageBreak(1),
            para("Short line one."),
            Block::PageBreak(2),
            para("Short line two."),
        ];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 2, "runts merged across pages: {c:?}");
        assert_eq!(c[0].page, Some(1));
        assert_eq!(c[1].page, Some(2));
    }

    #[test]
    fn short_chunks_on_the_same_page_still_merge() {
        let blocks = vec![Block::PageBreak(1), para("Short one."), para("Short two.")];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 1, "runt merging should still work within a page");
    }

    #[test]
    fn long_text_splits_into_several_chunks() {
        let cfg = ChunkConfig::default();
        let sentence = "This sentence exists to make the document long enough to split. ";
        let blocks: Vec<Block> = (0..60).map(|_| para(sentence.trim())).collect();
        let c = chunk_blocks(&blocks, &cfg);
        assert!(c.len() > 2, "expected several chunks, got {}", c.len());
        for chunk in &c {
            assert!(
                chunk.body.len() <= cfg.max,
                "chunk of {} exceeds max {}",
                chunk.body.len(),
                cfg.max
            );
        }
    }

    #[test]
    fn never_cuts_a_word_in_half() {
        let cfg = ChunkConfig::default();
        let blocks: Vec<Block> = (0..40)
            .map(|i| {
                para(&format!(
                    "Sentence number {i} about indivisible terminology."
                ))
            })
            .collect();
        let c = chunk_blocks(&blocks, &cfg);
        for chunk in &c {
            // A broken word would leave a fragment with no trailing space
            // before the boundary; check we end on whitespace or punctuation.
            let last = chunk.body.trim_end().chars().last().unwrap();
            assert!(
                last.is_alphanumeric() || ".!?\"')".contains(last),
                "chunk ends mid-token: {:?}",
                &chunk.body[chunk.body.len().saturating_sub(40)..]
            );
        }
    }

    #[test]
    fn pathological_single_line_is_hard_split() {
        let cfg = ChunkConfig::default();
        // 20k characters, no punctuation at all.
        let blob = "word ".repeat(4000);
        let c = chunk_blocks(&[para(&blob)], &cfg);
        assert!(c.len() > 5);
        for chunk in &c {
            assert!(chunk.body.len() <= cfg.max);
        }
    }

    #[test]
    fn drops_page_furniture() {
        let blocks = vec![
            para("— 12 —"),
            para("....."),
            para("Real content lives here and is long enough to keep."),
        ];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert_eq!(c.len(), 1, "page numbers and dot leaders must be dropped");
        assert!(c[0].body.contains("Real content"));
    }

    #[test]
    fn tiny_trailing_chunk_is_merged() {
        let cfg = ChunkConfig {
            target: 200,
            max: 300,
            min: 100,
            overlap: 0,
        };
        let blocks = vec![
            para(&"Long enough first paragraph. ".repeat(8)),
            para("Tiny."),
        ];
        let c = chunk_blocks(&blocks, &cfg);
        for chunk in &c {
            assert!(
                chunk.body.len() >= cfg.min || c.len() == 1,
                "runt chunk survived: {:?}",
                chunk.body
            );
        }
    }

    #[test]
    fn table_rows_are_kept_whole() {
        let row = "| Item | Qty | Price | Total | Notes about the line item |";
        let blocks = vec![
            heading(1, "Invoice"),
            Block::TableRow(row.into()),
            Block::TableRow(row.into()),
        ];
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        assert!(c[0].body.contains(row), "table row must not be split");
    }

    #[test]
    fn chunks_are_indexed_in_order() {
        let blocks: Vec<Block> = (0..30)
            .map(|i| {
                para(&format!(
                    "Paragraph {i} with enough text to be meaningful here."
                ))
            })
            .collect();
        let c = chunk_blocks(&blocks, &ChunkConfig::default());
        for (i, chunk) in c.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
    }

    #[test]
    fn overlap_repeats_context_between_chunks() {
        let cfg = ChunkConfig {
            target: 300,
            max: 500,
            min: 50,
            overlap: 120,
        };
        let blocks: Vec<Block> = (0..12)
            .map(|i| {
                para(&format!(
                    "Distinct paragraph {i} carrying its own meaning here."
                ))
            })
            .collect();
        let c = chunk_blocks(&blocks, &cfg);
        assert!(c.len() >= 2);
        // At least one later chunk should begin with text seen earlier.
        let overlapped = c.windows(2).any(|w| {
            let prev_tail: String = w[0]
                .body
                .chars()
                .rev()
                .take(60)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            w[1].body.contains(prev_tail.trim())
                || prev_tail
                    .split_whitespace()
                    .last()
                    .map(|word| w[1].body.contains(word))
                    .unwrap_or(false)
        });
        assert!(
            overlapped,
            "expected overlap context between adjacent chunks"
        );
    }

    #[test]
    fn unicode_is_not_corrupted() {
        let cfg = ChunkConfig {
            target: 100,
            max: 160,
            min: 20,
            overlap: 20,
        };
        let text = "नमस्ते दुनिया। यह एक परीक्षण है। ".repeat(20);
        let c = chunk_blocks(&[para(&text)], &cfg);
        for chunk in &c {
            // Round-tripping through String proves no broken code points.
            assert_eq!(
                chunk.body,
                String::from_utf8(chunk.body.clone().into_bytes()).unwrap()
            );
            assert!(!chunk.body.contains('\u{FFFD}'), "replacement char found");
        }
    }

    #[test]
    fn emoji_at_boundary_survives() {
        let cfg = ChunkConfig {
            target: 50,
            max: 80,
            min: 10,
            overlap: 10,
        };
        let text = "Status 🚀 ok. ".repeat(30);
        let c = chunk_blocks(&[para(&text)], &cfg);
        let joined: String = c.iter().map(|x| x.body.as_str()).collect();
        assert!(joined.contains('🚀'));
        assert!(!joined.contains('\u{FFFD}'));
    }
}
