use crate::config::{DecisionScore, ScoreConfig, ScoreDispersion};
use crate::decision::decide;
use crate::observation::Observation;
use crate::schema::{StabilityComponents, StabilityReport};
use indexmap::IndexMap;
use ordered_float::OrderedFloat;
use std::collections::HashMap;

fn normal_quantile(p: f64) -> f64 {
    let p = p.clamp(1e-10, 1.0 - 1e-10);
    let t = if p < 0.5 {
        (-2.0 * p.ln()).sqrt()
    } else {
        (-2.0 * (1.0 - p).ln()).sqrt()
    };
    let c = [2.515517_f64, 0.802853, 0.010328];
    let d = [1.432788_f64, 0.189269, 0.001308];
    let x =
        t - (c[0] + c[1] * t + c[2] * t * t) / (1.0 + d[0] * t + d[1] * t * t + d[2] * t * t * t);
    if p < 0.5 { -x } else { x }
}

fn wilson_lcb(successes: usize, trials: usize, confidence_level: f64) -> f64 {
    if trials == 0 {
        return 0.0;
    }
    let p = successes as f64 / trials as f64;
    let z = normal_quantile((1.0 + confidence_level) / 2.0);
    let n = trials as f64;
    let z2 = z * z;
    let num = p + z2 / (2.0 * n) - z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    (num / (1.0 + z2 / n)).clamp(0.0, 1.0)
}

/// Linear interpolation between order statistics (matches NumPy's/R's default "linear" method).
fn percentile_of_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = p * (sorted.len() - 1) as f64;
    let lower = pos.floor() as usize;
    let upper = pos.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let frac = pos - lower as f64;
        sorted[lower] + (sorted[upper] - sorted[lower]) * frac
    }
}

/// Compute a [`StabilityReport`] for one sample from its observations.
///
/// See the trust-boundary note on [`score_all`]: `obs` is expected to already be
/// validated (finite `score`/`budget`, non-empty `sample_id`); this is only checked
/// via `debug_assert!` here, not enforced in release builds.
pub fn compute_report(
    sample_id: &str,
    obs: &[Observation],
    config: &ScoreConfig,
) -> StabilityReport {
    debug_assert!(
        config.score_scale.is_finite() && config.score_scale > 0.0,
        "score_scale must be positive and finite"
    );
    for o in obs {
        debug_assert!(
            !o.sample_id.trim().is_empty(),
            "compute_report: empty sample_id (call Observation::validate() before scoring)"
        );
        debug_assert!(
            o.score.is_none_or(f64::is_finite),
            "compute_report: non-finite score (call Observation::validate() before scoring)"
        );
        debug_assert!(
            o.budget.is_none_or(f64::is_finite),
            "compute_report: non-finite budget (call Observation::validate() before scoring)"
        );
    }
    let n = obs.len();

    // --- label stats ---
    let labels: Vec<&str> = obs.iter().filter_map(|o| o.label.as_deref()).collect();
    let (
        majority_label,
        label_agreement,
        label_margin,
        label_entropy,
        label_agreement_lcb,
        label_distribution,
    ) = if labels.is_empty() {
        (None, None, None, None, None, None)
    } else {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for l in &labels {
            *counts.entry(l).or_insert(0) += 1;
        }
        // Sort: count desc, then label asc (deterministic tiebreak)
        let mut sorted: Vec<(&str, usize)> = counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

        let total = labels.len();
        let majority = sorted[0].0;
        let majority_count = sorted[0].1;

        let agreement = Some(majority_count as f64 / total as f64);
        let margin = if sorted.len() >= 2 {
            Some((majority_count - sorted[1].1) as f64 / total as f64)
        } else {
            Some(1.0) // only one label type
        };
        let entropy = {
            let h: f64 = sorted
                .iter()
                .map(|(_, c)| {
                    let p = *c as f64 / total as f64;
                    if p > 0.0 { -p * p.log2() } else { 0.0 }
                })
                .sum();
            let max_h = (sorted.len() as f64).log2().max(1.0);
            Some(if sorted.len() > 1 { h / max_h } else { 0.0 })
        };
        let label_agreement_lcb = Some(wilson_lcb(
            majority_count,
            labels.len(),
            config.confidence_level,
        ));
        let dist: IndexMap<String, f64> = sorted
            .iter()
            .map(|(l, c)| (l.to_string(), *c as f64 / total as f64))
            .collect();
        (
            Some(majority.to_string()),
            agreement,
            margin,
            entropy,
            label_agreement_lcb,
            Some(dist),
        )
    };

    // --- score stats ---
    let scores: Vec<f64> = obs.iter().filter_map(|o| o.score).collect();
    let (score_mean, score_std, score_range) = if scores.is_empty() {
        (None, None, None)
    } else {
        let mean = scores.iter().sum::<f64>() / scores.len() as f64;
        let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / scores.len() as f64;
        let std = variance.sqrt();
        let max = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = scores.iter().cloned().fold(f64::INFINITY, f64::min);
        (Some(mean), Some(std), Some(max - min))
    };

    let (score_mad, score_iqr) = if scores.len() >= 2 {
        let mut sorted_scores = scores.clone();
        sorted_scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = percentile_of_sorted(&sorted_scores, 0.5);
        let mut diffs: Vec<f64> = sorted_scores.iter().map(|s| (s - median).abs()).collect();
        diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mad = percentile_of_sorted(&diffs, 0.5);
        let q1 = percentile_of_sorted(&sorted_scores, 0.25);
        let q3 = percentile_of_sorted(&sorted_scores, 0.75);
        (Some(mad), Some(q3 - q1))
    } else {
        (None, None)
    };

    // --- sensitivity / agreement ---
    let budget_sensitivity = compute_range_sensitivity(
        obs,
        |o| o.budget.map(OrderedFloat),
        |o| o.score,
        config.score_scale,
    );
    let budget_slope = compute_budget_slope(obs);
    let seed_sensitivity = compute_range_sensitivity(
        obs,
        |o| o.seed.map(|s| OrderedFloat(s as f64)),
        |o| o.score,
        config.score_scale,
    );
    let model_agreement = compute_group_label_agreement(obs, |o| o.model_id.as_deref());
    let evaluator_agreement = compute_group_label_agreement(obs, |o| o.evaluator_id.as_deref());

    // --- stability_score (weighted mean of available sub-scores) ---
    // ponytail: single obs = low-confidence, always review
    let stability_score = if n == 1 {
        0.5
    } else {
        let w = &config.weights;
        let mut wsum = 0.0_f64;
        let mut wtotal = 0.0_f64;

        // For LCB mode, use label_agreement_lcb as the label component
        let label_for_score = match config.decision_score {
            DecisionScore::LowerConfidenceBound => label_agreement_lcb,
            _ => label_agreement,
        };

        if let Some(la) = label_for_score {
            wsum += la * w.label_agreement;
            wtotal += w.label_agreement;
        }
        let dispersion_val = match config.score_dispersion {
            ScoreDispersion::Std => score_std,
            ScoreDispersion::Mad => score_mad,
            ScoreDispersion::Iqr => score_iqr,
        };
        if let Some(d) = dispersion_val {
            wsum += (1.0 - (d / config.score_scale).min(1.0)) * w.score_stability;
            wtotal += w.score_stability;
        }
        if let Some(bs) = budget_sensitivity {
            wsum += (1.0 - bs) * w.budget_stability;
            wtotal += w.budget_stability;
        }
        if let Some(ss) = seed_sensitivity {
            wsum += (1.0 - ss) * w.seed_stability;
            wtotal += w.seed_stability;
        }
        if let Some(ma) = model_agreement {
            wsum += ma * w.model_agreement;
            wtotal += w.model_agreement;
        }
        if let Some(ea) = evaluator_agreement {
            wsum += ea * w.evaluator_agreement;
            wtotal += w.evaluator_agreement;
        }

        if wtotal > 0.0 { wsum / wtotal } else { 0.5 }
    };

    let confidence = n as f64 / (n as f64 + config.confidence_k);
    let adjusted_stability_score = stability_score * confidence + 0.5 * (1.0 - confidence);
    let disagreement_score = 1.0 - stability_score;

    // Choose which score drives the decision; MinRequirements is applied afterwards and cannot be overridden.
    let base_score = match config.decision_score {
        DecisionScore::Raw => stability_score,
        DecisionScore::Adjusted => adjusted_stability_score,
        DecisionScore::LowerConfidenceBound => stability_score,
    };
    let mut decision = decide(base_score, &config.thresholds);

    // If decided to Keep, check minimum requirements
    if decision == crate::schema::Decision::Keep {
        use std::collections::HashSet;
        let req = &config.min_requirements;
        if n < req.observations
            || obs
                .iter()
                .filter_map(|o| o.evaluator_id.as_deref())
                .collect::<HashSet<_>>()
                .len()
                < req.evaluators
            || obs
                .iter()
                .filter_map(|o| o.seed)
                .collect::<HashSet<_>>()
                .len()
                < req.seeds
            || obs
                .iter()
                .filter_map(|o| o.budget.map(OrderedFloat))
                .collect::<HashSet<_>>()
                .len()
                < req.budgets
            || obs
                .iter()
                .filter_map(|o| o.model_id.as_deref())
                .collect::<HashSet<_>>()
                .len()
                < req.models
        {
            decision = crate::schema::Decision::Review;
        }
    }

    let components = StabilityComponents {
        label: label_agreement,
        score_consistency: {
            let dv = match config.score_dispersion {
                ScoreDispersion::Std => score_std,
                ScoreDispersion::Mad => score_mad,
                ScoreDispersion::Iqr => score_iqr,
            };
            dv.map(|d| 1.0 - (d / config.score_scale).min(1.0))
        },
        budget_robustness: budget_sensitivity.map(|b| 1.0 - b),
        seed_robustness: seed_sensitivity.map(|s| 1.0 - s),
        model_agreement,
        evaluator_agreement,
    };

    StabilityReport {
        sample_id: sample_id.to_string(),
        n_observations: n,
        majority_label,
        label_agreement,
        label_agreement_lcb,
        label_margin,
        label_entropy,
        label_distribution,
        weighted_majority_label: None,
        weighted_label_confidence: None,
        weighted_label_distribution: None,
        majority_weighted_conflict: None,
        score_mean,
        score_std,
        score_range,
        score_mad,
        score_iqr,
        budget_sensitivity,
        budget_slope,
        seed_sensitivity,
        model_agreement,
        evaluator_agreement,
        confidence,
        adjusted_stability_score,
        disagreement_score,
        stability_score,
        decision,
        components,
    }
}

/// Computes normalized range of per-group mean scores. Returns `None` if < 2 groups.
fn compute_range_sensitivity<K, FK, FV>(
    obs: &[Observation],
    key_fn: FK,
    val_fn: FV,
    scale: f64,
) -> Option<f64>
where
    K: std::hash::Hash + Eq,
    FK: Fn(&Observation) -> Option<K>,
    FV: Fn(&Observation) -> Option<f64>,
{
    let mut groups: HashMap<K, Vec<f64>> = HashMap::new();
    for o in obs {
        if let (Some(k), Some(v)) = (key_fn(o), val_fn(o)) {
            groups.entry(k).or_default().push(v);
        }
    }
    if groups.len() < 2 {
        return None;
    }
    let means: Vec<f64> = groups
        .values()
        .map(|vs| vs.iter().sum::<f64>() / vs.len() as f64)
        .collect();
    let max = means.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min = means.iter().cloned().fold(f64::INFINITY, f64::min);
    Some(((max - min) / scale).min(1.0))
}

/// Computes label agreement across unique group values (model or evaluator).
/// Returns `None` if < 2 distinct groups have labels.
fn compute_group_label_agreement<'a, F>(obs: &'a [Observation], group_fn: F) -> Option<f64>
where
    F: Fn(&'a Observation) -> Option<&'a str>,
{
    let mut group_labels: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for o in obs {
        if let (Some(g), Some(l)) = (group_fn(o), o.label.as_deref()) {
            *group_labels.entry(g).or_default().entry(l).or_insert(0) += 1;
        }
    }
    if group_labels.len() < 2 {
        return None;
    }
    // Majority label per group; tiebreak: alphabetically first label wins.
    let majorities: Vec<&str> = group_labels
        .values()
        .map(|counts| {
            counts
                .iter()
                .max_by(|(l1, c1), (l2, c2)| c1.cmp(c2).then(l2.cmp(l1)))
                .unwrap()
                .0
        })
        .copied()
        .collect();
    let mut global: HashMap<&str, usize> = HashMap::new();
    for l in &majorities {
        *global.entry(l).or_insert(0) += 1;
    }
    let max_count = global.values().max().copied().unwrap_or(0);
    Some(max_count as f64 / majorities.len() as f64)
}

/// Score all samples and return reports in `sample_id` insertion order.
///
/// # Trust boundary
///
/// Callers are responsible for validating `observations` (non-empty `sample_id`,
/// finite `score`/`budget`) before calling this function — [`parse_jsonl`](crate::parse_jsonl)
/// and [`parse_csv`](crate::parse_csv) already do this via [`Observation::validate`]. Observations
/// built directly through the Rust API and passed here unvalidated are checked with
/// `debug_assert!` only (panics in debug builds, silently accepted in release builds).
pub fn score_all(observations: Vec<Observation>, config: &ScoreConfig) -> Vec<StabilityReport> {
    let groups = crate::group::group_by_sample_id(observations.into_iter());
    groups
        .iter()
        .map(|(id, obs)| compute_report(id, obs, config))
        .collect()
}

fn compute_budget_slope(obs: &[Observation]) -> Option<f64> {
    let mut budget_scores: std::collections::BTreeMap<OrderedFloat<f64>, Vec<f64>> =
        std::collections::BTreeMap::new();
    for o in obs {
        if let (Some(b), Some(s)) = (o.budget, o.score) {
            budget_scores.entry(OrderedFloat(b)).or_default().push(s);
        }
    }
    if budget_scores.len() < 2 {
        return None;
    }
    let pairs: Vec<(f64, f64)> = budget_scores
        .iter()
        .map(|(b, scores)| (b.0, scores.iter().sum::<f64>() / scores.len() as f64))
        .collect();
    // pairs are already sorted by BTreeMap key order
    let (b_first, s_first) = pairs[0];
    let (b_last, s_last) = pairs[pairs.len() - 1];
    let db = b_last - b_first;
    if db == 0.0 {
        return None;
    }
    Some((s_last - s_first) / db)
}
