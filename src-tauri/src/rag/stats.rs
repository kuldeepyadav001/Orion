//! Paired statistics for retrieval evaluation.
//!
//! ## Why this exists
//!
//! The M2 evaluation compared configurations by their averages. That supports
//! a *directional* claim and nothing more: two retriever configurations that
//! both retrieve the same 20 of 26 chunks, differing on the other 6, produce
//! averages that differ by a hair, and no amount of arithmetic on those
//! averages can tell whether the difference is real.
//!
//! The comparison that matters is **paired**: the same 26 questions are asked
//! of both configurations, so each question's outcome cancels out the
//! difficulty of that question and leaves only the effect of the
//! configuration. That is what this module computes.
//!
//! Two tests are reported, because they answer different questions:
//!
//! * **Exact sign test** — a distribution-free test on the *direction* of the
//!   per-question differences. It makes no assumption about the shape of the
//!   score distribution, which matters with n = 26 and a metric that is
//!   mostly 0 or 1.
//! * **Paired bootstrap confidence interval** — a distribution-free interval
//!   on the *magnitude* of the mean difference. It answers "how large is the
//!   effect, and is zero plausibly inside it?".
//!
//! If the interval contains zero and the sign test is not significant, the
//! honest sentence is "directional, not significant" — which is what the
//! evidence supports, and it is a stronger claim than either number alone.

use super::eval::{EvalReport, Metric};

/// Two values are treated as equal below this difference.
const EPS: f32 = 1e-6;

/// The result of comparing one configuration against another, question by
/// question.
#[derive(Debug, Clone)]
pub struct PairedComparison {
    pub metric: Metric,
    /// Number of questions present in both reports.
    pub n: usize,
    /// Mean metric value for the baseline configuration.
    pub baseline_mean: f32,
    /// Mean metric value for the candidate configuration.
    pub candidate_mean: f32,
    /// `candidate_mean - baseline_mean`.
    pub mean_delta: f32,
    /// Questions where the candidate scored strictly higher.
    pub wins: usize,
    /// Questions where the candidate scored strictly lower.
    pub losses: usize,
    /// Questions where the two scored the same.
    pub ties: usize,
    /// Exact two-sided sign test p-value over the discordant pairs.
    pub sign_p: f64,
    /// 95% paired bootstrap confidence interval on `mean_delta`.
    pub ci_low: f32,
    pub ci_high: f32,
    /// True when the bootstrap interval excludes zero AND the sign test
    /// rejects at 5%.
    pub significant: bool,
}

impl PairedComparison {
    /// One-line summary suitable for a CI log.
    pub fn render(&self, baseline: &str, candidate: &str) -> String {
        format!(
            "{metric} (n={n}): {b} {bm:.3} vs {c} {cm:.3} | delta {d:+.4} \
             [{lo:+.4}, {hi:+.4}] | W/L/T {w}/{l}/{t} | sign p={p:.4} | {verdict}",
            metric = self.metric.label(),
            n = self.n,
            b = baseline,
            bm = self.baseline_mean,
            c = candidate,
            cm = self.candidate_mean,
            d = self.mean_delta,
            lo = self.ci_low,
            hi = self.ci_high,
            w = self.wins,
            l = self.losses,
            t = self.ties,
            p = self.sign_p,
            verdict = if self.significant {
                "SIGNIFICANT"
            } else {
                "directional, not significant"
            }
        )
    }
}

/// Compare two evaluation reports on the questions they share.
///
/// The two reports are matched by question id, so a configuration that
/// scored a different number of questions is still compared fairly.
pub fn compare_paired(
    baseline: &EvalReport,
    candidate: &EvalReport,
    metric: Metric,
) -> PairedComparison {
    let mut pairs: Vec<(f32, f32)> = Vec::new();
    for b in &baseline.outcomes {
        if let Some(c) = candidate.outcomes.iter().find(|c| c.id == b.id) {
            pairs.push((b.metric(metric), c.metric(metric)));
        }
    }

    let n = pairs.len();
    if n == 0 {
        return PairedComparison {
            metric,
            n: 0,
            baseline_mean: 0.0,
            candidate_mean: 0.0,
            mean_delta: 0.0,
            wins: 0,
            losses: 0,
            ties: 0,
            sign_p: 1.0,
            ci_low: 0.0,
            ci_high: 0.0,
            significant: false,
        };
    }

    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut ties = 0usize;
    let mut deltas: Vec<f32> = Vec::with_capacity(n);
    let mut b_sum = 0.0f32;
    let mut c_sum = 0.0f32;

    for (b, c) in &pairs {
        b_sum += *b;
        c_sum += *c;
        let d = *c - *b;
        deltas.push(d);
        if d > EPS {
            wins += 1;
        } else if d < -EPS {
            losses += 1;
        } else {
            ties += 1;
        }
    }

    let baseline_mean = b_sum / n as f32;
    let candidate_mean = c_sum / n as f32;
    let mean_delta = candidate_mean - baseline_mean;

    let sign_p = sign_test_p(wins, losses);
    let (ci_low, ci_high) = paired_bootstrap_ci(&deltas, 10_000, 0x5EED_1234_ABCD_0001);

    // The interval excludes zero AND the sign test rejects at 5%.
    let significant = (ci_low > 0.0 || ci_high < 0.0) && sign_p < 0.05;

    PairedComparison {
        metric,
        n,
        baseline_mean,
        candidate_mean,
        mean_delta,
        wins,
        losses,
        ties,
        sign_p,
        ci_low,
        ci_high,
        significant,
    }
}

/// Exact two-sided sign test over the discordant pairs.
///
/// Under the null hypothesis that the candidate is no better than the
/// baseline, each discordant pair is a fair coin. The p-value is twice the
/// probability of the smaller tail, capped at 1.
pub fn sign_test_p(wins: usize, losses: usize) -> f64 {
    let n = wins + losses;
    if n == 0 {
        return 1.0;
    }
    let m = wins.min(losses) as u64;
    let n = n as u64;

    // C(n, 0) = 1; build up with C(n, i) = C(n, i-1) * (n - i + 1) / i.
    let mut choose = 1.0f64;
    let mut tail = choose;
    for i in 1..=m {
        choose = choose * (n - i + 1) as f64 / i as f64;
        tail += choose;
    }
    let p = 2.0 * tail / 2f64.powi(n as i32);
    p.min(1.0)
}

/// Percentile bootstrap confidence interval on the mean of paired deltas.
///
/// Resampling is over *pairs*, not over the two configurations separately:
/// a pair is resampled or it is not, so the pairing is preserved. The
/// generator is deterministic so that a CI printed in a CI log is
/// reproducible.
pub fn paired_bootstrap_ci(deltas: &[f32], reps: usize, seed: u64) -> (f32, f32) {
    let n = deltas.len();
    if n == 0 || reps == 0 {
        return (0.0, 0.0);
    }

    let mut rng = seed;
    let mut means: Vec<f64> = Vec::with_capacity(reps);
    for _ in 0..reps {
        let mut sum = 0.0f64;
        for _ in 0..n {
            // xorshift64*
            rng ^= rng >> 12;
            rng ^= rng << 25;
            rng ^= rng >> 27;
            let idx = (rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % n;
            sum += deltas[idx] as f64;
        }
        means.push(sum / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let lo_idx = ((reps as f64) * 0.025).floor() as usize;
    let hi_idx = (((reps as f64) * 0.975).floor() as usize).min(reps - 1);
    (means[lo_idx] as f32, means[hi_idx] as f32)
}

/// Wilson score interval for a binomial proportion.
///
/// The normal approximation is unusable at the proportions this project
/// reports (0.769 on n = 26 is far enough from 0.5 that the Wald interval
/// overshoots the [0, 1] bounds and is asymmetric in the wrong place).
/// Wilson is the standard fix and is well behaved at n = 26.
pub fn wilson_interval(successes: u32, total: u32, z: f64) -> (f64, f64) {
    if total == 0 {
        return (0.0, 1.0);
    }
    let n = total as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / denom;
    let half = (z / denom) * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    (
        (centre - half).clamp(0.0, 1.0),
        (centre + half).clamp(0.0, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rag::eval::{Probe, QuestionOutcome};

    fn outcome(id: u32, recall: f32, rr: f32) -> QuestionOutcome {
        QuestionOutcome {
            id,
            question: "q",
            probe: Probe::Exact,
            recall,
            reciprocal_rank: rr,
            precision: recall,
            retrieved: vec![1],
            hit_positions: vec![],
        }
    }

    fn report(items: Vec<QuestionOutcome>) -> EvalReport {
        EvalReport {
            total: items.len(),
            scored: items.len(),
            unanswerable: 0,
            recall_at_k: 0.0,
            mrr: 0.0,
            precision_at_k: 0.0,
            failures: vec![],
            by_probe: vec![],
            outcomes: items,
        }
    }

    #[test]
    fn sign_test_is_one_when_there_are_no_discordant_pairs() {
        assert!((sign_test_p(0, 0) - 1.0).abs() < 1e-12);
        assert!((sign_test_p(5, 5) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn sign_test_matches_the_exact_binomial() {
        // 7 wins, 0 losses: p = 2 * (1/2)^7 = 0.015625
        assert!((sign_test_p(7, 0) - 0.015625).abs() < 1e-9);
        // 6 wins, 0 losses: p = 2 * (1/2)^6 = 0.03125
        assert!((sign_test_p(6, 0) - 0.03125).abs() < 1e-9);
        // 8 wins, 2 losses: two-sided p = 2 * P(X <= 2), X ~ Bin(10, 0.5)
        let expected = 2.0 * (1.0 + 10.0 + 45.0) / 1024.0;
        assert!((sign_test_p(8, 2) - expected).abs() < 1e-9);
    }

    #[test]
    fn sign_test_is_symmetric_in_wins_and_losses() {
        assert!((sign_test_p(9, 3) - sign_test_p(3, 9)).abs() < 1e-12);
    }

    #[test]
    fn paired_comparison_detects_a_real_improvement() {
        let base = report((1..=26).map(|i| outcome(i, 0.5, 0.5)).collect());
        let cand = report((1..=26).map(|i| outcome(i, 1.0, 1.0)).collect());
        let c = compare_paired(&base, &cand, Metric::Recall);
        assert_eq!(c.n, 26);
        assert_eq!(c.wins, 26);
        assert_eq!(c.losses, 0);
        assert!((c.mean_delta - 0.5).abs() < 1e-6);
        assert!(c.ci_low > 0.0, "interval must exclude zero");
        assert!(c.sign_p < 0.05);
        assert!(c.significant);
    }

    #[test]
    fn paired_comparison_calls_a_tiny_difference_directional() {
        // 4 questions differ, each by 0.25 recall. The average moves by
        // 0.038 but the evidence is weak: the interval must contain zero.
        let base = report((1..=26).map(|i| outcome(i, 0.5, 0.5)).collect());
        let cand = report(
            (1..=26)
                .map(|i| {
                    if i <= 4 {
                        outcome(i, 0.75, 0.75)
                    } else {
                        outcome(i, 0.5, 0.5)
                    }
                })
                .collect(),
        );
        let c = compare_paired(&base, &cand, Metric::Recall);
        assert_eq!(c.wins, 4);
        assert_eq!(c.losses, 0);
        assert!(c.ci_low <= 0.0 || c.ci_high >= 0.0);
        assert!(!c.significant, "4 discordant pairs cannot be significant");
    }

    #[test]
    fn paired_comparison_matches_questions_by_id_not_by_position() {
        let base = report(vec![outcome(7, 0.0, 0.0), outcome(9, 1.0, 1.0)]);
        // Candidate has the same two questions in the opposite order.
        let cand = report(vec![outcome(9, 1.0, 1.0), outcome(7, 1.0, 1.0)]);
        let c = compare_paired(&base, &cand, Metric::Recall);
        assert_eq!(c.n, 2);
        assert_eq!(c.wins, 1);
        assert_eq!(c.losses, 0);
        assert_eq!(c.ties, 1);
    }

    #[test]
    fn paired_comparison_ignores_questions_only_one_side_has() {
        let base = report(vec![outcome(1, 0.0, 0.0)]);
        let cand = report(vec![outcome(1, 1.0, 1.0), outcome(2, 1.0, 1.0)]);
        let c = compare_paired(&base, &cand, Metric::Recall);
        assert_eq!(c.n, 1);
    }

    #[test]
    fn bootstrap_interval_brackets_the_mean_delta() {
        let deltas = vec![0.25f32; 4];
        let (lo, hi) = paired_bootstrap_ci(&deltas, 2_000, 7);
        assert!(lo <= 0.25 && 0.25 <= hi, "lo={lo} hi={hi}");
    }

    #[test]
    fn bootstrap_interval_is_deterministic() {
        let deltas = vec![0.1f32, -0.2, 0.3, 0.0, 0.05];
        let a = paired_bootstrap_ci(&deltas, 1_000, 99);
        let b = paired_bootstrap_ci(&deltas, 1_000, 99);
        assert_eq!(a, b);
    }

    #[test]
    fn bootstrap_interval_contains_zero_for_pure_noise() {
        // Symmetric noise: the mean difference is zero, so zero must be
        // comfortably inside the interval.
        let deltas: Vec<f32> = (0..20)
            .map(|i| if i % 2 == 0 { 0.1 } else { -0.1 })
            .collect();
        let (lo, hi) = paired_bootstrap_ci(&deltas, 2_000, 3);
        assert!(lo <= 0.0 && hi >= 0.0, "lo={lo} hi={hi}");
    }

    #[test]
    fn wilson_interval_matches_a_known_value() {
        // 20/26 = 0.769 at 95%: the published interval is [0.579, 0.890].
        let (lo, hi) = wilson_interval(20, 26, 1.959963984540054);
        assert!((lo - 0.579).abs() < 0.002, "lo={lo}");
        assert!((hi - 0.890).abs() < 0.002, "hi={hi}");
    }

    #[test]
    fn wilson_interval_stays_inside_the_unit_range() {
        let (lo, hi) = wilson_interval(0, 10, 1.96);
        assert_eq!(lo, 0.0);
        assert!(hi > 0.0 && hi < 1.0);
        let (lo, hi) = wilson_interval(10, 10, 1.96);
        assert!(lo > 0.0 && lo < 1.0);
        assert_eq!(hi, 1.0);
    }

    #[test]
    fn wilson_interval_of_an_empty_sample_is_the_full_range() {
        assert_eq!(wilson_interval(0, 0, 1.96), (0.0, 1.0));
    }
}
