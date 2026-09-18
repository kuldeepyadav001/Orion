//! Retrieval evaluation harness — the M2 exit gate.
//!
//! Without measurement, "the RAG feels better" is an opinion. Serina shipped
//! on that opinion and the retrieval quality gap was only found later by
//! reading complaints. This module makes retrieval quality a number that CI
//! can fail on.
//!
//! ## What is measured
//!
//! * **Recall@k** — was any correct chunk retrieved at all? If this is low,
//!   no amount of prompting fixes the answer.
//! * **MRR** — how high did the first correct chunk rank? Position matters
//!   because a small model attends most strongly to the first source.
//! * **Precision@k** — how much of the context was wasted on irrelevant text?
//!   Wasted context is wasted KV cache, which is scarce at Tier T1.
//!
//! ## The 30 questions
//!
//! Grouped by the failure mode each is designed to catch, not by topic. A
//! suite that only asks easy paraphrase questions will report excellent
//! numbers while the product fails on the questions users actually ask.
//!
//! Thresholds below are the gate. They are set from what hybrid retrieval
//! should comfortably achieve on a corpus this size; if a change drops them,
//! the change is wrong, not the threshold.

use crate::rag::search::{reciprocal_rank_fusion, vector_rank, Bm25, Ranked};

/// What kind of retrieval failure a question probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Wording differs from the document: needs semantic matching.
    Paraphrase,
    /// Exact identifier, code or number: needs keyword matching.
    Exact,
    /// Answer sits under a heading whose words are absent from the body.
    HeadingScoped,
    /// Two documents discuss the same topic; the right one must win.
    Disambiguation,
    /// The corpus does not contain the answer; retrieval should find nothing
    /// convincing and the system should refuse.
    Unanswerable,
}

#[derive(Debug, Clone)]
pub struct EvalQuestion {
    pub id: u32,
    pub question: &'static str,
    /// Chunk ids that genuinely answer the question. Empty = unanswerable.
    pub relevant: &'static [i64],
    pub probe: Probe,
}

#[derive(Debug, Clone, Default)]
pub struct EvalReport {
    pub total: usize,
    /// Answerable questions — those included in the averages below.
    pub scored: usize,
    /// Unanswerable questions, excluded from the averages. See `run_eval`.
    pub unanswerable: usize,
    pub recall_at_k: f32,
    pub mrr: f32,
    pub precision_at_k: f32,
    pub failures: Vec<(u32, &'static str, Probe)>,
    pub by_probe: Vec<(Probe, f32)>,
}

impl EvalReport {
    /// Render for CI logs and the M2 evidence document.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("questions:      {}\n", self.total));
        s.push_str(&format!("  scored:       {}\n", self.scored));
        s.push_str(&format!(
            "  unanswerable: {} (excluded; refusal is scored at generation)\n",
            self.unanswerable
        ));
        s.push_str(&format!("recall@k:       {:.3}\n", self.recall_at_k));
        s.push_str(&format!("MRR:            {:.3}\n", self.mrr));
        s.push_str(&format!("precision@k:    {:.3}\n", self.precision_at_k));
        s.push_str("\nby probe:\n");
        for (p, v) in &self.by_probe {
            s.push_str(&format!("  {p:<16?} {v:.3}\n"));
        }
        if !self.failures.is_empty() {
            s.push_str("\nfailures:\n");
            for (id, q, p) in &self.failures {
                s.push_str(&format!("  Q{id:<3} [{p:?}] {q}\n"));
            }
        }
        s
    }
}

/* ------------------------------------------------------------------ */
/* The fixture corpus                                                  */
/* ------------------------------------------------------------------ */

/// A miniature but adversarial corpus: two overlapping HR documents, a
/// contract, an invoice and a technical note. The overlap is deliberate —
/// retrieval is easy when every document is about something different.
///
/// Format: (chunk_id, heading_trail, body). The indexed text is the heading
/// plus the body, exactly as `RagStore` stores it.
pub const CORPUS: &[(i64, &str, &str)] = &[
    // --- handbook.md (current) ---
    (1, "Leave › Annual Leave",
     "Full-time staff accrue 2.08 days per completed month of service, to a maximum of 25 days \
      in any calendar year. Unused days beyond five are forfeited on 31 December."),
    (2, "Leave › Sick Leave",
     "Staff may take up to ten days per year without a medical certificate. Beyond ten \
      consecutive days a certificate from a registered practitioner is required."),
    (3, "Leave › Parental Leave",
     "Primary caregivers receive eighteen weeks at full pay. Secondary caregivers receive four \
      weeks. Notice of at least ten weeks before the expected date is expected."),
    (4, "Expenses › Travel",
     "Economy class is the standard for journeys under six hours. Anything longer may be booked \
      in premium economy with written approval from a director."),
    (5, "Expenses › Meals",
     "The daily limit is 45 per person domestically and 75 internationally. Alcohol is not \
      reimbursed. Receipts must be submitted within thirty days."),
    (6, "Remote Work › Eligibility",
     "Staff who have completed probation may work away from the office up to three days each \
      week, subject to their manager's agreement and the needs of their team."),
    (7, "Remote Work › Equipment",
     "The company provides a laptop and one external display. Chairs, desks and broadband are \
      the responsibility of the individual."),
    (8, "Conduct › Confidentiality",
     "Information about clients, pricing and unreleased products must not be disclosed outside \
      the company, during employment or after it ends."),

    // --- handbook-2019.md (superseded, deliberately conflicting) ---
    (9, "Leave › Annual Leave",
     "Staff accrue 1.67 days per month to a maximum of 20 days per year. This document was \
      superseded in 2023 and is retained for historical reference only."),
    (10, "Remote Work › Eligibility",
     "Working from home is permitted one day per week at the manager's discretion. Superseded \
      in 2023."),

    // --- services-agreement.md ---
    (11, "3. Term › 3.2 Termination",
     "Either party may bring this arrangement to an end by giving the other ninety days written \
      notice, delivered to the address in Schedule 1."),
    (12, "3. Term › 3.3 Termination for Cause",
     "Where a material breach is not remedied within fourteen days of written notice, the \
      non-breaching party may end the arrangement immediately."),
    (13, "4. Fees › 4.1 Charges",
     "The monthly retainer is 4,500 exclusive of tax. Work beyond the retained forty hours is \
      billed at 145 per hour."),
    (14, "4. Fees › 4.2 Payment",
     "Invoices fall due thirty days from the date of issue. Late amounts carry interest at 1.5 \
      per cent per month."),
    (15, "5. Liability",
     "Neither party is liable for indirect or consequential loss. Total liability is capped at \
      the fees paid in the preceding twelve months."),
    (16, "6. Intellectual Property",
     "All deliverables become the property of the client on payment in full. The supplier \
      retains ownership of pre-existing tools and libraries."),

    // --- invoice-2026-0412.md ---
    (17, "Invoice Details",
     "Invoice INV-2026-0412 issued 14 March 2026. Reference PO-88231. Amount due 5,340 including \
      tax. Remit to account 20-44-19 / 60817244."),
    (18, "Line Items",
     "Retainer for March: 4,500. Additional hours (6 at 145): 870. Subtotal 5,370 less credit \
      note CN-0031 of 30."),

    // --- runbook.md ---
    (19, "Deployment › Rollback",
     "Run the previous release tag through the same pipeline. Do not revert the database; \
      migrations are forward-only and a reverted schema will corrupt in-flight writes."),
    (20, "Deployment › Health Checks",
     "The readiness probe hits /healthz and expects a 200 within two seconds. Three consecutive \
      failures remove the instance from the pool."),
    (21, "Incidents › Severity",
     "Sev-1 means customer data is at risk or the product is entirely unavailable. Sev-2 means a \
      major feature is broken with no workaround."),
    (22, "Incidents › On Call",
     "The primary responder acknowledges within five minutes. If unacknowledged after ten \
      minutes the page escalates to the secondary."),
];

/// Chunk ids whose *body* does not contain the heading's key word. These are
/// the questions that only work if the heading trail is indexed.
pub const HEADING_DEPENDENT: &[i64] = &[11, 12, 20, 22];

/// The 30-question suite.
pub const QUESTIONS: &[EvalQuestion] = &[
    // --- Paraphrase: wording differs from the document (8) ---
    EvalQuestion {
        id: 1,
        question: "how much holiday do I get each year",
        relevant: &[1],
        probe: Probe::Paraphrase,
    },
    EvalQuestion {
        id: 2,
        question: "do I need a doctor's note if I am off ill",
        relevant: &[2],
        probe: Probe::Paraphrase,
    },
    EvalQuestion {
        id: 3,
        question: "what happens if someone is having a baby",
        relevant: &[3],
        probe: Probe::Paraphrase,
    },
    EvalQuestion {
        id: 4,
        question: "can I expense a glass of wine at dinner",
        relevant: &[5],
        probe: Probe::Paraphrase,
    },
    EvalQuestion {
        id: 5,
        question: "how many days can I work from home",
        relevant: &[6],
        probe: Probe::Paraphrase,
    },
    EvalQuestion {
        id: 6,
        question: "will the company buy me a desk chair",
        relevant: &[7],
        probe: Probe::Paraphrase,
    },
    EvalQuestion {
        id: 7,
        question: "who owns the code once the project is finished",
        relevant: &[16],
        probe: Probe::Paraphrase,
    },
    EvalQuestion {
        id: 8,
        question: "what counts as the most serious kind of outage",
        relevant: &[21],
        probe: Probe::Paraphrase,
    },
    // --- Exact: identifiers and numbers that embeddings blur (7) ---
    EvalQuestion {
        id: 9,
        question: "what is invoice INV-2026-0412 for",
        relevant: &[17, 18],
        probe: Probe::Exact,
    },
    EvalQuestion {
        id: 10,
        question: "find purchase order PO-88231",
        relevant: &[17],
        probe: Probe::Exact,
    },
    EvalQuestion {
        id: 11,
        question: "what is credit note CN-0031",
        relevant: &[18],
        probe: Probe::Exact,
    },
    EvalQuestion {
        id: 12,
        question: "what is the bank account 60817244",
        relevant: &[17],
        probe: Probe::Exact,
    },
    EvalQuestion {
        id: 13,
        question: "which endpoint does the readiness probe use",
        relevant: &[20],
        probe: Probe::Exact,
    },
    EvalQuestion {
        id: 14,
        question: "what is the hourly rate of 145 for",
        relevant: &[13],
        probe: Probe::Exact,
    },
    EvalQuestion {
        id: 15,
        question: "what does Schedule 1 contain",
        relevant: &[11],
        probe: Probe::Exact,
    },
    // --- Heading-scoped: the body never says the query word (6) ---
    EvalQuestion {
        id: 16,
        question: "termination",
        relevant: &[11, 12],
        probe: Probe::HeadingScoped,
    },
    EvalQuestion {
        id: 17,
        question: "how do I terminate the services agreement",
        relevant: &[11],
        probe: Probe::HeadingScoped,
    },
    EvalQuestion {
        id: 18,
        question: "termination for cause",
        relevant: &[12],
        probe: Probe::HeadingScoped,
    },
    EvalQuestion {
        id: 19,
        question: "health checks",
        relevant: &[20],
        probe: Probe::HeadingScoped,
    },
    EvalQuestion {
        id: 20,
        question: "on call escalation",
        relevant: &[22],
        probe: Probe::HeadingScoped,
    },
    EvalQuestion {
        id: 21,
        question: "liability cap",
        relevant: &[15],
        probe: Probe::HeadingScoped,
    },
    // --- Disambiguation: near-duplicate content, one right answer (5) ---
    EvalQuestion {
        id: 22,
        question: "current annual leave entitlement maximum days",
        relevant: &[1],
        probe: Probe::Disambiguation,
    },
    EvalQuestion {
        id: 23,
        question: "what was the old leave policy before 2023",
        relevant: &[9],
        probe: Probe::Disambiguation,
    },
    EvalQuestion {
        id: 24,
        question: "current remote working allowance per week",
        relevant: &[6],
        probe: Probe::Disambiguation,
    },
    EvalQuestion {
        id: 25,
        question: "notice period to end the contract normally",
        relevant: &[11],
        probe: Probe::Disambiguation,
    },
    EvalQuestion {
        id: 26,
        question: "when are invoices due for payment",
        relevant: &[14],
        probe: Probe::Disambiguation,
    },
    // --- Unanswerable: must not confabulate (4) ---
    EvalQuestion {
        id: 27,
        question: "what is the company pension contribution rate",
        relevant: &[],
        probe: Probe::Unanswerable,
    },
    EvalQuestion {
        id: 28,
        question: "who is the chief executive",
        relevant: &[],
        probe: Probe::Unanswerable,
    },
    EvalQuestion {
        id: 29,
        question: "what is the wifi password in the Berlin office",
        relevant: &[],
        probe: Probe::Unanswerable,
    },
    EvalQuestion {
        id: 30,
        question: "how many parking spaces does the building have",
        relevant: &[],
        probe: Probe::Unanswerable,
    },
];

/* ------------------------------------------------------------------ */
/* Scoring                                                             */
/* ------------------------------------------------------------------ */

/// Run the suite against a retrieval function.
///
/// `retrieve` takes the question and k, returns ranked chunk ids. This is a
/// closure so the same suite scores the in-memory harness here, the real
/// `RagStore`, and any future reranker without being rewritten.
pub fn run_eval<F>(k: usize, mut retrieve: F) -> EvalReport
where
    F: FnMut(&str, usize) -> Vec<i64>,
{
    let mut recall_sum = 0.0;
    let mut rr_sum = 0.0;
    let mut precision_sum = 0.0;
    let mut failures = Vec::new();
    let mut unanswerable = 0usize;

    let mut probe_scores: Vec<(Probe, f32, usize)> = Vec::new();

    for q in QUESTIONS {
        let got = retrieve(q.question, k);

        if q.relevant.is_empty() {
            // Unanswerable questions are deliberately EXCLUDED from recall,
            // MRR and precision.
            //
            // The first version of this harness scored them as "retrieved
            // nothing = 1.0", and every retriever scored 0.000 on the entire
            // category. That was the metric being wrong, not the retriever: a
            // similarity search over a small corpus always returns some
            // nearest neighbour, because "nearest" is not "relevant", and no
            // score threshold separates the two reliably across queries.
            //
            // Refusing to answer is the generator's job, enforced by
            // GROUNDED_SYSTEM_PROMPT and detectable via `is_uncited_claim`.
            // These four questions are scored in the runtime eval against a
            // live model. Leaving them in these averages would have dragged
            // every number down by a flat 13% and made the gate meaningless.
            unanswerable += 1;
            continue;
        }

        let hit_positions: Vec<usize> = got
            .iter()
            .enumerate()
            .filter(|(_, id)| q.relevant.contains(id))
            .map(|(i, _)| i)
            .collect();

        let recall = if hit_positions.is_empty() { 0.0 } else { 1.0 };
        let rr = hit_positions
            .first()
            .map(|p| 1.0 / (*p as f32 + 1.0))
            .unwrap_or(0.0);
        let precision = if got.is_empty() {
            0.0
        } else {
            hit_positions.len() as f32 / got.len().min(k) as f32
        };

        if recall < 1.0 {
            failures.push((q.id, q.question, q.probe));
        }

        recall_sum += recall;
        rr_sum += rr;
        precision_sum += precision;

        match probe_scores.iter_mut().find(|(p, _, _)| *p == q.probe) {
            Some(e) => {
                e.1 += recall;
                e.2 += 1;
            }
            None => probe_scores.push((q.probe, recall, 1)),
        }
    }

    let scored = QUESTIONS.len() - unanswerable;
    let n = (scored as f32).max(1.0);

    EvalReport {
        total: QUESTIONS.len(),
        scored,
        unanswerable,
        recall_at_k: recall_sum / n,
        mrr: rr_sum / n,
        precision_at_k: precision_sum / n,
        failures,
        by_probe: probe_scores
            .into_iter()
            .map(|(p, sum, count)| (p, sum / count as f32))
            .collect(),
    }
}

/* ------------------------------------------------------------------ */
/* In-memory harness                                                   */
/* ------------------------------------------------------------------ */

/// Indexed text for a chunk: heading trail prefixed to the body, matching
/// what `RagStore` writes to FTS.
pub fn indexed_text(heading: &str, body: &str) -> String {
    if heading.is_empty() {
        body.to_string()
    } else {
        format!("{heading}\n{body}")
    }
}

/// A deterministic stand-in for a real embedding model.
///
/// It is a hashed bag-of-words projection: documents sharing vocabulary get
/// similar vectors. It is far weaker than a real model — it cannot match
/// "holiday" to "annual leave" — so it is used only to verify that the fusion
/// *plumbing* behaves, never to claim a quality number. Real numbers come
/// from the runtime eval against a live embedding server.
pub fn pseudo_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; dim];
    for token in crate::rag::search::tokenize(text) {
        let mut h: u64 = 1469598103934665603;
        for b in token.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        let idx = (h % dim as u64) as usize;
        v[idx] += 1.0;
        // A second slot reduces collisions between unrelated tokens.
        v[((h >> 32) % dim as u64) as usize] += 0.5;
    }
    crate::rag::embed::normalize(v)
}

/// Build a hybrid retriever over `CORPUS` for tests.
pub struct Harness {
    bm25: Bm25,
    vectors: Vec<(i64, Vec<f32>)>,
    /// When false, the heading trail is stripped from the indexed text — used
    /// to prove the heading trail is actually earning its place.
    pub use_headings: bool,
}

impl Harness {
    pub fn new(use_headings: bool) -> Self {
        let docs: Vec<(i64, String)> = CORPUS
            .iter()
            .map(|(id, h, b)| {
                let text = if use_headings {
                    indexed_text(h, b)
                } else {
                    b.to_string()
                };
                (*id, text)
            })
            .collect();

        let vectors = docs
            .iter()
            .map(|(id, t)| (*id, pseudo_embed(t, 256)))
            .collect();

        Self {
            bm25: Bm25::new(docs),
            vectors,
            use_headings,
        }
    }

    pub fn bm25_only(&self, q: &str, k: usize) -> Vec<i64> {
        self.bm25
            .search(q, k)
            .into_iter()
            .map(|r| r.chunk_id)
            .collect()
    }

    pub fn vector_only(&self, q: &str, k: usize) -> Vec<i64> {
        let qv = pseudo_embed(q, 256);
        vector_rank(&qv, &self.vectors, k)
            .into_iter()
            .map(|r| r.chunk_id)
            .collect()
    }

    pub fn hybrid(&self, q: &str, k: usize) -> Vec<i64> {
        let bm: Vec<Ranked> = self.bm25.search(q, 40);
        let qv = pseudo_embed(q, 256);
        let vc = vector_rank(&qv, &self.vectors, 40);

        reciprocal_rank_fusion(
            &[
                ("bm25", bm, crate::rag::store::WEIGHT_BM25),
                ("vector", vc, crate::rag::store::WEIGHT_VECTOR),
            ],
            k,
        )
        .into_iter()
        .map(|(id, _, _)| id)
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------- suite integrity ---------- */

    #[test]
    fn the_suite_has_thirty_questions() {
        assert_eq!(QUESTIONS.len(), 30, "M2 exit gate specifies 30 questions");
    }

    #[test]
    fn question_ids_are_unique_and_sequential() {
        for (i, q) in QUESTIONS.iter().enumerate() {
            assert_eq!(q.id as usize, i + 1, "question ids must be 1..=30");
        }
    }

    #[test]
    fn every_probe_category_is_represented() {
        for probe in [
            Probe::Paraphrase,
            Probe::Exact,
            Probe::HeadingScoped,
            Probe::Disambiguation,
            Probe::Unanswerable,
        ] {
            let n = QUESTIONS.iter().filter(|q| q.probe == probe).count();
            assert!(
                n >= 4,
                "{probe:?} has only {n} questions; too few to mean anything"
            );
        }
    }

    #[test]
    fn every_relevant_chunk_id_exists_in_the_corpus() {
        for q in QUESTIONS {
            for id in q.relevant {
                assert!(
                    CORPUS.iter().any(|(cid, _, _)| cid == id),
                    "Q{} references chunk {id}, which is not in the corpus",
                    q.id
                );
            }
        }
    }

    #[test]
    fn unanswerable_questions_have_no_relevant_chunks() {
        for q in QUESTIONS.iter().filter(|q| q.probe == Probe::Unanswerable) {
            assert!(q.relevant.is_empty(), "Q{} is contradictory", q.id);
        }
    }

    #[test]
    fn answerable_questions_have_at_least_one_relevant_chunk() {
        for q in QUESTIONS.iter().filter(|q| q.probe != Probe::Unanswerable) {
            assert!(!q.relevant.is_empty(), "Q{} has no answer key", q.id);
        }
    }

    #[test]
    fn corpus_chunk_ids_are_unique() {
        let mut ids: Vec<i64> = CORPUS.iter().map(|(id, _, _)| *id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate chunk id in the corpus");
    }

    #[test]
    fn heading_dependent_chunks_really_are_heading_dependent() {
        // If the body already contains the heading's key word, the question
        // is not actually testing heading indexing and the suite is lying.
        for id in HEADING_DEPENDENT {
            let (_, heading, body) = CORPUS.iter().find(|(c, _, _)| c == id).unwrap();
            let key = heading
                .split('›')
                .next_back()
                .unwrap()
                .split_whitespace()
                .next_back()
                .unwrap()
                .to_lowercase();
            let stem: String = key.chars().take(6).collect();
            assert!(
                !body.to_lowercase().contains(&stem),
                "chunk {id}: body already contains {stem:?}, so it does not test headings"
            );
        }
    }

    /* ---------- scoring correctness ---------- */

    #[test]
    fn a_perfect_retriever_scores_perfectly() {
        let report = run_eval(4, |q, _k| {
            QUESTIONS
                .iter()
                .find(|x| x.question == q)
                .map(|x| x.relevant.to_vec())
                .unwrap_or_default()
        });
        assert!(
            (report.recall_at_k - 1.0).abs() < 1e-6,
            "{}",
            report.render()
        );
        assert!((report.mrr - 1.0).abs() < 1e-6);
        assert!(report.failures.is_empty());
        assert_eq!(report.scored, 26, "4 of the 30 are unanswerable");
    }

    #[test]
    fn a_retriever_that_returns_nothing_scores_zero() {
        let report = run_eval(4, |_q, _k| vec![]);
        assert_eq!(report.recall_at_k, 0.0, "{}", report.render());
        assert_eq!(report.scored, 26);
        assert_eq!(report.unanswerable, 4);
        assert_eq!(report.failures.len(), 26);
    }

    #[test]
    fn unanswerable_questions_are_excluded_from_the_averages() {
        // A retriever that is perfect on answerable questions must score 1.0,
        // even though it returns irrelevant nearest-neighbours for the four
        // unanswerable ones. Anything else means the metric is punishing the
        // retriever for the generator's job.
        let report = run_eval(4, |q, _| {
            let q = QUESTIONS.iter().find(|x| x.question == q).unwrap();
            if q.relevant.is_empty() {
                vec![99, 98] // plausible-looking junk
            } else {
                q.relevant.to_vec()
            }
        });
        assert!(
            (report.recall_at_k - 1.0).abs() < 1e-6,
            "{}",
            report.render()
        );
        assert_eq!(report.total, 30);
        assert_eq!(report.scored + report.unanswerable, report.total);
    }

    #[test]
    fn mrr_penalises_a_correct_answer_ranked_low() {
        let high = run_eval(4, |q, _| {
            QUESTIONS
                .iter()
                .find(|x| x.question == q)
                .map(|x| x.relevant.to_vec())
                .unwrap_or_default()
        });
        let low = run_eval(4, |q, _| {
            QUESTIONS
                .iter()
                .find(|x| x.question == q)
                .map(|x| {
                    if x.relevant.is_empty() {
                        vec![]
                    } else {
                        // Pad with wrong answers so the right one lands 4th.
                        let mut v = vec![-1i64, -2, -3];
                        v.extend_from_slice(x.relevant);
                        v
                    }
                })
                .unwrap_or_default()
        });
        assert!(
            low.mrr < high.mrr,
            "MRR must reward rank position: {} vs {}",
            low.mrr,
            high.mrr
        );
        assert!(
            (low.recall_at_k - high.recall_at_k).abs() < 1e-6,
            "recall is unchanged"
        );
    }

    #[test]
    fn precision_falls_when_the_context_is_padded() {
        let tight = run_eval(4, |q, _| {
            QUESTIONS
                .iter()
                .find(|x| x.question == q)
                .unwrap()
                .relevant
                .to_vec()
        });
        let padded = run_eval(4, |q, _| {
            let mut v = QUESTIONS
                .iter()
                .find(|x| x.question == q)
                .unwrap()
                .relevant
                .to_vec();
            if !v.is_empty() {
                v.extend_from_slice(&[-1, -2, -3]);
            }
            v
        });
        assert!(padded.precision_at_k < tight.precision_at_k);
    }

    #[test]
    fn the_report_renders_readably() {
        let r = run_eval(4, |_q, _k| vec![]);
        let out = r.render();
        assert!(out.contains("recall@k"));
        assert!(out.contains("MRR"));
        assert!(out.contains("by probe"));
        assert!(out.contains("failures"));
    }

    #[test]
    fn the_unanswerable_set_is_the_refusal_suite() {
        // These questions are not dead weight — they are the inputs for the
        // generation-side refusal check. Assert the contract so nobody
        // "optimises" them out of the corpus later.
        let unanswerable: Vec<&EvalQuestion> = QUESTIONS
            .iter()
            .filter(|q| q.probe == Probe::Unanswerable)
            .collect();
        assert_eq!(unanswerable.len(), 4);

        for q in unanswerable {
            // Confirm the corpus genuinely lacks the answer, so a refusal is
            // the correct behaviour rather than a retrieval miss.
            let key = crate::rag::search::tokenize(q.question);
            let distinctive: Vec<&String> = key
                .iter()
                .filter(|t| matches!(t.as_str(), "pension" | "executive" | "wifi" | "parking"))
                .collect();
            assert!(!distinctive.is_empty(), "Q{} has no distinctive term", q.id);

            for term in distinctive {
                assert!(
                    !CORPUS.iter().any(|(_, h, b)| {
                        h.to_lowercase().contains(term.as_str())
                            || b.to_lowercase().contains(term.as_str())
                    }),
                    "Q{} is marked unanswerable but {term:?} appears in the corpus",
                    q.id
                );
            }
        }
    }

    /* ---------- what the harness actually proves ---------- */

    #[test]
    fn hybrid_beats_bm25_alone_overall() {
        let h = Harness::new(true);
        let bm = run_eval(4, |q, k| h.bm25_only(q, k));
        let hy = run_eval(4, |q, k| h.hybrid(q, k));
        assert!(
            hy.recall_at_k >= bm.recall_at_k,
            "hybrid regressed against BM25 alone:\nBM25:\n{}\nHYBRID:\n{}",
            bm.render(),
            hy.render()
        );
    }

    #[test]
    fn bm25_carries_the_exact_identifier_questions() {
        // The core argument for hybrid retrieval: dense search alone loses
        // invoice numbers and account codes.
        let h = Harness::new(true);
        let exact: Vec<&EvalQuestion> = QUESTIONS
            .iter()
            .filter(|q| q.probe == Probe::Exact)
            .collect();

        let mut bm_hits = 0;
        for q in &exact {
            let got = h.bm25_only(q.question, 4);
            if q.relevant.iter().any(|r| got.contains(r)) {
                bm_hits += 1;
            }
        }
        assert!(
            bm_hits as f32 / exact.len() as f32 >= 0.85,
            "BM25 found only {bm_hits}/{} exact-identifier answers",
            exact.len()
        );
    }

    #[test]
    fn heading_indexing_measurably_improves_heading_scoped_questions() {
        // Proves the heading-trail design decision with a number rather than
        // an assertion in a comment.
        let with = Harness::new(true);
        let without = Harness::new(false);

        let scoped: Vec<&EvalQuestion> = QUESTIONS
            .iter()
            .filter(|q| q.probe == Probe::HeadingScoped)
            .collect();

        let score = |h: &Harness| {
            scoped
                .iter()
                .filter(|q| {
                    let got = h.hybrid(q.question, 4);
                    q.relevant.iter().any(|r| got.contains(r))
                })
                .count()
        };

        let a = score(&with);
        let b = score(&without);
        assert!(
            a > b,
            "heading trail gave no benefit ({a} vs {b} of {}); either the \
             questions or the indexing is wrong",
            scoped.len()
        );
    }

    #[test]
    fn hybrid_retrieval_meets_the_m2_exit_gate_on_keyword_answerable_questions() {
        // The pseudo-embedder cannot do real semantics, so gating the whole
        // suite on it would be dishonest. We gate on the subset a lexical
        // system should handle, and record the rest for the runtime eval.
        let h = Harness::new(true);
        let gated: Vec<&EvalQuestion> = QUESTIONS
            .iter()
            .filter(|q| {
                matches!(
                    q.probe,
                    Probe::Exact | Probe::HeadingScoped | Probe::Disambiguation
                )
            })
            .collect();

        let hits = gated
            .iter()
            .filter(|q| {
                let got = h.hybrid(q.question, 4);
                q.relevant.iter().any(|r| got.contains(r))
            })
            .count();

        let recall = hits as f32 / gated.len() as f32;
        assert!(
            recall >= 0.80,
            "recall@4 on lexically-answerable questions is {recall:.3}, gate is 0.80"
        );
    }

    #[test]
    fn the_superseded_document_does_not_outrank_the_current_one() {
        // Chunks 9 and 10 are near-duplicates of 1 and 6. If retrieval prefers
        // the 2019 handbook, the user gets confidently wrong answers.
        let h = Harness::new(true);
        let got = h.hybrid("current annual leave entitlement maximum days", 4);
        let pos_current = got.iter().position(|id| *id == 1);
        let pos_old = got.iter().position(|id| *id == 9);
        if let (Some(c), Some(o)) = (pos_current, pos_old) {
            assert!(
                c < o,
                "the superseded 2019 policy outranked the current one: {got:?}"
            );
        }
        assert!(
            pos_current.is_some(),
            "current policy not retrieved at all: {got:?}"
        );
    }

    #[test]
    fn pseudo_embed_is_deterministic_and_normalised() {
        let a = pseudo_embed("annual leave policy", 256);
        let b = pseudo_embed("annual leave policy", 256);
        assert_eq!(a, b);
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn pseudo_embed_puts_related_text_closer_than_unrelated() {
        let q = pseudo_embed("annual leave accrual days", 256);
        let related = pseudo_embed(CORPUS[0].2, 256);
        let unrelated = pseudo_embed(CORPUS[18].2, 256);
        assert!(
            crate::rag::search::cosine(&q, &related) > crate::rag::search::cosine(&q, &unrelated)
        );
    }
}
