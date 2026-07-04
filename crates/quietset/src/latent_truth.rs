use crate::observation::Observation;
use indexmap::IndexMap;
use std::collections::{BTreeSet, HashMap};

const MAX_ITERATIONS: usize = 20;
const CONVERGENCE_EPSILON: f64 = 1e-4;
const SMOOTHING_ALPHA: f64 = 0.5;

/// EM-estimated latent truth for one sample: the most likely label, its posterior
/// confidence, and the full posterior distribution.
#[derive(Debug, Clone)]
pub struct LatentTruthEstimate {
    pub label: String,
    pub confidence: f64,
    pub distribution: IndexMap<String, f64>,
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

    for _ in 0..MAX_ITERATIONS {
        let new_pi = em_step(k, j, &samples, &pi);
        let delta: f64 = pi
            .iter()
            .zip(&new_pi)
            .map(|(old, new)| old.iter().zip(new).map(|(a, b)| (a - b).abs()).sum::<f64>())
            .sum();
        pi = new_pi;
        if delta < CONVERGENCE_EPSILON {
            // EM's log-likelihood is monotone non-decreasing every iteration, so if the cap
            // is hit without this ever firing, the last `pi` is still a valid (if perhaps
            // low-confidence) posterior — never worse than the majority-vote init.
            break;
        }
    }

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
        result.insert(
            sid,
            LatentTruthEstimate {
                label: labels_sorted[best].clone(),
                confidence: p[best],
                distribution,
            },
        );
    }
    Some(result)
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
}
