use crate::decision::{decide, Thresholds};
use crate::observation::Observation;
use crate::schema::StabilityReport;
use ordered_float::OrderedFloat;
use std::collections::HashMap;

/// Configuration for [`score_all`] and [`compute_report`].
pub struct ScoreConfig {
    /// Denominator for normalising `score_std` and sensitivity metrics into `[0.0, 1.0]`.
    /// Set to the expected range of your scores (e.g. `2.0` if scores span `[-1, 1]`). Default `1.0`.
    pub score_scale: f64,
    /// Thresholds that map `stability_score` to a [`crate::Decision`].
    pub thresholds: Thresholds,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            score_scale: 1.0,
            thresholds: Thresholds::default(),
        }
    }
}

/// Compute a [`StabilityReport`] for one sample from its observations.
pub fn compute_report(
    sample_id: &str,
    obs: &[Observation],
    config: &ScoreConfig,
) -> StabilityReport {
    let n = obs.len();

    // --- label stats ---
    let labels: Vec<&str> = obs.iter().filter_map(|o| o.label.as_deref()).collect();
    let (majority_label, label_agreement) = if labels.is_empty() {
        (None, None)
    } else {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for l in &labels {
            *counts.entry(l).or_insert(0) += 1;
        }
        let (majority, count) = counts.into_iter().max_by_key(|(_, c)| *c).unwrap();
        (
            Some(majority.to_string()),
            Some(count as f64 / labels.len() as f64),
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

    // --- sensitivity / agreement ---
    let budget_sensitivity = compute_range_sensitivity(
        obs,
        |o| o.budget.map(OrderedFloat),
        |o| o.score,
        config.score_scale,
    );
    let seed_sensitivity = compute_range_sensitivity(
        obs,
        |o| o.seed.map(|s| OrderedFloat(s as f64)),
        |o| o.score,
        config.score_scale,
    );
    let model_agreement = compute_group_label_agreement(obs, |o| o.model_id.as_deref());
    let evaluator_agreement = compute_group_label_agreement(obs, |o| o.evaluator_id.as_deref());

    // --- stability_score ---
    // ponytail: single obs = low-confidence, always review
    let stability_score = if n == 1 {
        0.5
    } else {
        let mut components: Vec<f64> = Vec::new();
        if let Some(la) = label_agreement {
            components.push(la);
        }
        if let Some(std) = score_std {
            components.push(1.0 - (std / config.score_scale).min(1.0));
        }
        if let Some(bs) = budget_sensitivity {
            components.push(1.0 - bs);
        }
        if let Some(ma) = model_agreement {
            components.push(ma);
        }
        if let Some(ea) = evaluator_agreement {
            components.push(ea);
        }
        if components.is_empty() {
            0.5
        } else {
            components.iter().sum::<f64>() / components.len() as f64
        }
    };

    let disagreement_score = 1.0 - stability_score;
    let decision = decide(stability_score, &config.thresholds);

    StabilityReport {
        sample_id: sample_id.to_string(),
        n_observations: n,
        majority_label,
        label_agreement,
        score_mean,
        score_std,
        score_range,
        budget_sensitivity,
        seed_sensitivity,
        model_agreement,
        evaluator_agreement,
        disagreement_score,
        stability_score,
        decision,
    }
}

/// Computes normalized range of per-group mean scores. Returns None if < 2 groups.
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
/// Returns None if < 2 distinct groups have labels.
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
    // majority label per group
    let majorities: Vec<&str> = group_labels
        .values()
        .map(|counts| counts.iter().max_by_key(|(_, c)| *c).unwrap().0)
        .copied()
        .collect();
    let mut global: HashMap<&str, usize> = HashMap::new();
    for l in &majorities {
        *global.entry(l).or_insert(0) += 1;
    }
    let max_count = global.values().max().copied().unwrap_or(0);
    Some(max_count as f64 / majorities.len() as f64)
}

/// Score all samples and return reports in sample_id insertion order.
pub fn score_all(observations: Vec<Observation>, config: &ScoreConfig) -> Vec<StabilityReport> {
    let groups = crate::group::group_by_sample_id(observations.into_iter());
    groups
        .iter()
        .map(|(id, obs)| compute_report(id, obs, config))
        .collect()
}
