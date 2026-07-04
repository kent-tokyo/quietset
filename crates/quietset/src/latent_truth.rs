use crate::config::LatentTruthSafety;
use crate::observation::Observation;
use indexmap::IndexMap;
use std::collections::{BTreeSet, HashMap};

const MAX_ITERATIONS: usize = 20;
const CONVERGENCE_EPSILON: f64 = 1e-4;
const SMOOTHING_ALPHA: f64 = 0.5;
/// Below this many samples shared by an evaluator pair, their pairwise correlation is
/// statistically unreliable (e.g. a 1/1 shared sample trivially gives ±1 with zero power).
const MIN_SHARED_SAMPLES_FOR_CORRELATION: usize = 5;
/// `correlated_evaluator_warning` fires when a sample's effective evaluator count falls
/// below this fraction of its nominal (qualifying) evaluator count — "less than 70% of
/// nominal independent evidence."
const EFFECTIVE_N_WARNING_RATIO: f64 = 0.7;

/// EM-estimated latent truth for one sample: the most likely label, its posterior
/// confidence, and the full posterior distribution.
#[derive(Debug, Clone)]
pub struct LatentTruthEstimate {
    pub label: String,
    pub confidence: f64,
    pub distribution: IndexMap<String, f64>,
    /// Effective number of independent evaluators behind this estimate, correcting for
    /// pairwise-correlated evaluator errors (Kish design effect). Equals the nominal
    /// qualifying evaluator count when no correlation is detected; always in `(0, n]`.
    pub evaluator_effective_n: f64,
    /// True when `evaluator_effective_n` is below `EFFECTIVE_N_WARNING_RATIO` of the nominal
    /// evaluator count — a "N evaluators agree" reading is really closer to a smaller number
    /// of independent confirmations.
    pub correlated_evaluator_warning: bool,
    /// True if the EM loop's `delta` dropped below `CONVERGENCE_EPSILON` before exhausting
    /// `MAX_ITERATIONS`. False means the loop ran the full cap without settling — `pi` is
    /// still a valid posterior (monotone log-likelihood, never worse than majority-vote init)
    /// but a less certain one. Same value for every sample in a batch (one EM run per batch).
    pub converged: bool,
    /// Number of EM iterations actually run (`1..=MAX_ITERATIONS`). Same value for every
    /// sample in a batch.
    pub iterations: usize,
    /// The `delta` (summed absolute change in `pi`) from the final iteration run. Same value
    /// for every sample in a batch. Useful alongside `converged` to see how close a
    /// non-converged run got to `CONVERGENCE_EPSILON`.
    pub convergence_delta: f64,
}

/// Dawid-Skene EM: infers per-evaluator confusion matrices and per-sample label posteriors
/// from disagreement patterns alone, with no `gold_label` required.
///
/// Only observations with both `label` and `evaluator_id` set participate (same filter as
/// [`crate::weighting::compute_evaluator_weights`]). Returns `None` when the batch has fewer
/// than 2 distinct qualifying labels or fewer than 2 distinct qualifying evaluators — too
/// little signal for EM to say anything beyond the plain majority vote. Samples with no
/// qualifying observations get no entry in the returned map.
pub fn compute_latent_truth(
    observations: &[Observation],
) -> Option<HashMap<String, LatentTruthEstimate>> {
    let mut sample_obs: IndexMap<String, Vec<(String, String)>> = IndexMap::new();
    for o in observations {
        if let (Some(label), Some(eval_id)) = (o.label.as_deref(), o.evaluator_id.as_deref()) {
            sample_obs
                .entry(o.sample_id.clone())
                .or_default()
                .push((eval_id.to_string(), label.to_string()));
        }
    }

    let mut labels_set: BTreeSet<String> = BTreeSet::new();
    let mut evaluators_set: BTreeSet<String> = BTreeSet::new();
    for pairs in sample_obs.values() {
        for (eval_id, label) in pairs {
            evaluators_set.insert(eval_id.clone());
            labels_set.insert(label.clone());
        }
    }
    let labels_sorted: Vec<String> = labels_set.into_iter().collect();
    let evaluators_sorted: Vec<String> = evaluators_set.into_iter().collect();
    let k = labels_sorted.len();
    let j = evaluators_sorted.len();
    if k < 2 || j < 2 {
        return None;
    }

    let label_index: HashMap<&str, usize> = labels_sorted
        .iter()
        .enumerate()
        .map(|(i, l)| (l.as_str(), i))
        .collect();
    let eval_index: HashMap<&str, usize> = evaluators_sorted
        .iter()
        .enumerate()
        .map(|(i, e)| (e.as_str(), i))
        .collect();

    let sample_ids: Vec<String> = sample_obs.keys().cloned().collect();
    let samples: Vec<Vec<(usize, usize)>> = sample_obs
        .values()
        .map(|pairs| {
            pairs
                .iter()
                .map(|(e, l)| (eval_index[e.as_str()], label_index[l.as_str()]))
                .collect()
        })
        .collect();

    // Init: one-hot on each sample's own majority label (tiebreak: count desc, then label asc
    // — `l` ascending already matches alphabetical order since `labels_sorted` is sorted).
    let mut pi: Vec<Vec<f64>> = samples
        .iter()
        .map(|obs| {
            let mut counts = vec![0usize; k];
            for &(_, l) in obs {
                counts[l] += 1;
            }
            let mut best = 0usize;
            for l in 1..k {
                if counts[l] > counts[best] {
                    best = l;
                }
            }
            let mut v = vec![0.0; k];
            v[best] = 1.0;
            v
        })
        .collect();

    let mut iterations = 0usize;
    let mut convergence_delta = f64::INFINITY;
    let mut converged = false;
    for _ in 0..MAX_ITERATIONS {
        let new_pi = em_step(k, j, &samples, &pi);
        let delta: f64 = pi
            .iter()
            .zip(&new_pi)
            .map(|(old, new)| old.iter().zip(new).map(|(a, b)| (a - b).abs()).sum::<f64>())
            .sum();
        pi = new_pi;
        iterations += 1;
        convergence_delta = delta;
        if delta < CONVERGENCE_EPSILON {
            // EM's log-likelihood is monotone non-decreasing every iteration, so if the cap
            // is hit without this ever firing, the last `pi` is still a valid (if perhaps
            // low-confidence) posterior — never worse than the majority-vote init.
            converged = true;
            break;
        }
    }

    let diagnostics = evaluator_effective_n_diagnostics(j, &samples, &pi);

    let mut result = HashMap::with_capacity(sample_ids.len());
    for (s, sid) in sample_ids.into_iter().enumerate() {
        let p = &pi[s];
        let mut best = 0usize;
        for l in 1..k {
            if p[l] > p[best] {
                best = l;
            }
        }
        let mut dist_pairs: Vec<(usize, f64)> = (0..k).map(|l| (l, p[l])).collect();
        dist_pairs.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        let distribution: IndexMap<String, f64> = dist_pairs
            .into_iter()
            .map(|(l, prob)| (labels_sorted[l].clone(), prob))
            .collect();
        let (evaluator_effective_n, correlated_evaluator_warning) = diagnostics[s];
        result.insert(
            sid,
            LatentTruthEstimate {
                label: labels_sorted[best].clone(),
                confidence: p[best],
                distribution,
                evaluator_effective_n,
                correlated_evaluator_warning,
                converged,
                iterations,
                convergence_delta,
            },
        );
    }
    Some(result)
}

/// Post-hoc diagnostic pass over converged `pi`: for each sample, the effective number of
/// independent evaluators among its own qualifying evaluators (Kish design effect:
/// `DEFF = 1 + (n-1)*avg_pairwise_correlation`, `effective_n = n / DEFF`), and whether
/// that's materially below the nominal count. Does not affect EM — `pi`/`theta` are already
/// converged by the time this runs.
///
/// This is a diagnostic of *self-consistency*, not an independent correctness check:
/// correlation is measured against EM's own converged label, since no external gold exists.
/// It can only detect correlated evaluators when *other* evaluators elsewhere reveal their
/// shared deviation — if a correlated block captures the EM consensus outright, those
/// evaluators match the converged label on every shared sample (zero variance, `phi`
/// undefined, defaults to neutral) and no warning fires. Same blind spot as
/// `LatentTruthEstimate::confidence`'s caveat, not a bug.
fn evaluator_effective_n_diagnostics(
    j: usize,
    samples: &[Vec<(usize, usize)>],
    pi: &[Vec<f64>],
) -> Vec<(f64, bool)> {
    let final_label: Vec<usize> = pi
        .iter()
        .map(|p| {
            let mut best = 0usize;
            for l in 1..p.len() {
                if p[l] > p[best] {
                    best = l;
                }
            }
            best
        })
        .collect();

    // Per-evaluator match indicator: sample_idx -> did this evaluator's label match the
    // sample's final latent-truth label. An evaluator with >1 observation on the same sample
    // (not deduped elsewhere in this file either) OR-merges into a single indicator.
    let mut indicators: Vec<HashMap<usize, bool>> = vec![HashMap::new(); j];
    for (s, obs) in samples.iter().enumerate() {
        for &(jj, lp) in obs {
            let matched = lp == final_label[s];
            let entry = indicators[jj].entry(s).or_insert(false);
            *entry = *entry || matched;
        }
    }

    let mut pair_cache: HashMap<(usize, usize), f64> = HashMap::new();
    let mut results = Vec::with_capacity(samples.len());
    for obs in samples {
        let evaluators: Vec<usize> = obs
            .iter()
            .map(|&(jj, _)| jj)
            .collect::<BTreeSet<usize>>()
            .into_iter()
            .collect();
        let n = evaluators.len();
        if n <= 1 {
            results.push((n as f64, false));
            continue;
        }
        let mut sum_corr = 0.0;
        let mut pairs = 0usize;
        for i in 0..evaluators.len() {
            for k2 in (i + 1)..evaluators.len() {
                let (a, b) = (evaluators[i], evaluators[k2]);
                let key = if a < b { (a, b) } else { (b, a) };
                let corr = *pair_cache.entry(key).or_insert_with(|| {
                    evaluator_pairwise_correlation(&indicators[key.0], &indicators[key.1])
                });
                sum_corr += corr;
                pairs += 1;
            }
        }
        let rho = sum_corr / pairs as f64;
        let deff = 1.0 + (n as f64 - 1.0) * rho;
        let effective_n = n as f64 / deff;
        let warning = effective_n < EFFECTIVE_N_WARNING_RATIO * n as f64;
        results.push((effective_n, warning));
    }
    results
}

/// Phi coefficient (binary Pearson correlation) between two evaluators' match indicators,
/// restricted to samples where both have a qualifying observation. Returns `0.0` (neutral —
/// "assume independent," mirroring `weighting.rs`'s `.unwrap_or(1.0)`-style convention for
/// sparse/missing evidence) if fewer than `MIN_SHARED_SAMPLES_FOR_CORRELATION` samples are
/// shared, or if phi is undefined (one indicator has zero variance over the shared set).
/// Clamped to `[0.0, 1.0]` — only positive, redundancy-indicating correlation is modeled;
/// this diagnostic exists to catch two evaluators moving together, not to award bonus
/// independence for anti-correlation.
fn evaluator_pairwise_correlation(a: &HashMap<usize, bool>, b: &HashMap<usize, bool>) -> f64 {
    let shared: Vec<(bool, bool)> = a
        .iter()
        .filter_map(|(sample, &va)| b.get(sample).map(|&vb| (va, vb)))
        .collect();
    if shared.len() < MIN_SHARED_SAMPLES_FOR_CORRELATION {
        return 0.0;
    }
    let (mut n11, mut n10, mut n01, mut n00) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for (va, vb) in &shared {
        match (va, vb) {
            (true, true) => n11 += 1.0,
            (true, false) => n10 += 1.0,
            (false, true) => n01 += 1.0,
            (false, false) => n00 += 1.0,
        }
    }
    let n1_ = n11 + n10;
    let n0_ = n01 + n00;
    let n_1 = n11 + n01;
    let n_0 = n10 + n00;
    let denom = (n1_ * n0_ * n_1 * n_0).sqrt();
    if denom == 0.0 {
        return 0.0;
    }
    let phi = (n11 * n00 - n10 * n01) / denom;
    phi.clamp(0.0, 1.0)
}

/// Decides whether a latent-truth `Keep` should be demoted to `Review`, and why. Checked in a
/// fixed priority order when multiple conditions fire simultaneously (same fixed-declaration-
/// order tiebreak convention as `StabilityComponents::weakest()`): the relative correlated-
/// evaluator warning first, then the absolute effective-n floor, then non-convergence. Pure
/// and independent of EM — takes only the already-converged `LatentTruthEstimate` plus config,
/// so it's directly unit-testable without running EM to convergence.
pub(crate) fn latent_truth_demotion_reason(
    estimate: &LatentTruthEstimate,
    safety: &LatentTruthSafety,
) -> Option<&'static str> {
    if safety.demote_on_correlated_warning && estimate.correlated_evaluator_warning {
        return Some("correlated_evaluator_warning");
    }
    if let Some(min_n) = safety.min_evaluator_effective_n
        && estimate.evaluator_effective_n < min_n
    {
        return Some("evaluator_effective_n_below_minimum");
    }
    if safety.demote_on_non_convergence && !estimate.converged {
        return Some("latent_truth_not_converged");
    }
    None
}

/// One Dawid-Skene M-step + E-step. `samples[s]` is sample `s`'s qualifying
/// `(evaluator_idx, label_idx)` observations; `pi[s][l]` is its current posterior for label
/// `l`. Returns updated posteriors. Laplace smoothing (`SMOOTHING_ALPHA`) keeps every
/// confusion-matrix entry and class prior strictly positive, so the E-step's `ln` never hits
/// `-inf`.
fn em_step(k: usize, j: usize, samples: &[Vec<(usize, usize)>], pi: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let m = samples.len();

    // M-step: confusion matrices theta[evaluator][true_label][reported_label].
    let mut counts = vec![vec![vec![0.0f64; k]; k]; j];
    for (s, obs) in samples.iter().enumerate() {
        for &(jj, lp) in obs {
            for (l, count) in counts[jj].iter_mut().enumerate() {
                count[lp] += pi[s][l];
            }
        }
    }
    let mut theta = vec![vec![vec![0.0f64; k]; k]; j];
    for jj in 0..j {
        for l in 0..k {
            let row_total: f64 = counts[jj][l].iter().sum();
            for lp in 0..k {
                theta[jj][l][lp] = (counts[jj][l][lp] + SMOOTHING_ALPHA)
                    / (row_total + SMOOTHING_ALPHA * k as f64);
            }
        }
    }

    // Class prior.
    let mut prior = vec![0.0f64; k];
    for (l, prior_l) in prior.iter_mut().enumerate() {
        let sum_pi: f64 = pi.iter().map(|p| p[l]).sum();
        *prior_l = (sum_pi + SMOOTHING_ALPHA) / (m as f64 + SMOOTHING_ALPHA * k as f64);
    }

    // E-step: log-space posterior, softmax-normalized.
    samples
        .iter()
        .map(|obs| {
            let log_score: Vec<f64> = (0..k)
                .map(|l| {
                    prior[l].ln()
                        + obs
                            .iter()
                            .map(|&(jj, lp)| theta[jj][l][lp].ln())
                            .sum::<f64>()
                })
                .collect();
            let max_log = log_score.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let exp: Vec<f64> = log_score.iter().map(|&x| (x - max_log).exp()).collect();
            let sum_exp: f64 = exp.iter().sum();
            exp.iter().map(|&x| x / sum_exp).collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LatentTruthSafety;

    fn estimate(
        evaluator_effective_n: f64,
        correlated_evaluator_warning: bool,
        converged: bool,
    ) -> LatentTruthEstimate {
        LatentTruthEstimate {
            label: "a".into(),
            confidence: 0.9,
            distribution: IndexMap::new(),
            evaluator_effective_n,
            correlated_evaluator_warning,
            converged,
            iterations: 1,
            convergence_delta: 0.0,
        }
    }

    #[test]
    fn test_demotion_reason_correlated_warning_alone() {
        let e = estimate(3.0, true, true);
        let safety = LatentTruthSafety {
            demote_on_correlated_warning: true,
            ..Default::default()
        };
        assert_eq!(
            latent_truth_demotion_reason(&e, &safety),
            Some("correlated_evaluator_warning")
        );
    }

    #[test]
    fn test_demotion_reason_min_effective_n_alone() {
        let e = estimate(1.5, false, true);
        let safety = LatentTruthSafety {
            min_evaluator_effective_n: Some(2.0),
            ..Default::default()
        };
        assert_eq!(
            latent_truth_demotion_reason(&e, &safety),
            Some("evaluator_effective_n_below_minimum")
        );
    }

    #[test]
    fn test_demotion_reason_non_convergence_alone() {
        let e = estimate(3.0, false, false);
        let safety = LatentTruthSafety {
            demote_on_non_convergence: true,
            ..Default::default()
        };
        assert_eq!(
            latent_truth_demotion_reason(&e, &safety),
            Some("latent_truth_not_converged")
        );
    }

    #[test]
    fn test_demotion_reason_all_flags_off_returns_none() {
        // Every condition would fire, but LatentTruthSafety::default() has all flags off.
        let e = estimate(1.0, true, false);
        assert_eq!(
            latent_truth_demotion_reason(&e, &LatentTruthSafety::default()),
            None
        );
    }

    #[test]
    fn test_demotion_reason_priority_order() {
        // All three conditions fire, all three flags on -> correlated_evaluator_warning wins.
        let e = estimate(1.0, true, false);
        let safety = LatentTruthSafety {
            demote_on_correlated_warning: true,
            min_evaluator_effective_n: Some(2.0),
            demote_on_non_convergence: true,
        };
        assert_eq!(
            latent_truth_demotion_reason(&e, &safety),
            Some("correlated_evaluator_warning")
        );

        // Correlated flag off, but min-n and non-convergence both fire -> min-n wins.
        let safety2 = LatentTruthSafety {
            demote_on_correlated_warning: false,
            min_evaluator_effective_n: Some(2.0),
            demote_on_non_convergence: true,
        };
        assert_eq!(
            latent_truth_demotion_reason(&e, &safety2),
            Some("evaluator_effective_n_below_minimum")
        );
    }

    /// Two perfectly-agreeing evaluators (A, B), two samples each unanimous on a different
    /// label. Hand-derived: after majority-vote init `pi = [[1,0],[0,1]]`, one M+E step gives
    /// each evaluator's confusion matrix diagonal `theta = 0.75` (`(1+0.5)/(1+1)`) and
    /// off-diagonal `0.25`, symmetric priors of `0.5` cancel in the softmax, leaving a
    /// likelihood ratio of `(0.75*0.75)/(0.25*0.25) = 9`, i.e. posteriors of exactly `0.9`/`0.1`.
    #[test]
    fn test_em_step_exact_two_evaluator_two_label() {
        let k = 2;
        let j = 2;
        // label 0, label 1; evaluator 0 = A, evaluator 1 = B
        let samples = vec![
            vec![(0, 0), (1, 0)], // sample "s1": A says label0, B says label0
            vec![(0, 1), (1, 1)], // sample "s2": A says label1, B says label1
        ];
        let pi = vec![vec![1.0, 0.0], vec![0.0, 1.0]];

        let new_pi = em_step(k, j, &samples, &pi);

        assert!((new_pi[0][0] - 0.9).abs() < 1e-9, "{:?}", new_pi[0]);
        assert!((new_pi[0][1] - 0.1).abs() < 1e-9, "{:?}", new_pi[0]);
        assert!((new_pi[1][0] - 0.1).abs() < 1e-9, "{:?}", new_pi[1]);
        assert!((new_pi[1][1] - 0.9).abs() < 1e-9, "{:?}", new_pi[1]);
    }

    /// Many evaluators split exactly 50/50 on every sample (maximally ambiguous, no
    /// evaluator ever agrees with any other more than chance) — EM should never converge to
    /// a confident posterior, likely exhausting `MAX_ITERATIONS`, but must still terminate
    /// and return valid (finite, normalized) posteriors rather than panicking or producing
    /// NaN.
    #[test]
    fn test_compute_latent_truth_terminates_on_adversarial_tie() {
        let mut observations = Vec::new();
        for s in 0..6 {
            let sample_id = format!("s{s}");
            for e in 0..4 {
                let label = if (s + e) % 2 == 0 { "a" } else { "b" };
                observations.push(Observation {
                    sample_id: sample_id.clone(),
                    label: Some(label.to_string()),
                    evaluator_id: Some(format!("eval{e}")),
                    ..Default::default()
                });
            }
        }

        let result = compute_latent_truth(&observations).expect("k=2, j=4 should run EM");
        assert_eq!(result.len(), 6);
        for estimate in result.values() {
            assert!(estimate.confidence.is_finite());
            assert!((0.0..=1.0).contains(&estimate.confidence));
            let sum: f64 = estimate.distribution.values().sum();
            assert!(
                (sum - 1.0).abs() < 1e-6,
                "distribution should sum to 1: {sum}"
            );
        }
    }

    #[test]
    fn test_compute_latent_truth_none_with_single_evaluator() {
        let observations: Vec<Observation> = (0..4)
            .map(|i| Observation {
                sample_id: format!("s{i}"),
                label: Some(if i % 2 == 0 { "a" } else { "b" }.to_string()),
                evaluator_id: Some("only_one".to_string()),
                ..Default::default()
            })
            .collect();
        assert!(compute_latent_truth(&observations).is_none());
    }

    #[test]
    fn test_compute_latent_truth_none_with_single_label() {
        let observations: Vec<Observation> = (0..4)
            .map(|i| Observation {
                sample_id: format!("s{i}"),
                label: Some("only_label".to_string()),
                evaluator_id: Some(format!("eval{i}")),
                ..Default::default()
            })
            .collect();
        assert!(compute_latent_truth(&observations).is_none());
    }

    #[test]
    fn test_evaluator_pairwise_correlation_exact_full_correlation() {
        // 6 shared samples (>= MIN_SHARED_SAMPLES_FOR_CORRELATION), identical indicators with
        // genuine variance (3 true, 3 false) -> n11=3, n00=3, n10=n01=0 -> phi = 9/9 = 1.0.
        let a: HashMap<usize, bool> = (0..6).map(|s| (s, s < 3)).collect();
        let b = a.clone();
        assert!((evaluator_pairwise_correlation(&a, &b) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_evaluator_pairwise_correlation_below_min_shared_defaults_to_zero() {
        // Only 2 shared samples, both "matched" -- would look perfectly correlated if it
        // were exact, but MIN_SHARED_SAMPLES_FOR_CORRELATION (5) isn't met, so this must
        // default to 0.0 (neutral / "assume independent") rather than 1.0.
        let a: HashMap<usize, bool> = [(0, true), (1, true)].into_iter().collect();
        let b = a.clone();
        assert_eq!(evaluator_pairwise_correlation(&a, &b), 0.0);
    }

    #[test]
    fn test_evaluator_pairwise_correlation_independent_contingency_computed() {
        // 8 shared samples, balanced 2x2 contingency (n11=n10=n01=n00=2) -> phi = 0.0,
        // genuinely computed (not floor-defaulted, since 8 >= 5).
        let a: HashMap<usize, bool> = (0..8).map(|s| (s, s < 4)).collect();
        let b: HashMap<usize, bool> = (0..8).map(|s| (s, s % 4 < 2)).collect();
        assert_eq!(evaluator_pairwise_correlation(&a, &b), 0.0);
    }

    #[test]
    fn test_evaluator_effective_n_diagnostics_single_evaluator() {
        let samples = vec![vec![(0usize, 0usize)]];
        let pi = vec![vec![1.0, 0.0]];
        let result = evaluator_effective_n_diagnostics(1, &samples, &pi);
        assert_eq!(result, vec![(1.0, false)]);
    }

    #[test]
    fn test_evaluator_effective_n_diagnostics_arithmetic() {
        // Evaluators A=0, B=1, C=2. A and B report identically across 7 shared samples
        // (bg0..bg5 plus the target, sample index 6) with genuine variance (match on 4,
        // mismatch on 3) -> phi(A,B) = 1.0 exactly. C only co-occurs with A/B on the target
        // sample (1 shared sample each, < MIN_SHARED_SAMPLES_FOR_CORRELATION) -> phi(A,C) =
        // phi(B,C) = 0.0 (defaulted). On the target sample (n=3): rho = (1.0+0.0+0.0)/3 =
        // 1/3, DEFF = 1 + 2*(1/3) = 5/3, effective_n = 3/(5/3) = 1.8 exactly. 1.8 < 0.7*3=2.1
        // -> warning fires.
        let samples = vec![
            vec![(0, 0), (1, 0)],         // bg0: final_label 0, A/B both match
            vec![(0, 0), (1, 0)],         // bg1
            vec![(0, 0), (1, 0)],         // bg2
            vec![(0, 0), (1, 0)],         // bg3: final_label 1, A/B both mismatch
            vec![(0, 0), (1, 0)],         // bg4
            vec![(0, 0), (1, 0)],         // bg5
            vec![(0, 0), (1, 0), (2, 0)], // target: A, B, C all present
        ];
        let pi = vec![
            vec![1.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 0.0],
            vec![0.0, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 1.0],
            vec![1.0, 0.0],
        ];
        let result = evaluator_effective_n_diagnostics(3, &samples, &pi);
        let (effective_n, warning) = result[6];
        assert!((effective_n - 1.8).abs() < 1e-9, "{effective_n}");
        assert!(warning);
    }

    #[test]
    fn test_correlated_evaluators_lower_effective_n_and_warn() {
        // 3 reliable evaluators (R1-R3, always correct) + 2 evaluators (TW1, TW2) who are
        // identical to each other: correct on the first half of training samples, both wrong
        // (same wrong label) on the second half. The reliable trio anchors EM's converged
        // label regardless of the twins (3-vs-2 majority throughout), so TW1/TW2's match
        // indicator gets genuine variance from the reliable trio "revealing" their deviation
        // on half the samples -- exactly the condition needed to detect their correlation.
        let mut obs = Vec::new();
        let training = [
            ("t1", "win"),
            ("t2", "loss"),
            ("t3", "win"),
            ("t4", "loss"),
            ("t5", "win"),
            ("t6", "loss"),
        ];
        for (i, (id, truth)) in training.iter().enumerate() {
            let opposite = if *truth == "win" { "loss" } else { "win" };
            for eval in ["R1", "R2", "R3"] {
                obs.push(Observation {
                    sample_id: (*id).into(),
                    label: Some((*truth).to_string()),
                    evaluator_id: Some(eval.into()),
                    ..Default::default()
                });
            }
            // Twins agree with truth on the first half, both wrong together on the second.
            let twin_label = if i < 3 { *truth } else { opposite };
            for eval in ["TW1", "TW2"] {
                obs.push(Observation {
                    sample_id: (*id).into(),
                    label: Some(twin_label.to_string()),
                    evaluator_id: Some(eval.into()),
                    ..Default::default()
                });
            }
        }
        // Target sample: one reliable evaluator + both twins (n=3 qualifying evaluators).
        for (eval, label) in [("R1", "win"), ("TW1", "loss"), ("TW2", "loss")] {
            obs.push(Observation {
                sample_id: "s".into(),
                label: Some(label.into()),
                evaluator_id: Some(eval.into()),
                ..Default::default()
            });
        }

        let estimates = compute_latent_truth(&obs).expect("j=5, k=2 should run EM");
        let s = &estimates["s"];
        assert!(
            s.evaluator_effective_n < 3.0 * 0.9,
            "correlated twins should measurably lower effective_n below nominal 3, got {}",
            s.evaluator_effective_n
        );
        assert!(
            s.correlated_evaluator_warning,
            "effective_n {} should trigger the warning",
            s.evaluator_effective_n
        );
    }

    #[test]
    fn test_correlated_twins_blind_spot_when_uncontested() {
        // TW1 and TW2 are the ONLY evaluators in the whole dataset, always identical to each
        // other. With no independent evaluator ever present to reveal their shared deviation,
        // EM's converged label always equals whatever the twins agree on (they define the
        // majority themselves) -- so their match indicator has zero variance, phi defaults to
        // 0.0, and the warning does NOT fire. This is the documented blind spot, not a bug:
        // the diagnostic can only detect correlation when something independent exposes it.
        let mut obs = Vec::new();
        for (id, label) in [("a", "win"), ("b", "loss"), ("c", "win"), ("d", "loss")] {
            for eval in ["TW1", "TW2"] {
                obs.push(Observation {
                    sample_id: id.into(),
                    label: Some(label.into()),
                    evaluator_id: Some(eval.into()),
                    ..Default::default()
                });
            }
        }
        let estimates = compute_latent_truth(&obs).expect("j=2, k=2 should run EM");
        let a = &estimates["a"];
        assert!(
            (a.evaluator_effective_n - 2.0).abs() < 1e-9,
            "uncontested twins should read as fully independent (effective_n == nominal 2), \
             got {} -- this is the documented blind spot",
            a.evaluator_effective_n
        );
        assert!(!a.correlated_evaluator_warning);
    }

    #[test]
    fn test_compute_latent_truth_converges_quickly_on_clean_data() {
        // 8 samples, 3 evaluators each, all agreeing -> EM should settle in very few iterations.
        let mut observations = Vec::new();
        for s in 0..8 {
            let sample_id = format!("s{s}");
            let true_label = if s % 2 == 0 { "a" } else { "b" };
            for e in 0..3 {
                observations.push(Observation {
                    sample_id: sample_id.clone(),
                    label: Some(true_label.to_string()),
                    evaluator_id: Some(format!("eval{e}")),
                    ..Default::default()
                });
            }
        }
        let result = compute_latent_truth(&observations).expect("k=2, j=3 should run EM");
        for estimate in result.values() {
            assert!(
                estimate.converged,
                "clean, fully-agreeing data should converge"
            );
            assert!(
                estimate.iterations <= 5,
                "clean data should settle in very few iterations, got {}",
                estimate.iterations
            );
            assert!(estimate.convergence_delta < CONVERGENCE_EPSILON);
        }
    }

    #[test]
    fn test_compute_latent_truth_adversarial_tie_hits_iteration_cap() {
        // Same fixture as test_compute_latent_truth_terminates_on_adversarial_tie: it's
        // constructed so no sample ever settles, so this locks in that it hits MAX_ITERATIONS
        // without converging, rather than silently returning a low-confidence posterior with
        // no way to tell it never settled.
        let mut observations = Vec::new();
        for s in 0..6 {
            let sample_id = format!("s{s}");
            for e in 0..4 {
                let label = if (s + e) % 2 == 0 { "a" } else { "b" };
                observations.push(Observation {
                    sample_id: sample_id.clone(),
                    label: Some(label.to_string()),
                    evaluator_id: Some(format!("eval{e}")),
                    ..Default::default()
                });
            }
        }
        let result = compute_latent_truth(&observations).expect("k=2, j=4 should run EM");
        for estimate in result.values() {
            assert!(!estimate.converged, "adversarial tie should never settle");
            assert_eq!(estimate.iterations, MAX_ITERATIONS);
            assert!(estimate.convergence_delta >= CONVERGENCE_EPSILON);
        }
    }
}
