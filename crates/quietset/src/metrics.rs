use crate::decision::{Thresholds, decide};
use crate::observation::Observation;
use crate::schema::{StabilityComponents, StabilityReport};
use ordered_float::OrderedFloat;
use std::collections::HashMap;

/// Minimum observation counts required for a Keep decision. Samples not meeting these are demoted to Review.
#[derive(Debug, Clone)]
pub struct MinRequirements {
    /// Minimum total observations. Default 1.
    pub observations: usize,
    /// Minimum distinct evaluator_ids required for Keep. Default 0 (no minimum).
    pub evaluators: usize,
    /// Minimum distinct seeds required for Keep. Default 0 (no minimum).
    pub seeds: usize,
    /// Minimum distinct budget levels required for Keep. Default 0 (no minimum).
    pub budgets: usize,
    /// Minimum distinct model_ids required for Keep. Default 0 (no minimum).
    pub models: usize,
}

impl Default for MinRequirements {
    fn default() -> Self {
        Self {
            observations: 1,
            evaluators: 0,
            seeds: 0,
            budgets: 0,
            models: 0,
        }
    }
}

/// Per-dimension weights for the `stability_score` weighted mean.
///
/// Set a weight to `0.0` to exclude that dimension from the score entirely.
/// All weights default to `1.0` (equal weighting).
#[derive(Debug, Clone)]
pub struct ScoreWeights {
    /// Weight for `label_agreement`.
    pub label_agreement: f64,
    /// Weight for score stability (`1 - normalized_score_std`).
    pub score_stability: f64,
    /// Weight for budget stability (`1 - budget_sensitivity`).
    pub budget_stability: f64,
    /// Weight for seed stability (`1 - seed_sensitivity`).
    pub seed_stability: f64,
    /// Weight for `model_agreement`.
    pub model_agreement: f64,
    /// Weight for `evaluator_agreement`.
    pub evaluator_agreement: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            label_agreement: 1.0,
            score_stability: 1.0,
            budget_stability: 1.0,
            seed_stability: 1.0,
            model_agreement: 1.0,
            evaluator_agreement: 1.0,
        }
    }
}

/// Configuration for [`score_all`] and [`compute_report`].
pub struct ScoreConfig {
    /// Denominator for normalising `score_std` and sensitivity metrics into `[0.0, 1.0]`.
    /// Set to the expected range of your scores (e.g. `2.0` if scores span `[-1, 1]`). Default `1.0`.
    pub score_scale: f64,
    /// Thresholds that map `stability_score` to a [`crate::Decision`].
    pub thresholds: Thresholds,
    /// Per-dimension weights for the `stability_score` weighted mean.
    pub weights: ScoreWeights,
    /// k parameter for confidence = n / (n + confidence_k). Default 3.0.
    pub confidence_k: f64,
    /// Minimum requirements for a Keep decision.
    pub min_requirements: MinRequirements,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            score_scale: 1.0,
            thresholds: Thresholds::default(),
            weights: ScoreWeights::default(),
            confidence_k: 3.0,
            min_requirements: MinRequirements::default(),
        }
    }
}

impl ScoreConfig {
    /// Validate this config, returning an error for invalid values.
    pub fn validate(&self) -> crate::error::Result<()> {
        if !self.score_scale.is_finite() || self.score_scale <= 0.0 {
            return Err(crate::error::Error::InvalidScoreScale(self.score_scale));
        }
        if !self.confidence_k.is_finite() || self.confidence_k < 0.0 {
            return Err(crate::error::Error::InvalidConfidenceK(self.confidence_k));
        }
        let t = &self.thresholds;
        if !t.keep.is_finite() || t.keep < 0.0 || t.keep > 1.0 {
            return Err(crate::error::Error::InvalidThreshold(format!(
                "keep_threshold ({}) must be in [0.0, 1.0]",
                t.keep
            )));
        }
        if !t.drop.is_finite() || t.drop < 0.0 || t.drop > 1.0 {
            return Err(crate::error::Error::InvalidThreshold(format!(
                "drop_threshold ({}) must be in [0.0, 1.0]",
                t.drop
            )));
        }
        if t.drop > t.keep {
            return Err(crate::error::Error::InvalidThreshold(format!(
                "drop_threshold ({}) must be <= keep_threshold ({})",
                t.drop, t.keep
            )));
        }
        let w = &self.weights;
        for (name, value) in [
            ("label_agreement", w.label_agreement),
            ("score_stability", w.score_stability),
            ("budget_stability", w.budget_stability),
            ("seed_stability", w.seed_stability),
            ("model_agreement", w.model_agreement),
            ("evaluator_agreement", w.evaluator_agreement),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(crate::error::Error::InvalidWeight { name, value });
            }
        }
        if w.label_agreement
            + w.score_stability
            + w.budget_stability
            + w.seed_stability
            + w.model_agreement
            + w.evaluator_agreement
            == 0.0
        {
            return Err(crate::error::Error::AllWeightsZero);
        }
        Ok(())
    }
}

/// Compute a [`StabilityReport`] for one sample from its observations.
pub fn compute_report(
    sample_id: &str,
    obs: &[Observation],
    config: &ScoreConfig,
) -> StabilityReport {
    debug_assert!(
        config.score_scale.is_finite() && config.score_scale > 0.0,
        "score_scale must be positive and finite"
    );
    let n = obs.len();

    // --- label stats ---
    let labels: Vec<&str> = obs.iter().filter_map(|o| o.label.as_deref()).collect();
    let (majority_label, label_agreement, label_margin, label_entropy) = if labels.is_empty() {
        (None, None, None, None)
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
        (Some(majority.to_string()), agreement, margin, entropy)
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

        if let Some(la) = label_agreement {
            wsum += la * w.label_agreement;
            wtotal += w.label_agreement;
        }
        if let Some(std) = score_std {
            wsum += (1.0 - (std / config.score_scale).min(1.0)) * w.score_stability;
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
    let mut decision = decide(stability_score, &config.thresholds);

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
        score_consistency: score_std.map(|s| 1.0 - (s / config.score_scale).min(1.0)),
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
        label_margin,
        label_entropy,
        score_mean,
        score_std,
        score_range,
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

/// Compute per-evaluator reliability: fraction of evaluations matching the sample's majority label.
///
/// **Experimental**: reliability is computed against the majority label from the initial scoring,
/// not a gold standard. Results are informational only.
pub fn compute_evaluator_reliability(
    observations: &[Observation],
    reports: &[StabilityReport],
) -> std::collections::HashMap<String, f64> {
    // Build sample_id -> majority_label map
    let majority_map: std::collections::HashMap<&str, &str> = reports
        .iter()
        .filter_map(|r| {
            r.majority_label
                .as_deref()
                .map(|m| (r.sample_id.as_str(), m))
        })
        .collect();

    // For each evaluator, count matches vs total
    let mut counts: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for obs in observations {
        if let (Some(eval_id), Some(label)) = (obs.evaluator_id.as_deref(), obs.label.as_deref())
            && let Some(&majority) = majority_map.get(obs.sample_id.as_str())
        {
            let entry = counts.entry(eval_id.to_string()).or_insert((0, 0));
            entry.1 += 1;
            if label == majority {
                entry.0 += 1;
            }
        }
    }

    counts
        .into_iter()
        .map(|(id, (matches, total))| {
            (
                id,
                if total > 0 {
                    matches as f64 / total as f64
                } else {
                    0.0
                },
            )
        })
        .collect()
}
