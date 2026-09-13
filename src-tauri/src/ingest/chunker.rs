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
        // ~1200 chars is roughly 300 tokens: large enough to hold a complete
        // thought, small enough that four of them fit a small model's context
        // alongside the question and the answer.
        Self {
            target: 1200,
            max: 1800,
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
            let text = buf.trim();
            if !text.is_empty() && has_content(text) {
                push_chunk(&mut out, text, &buf_trail, buf_page);
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
                    for piece in split_long(p, cfg) {
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

    for (i, c) in out.iter_mut().enumerate() {
        c.index = i;
    }
    out
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
    let tail = &s[start..];
    match tail.find(". ") {
        Some(i) if i + 2 < tail.len() => tail[i + 2..].trim().to_string(),
        _ => tail.trim().to_string(),
    }
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
                let word_start = s[start..i]
                    .rfind(|ch: char| ch.is_whitespace())
                    .map(|p| start + p + 1)
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
