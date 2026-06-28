use crate::decision::{Thresholds, decide};
use crate::observation::Observation;
use crate::schema::{StabilityComponents, StabilityReport};
use ordered_float::OrderedFloat;
use std::collections::HashMap;

/// Which score value drives the keep / review / drop decision.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DecisionScore {
    /// Use raw `stability_score` (default).
    #[default]
    Raw,
    /// Use `adjusted_stability_score` (confidence-penalised). MinRequirements still applies afterwards.
    Adjusted,
    /// Use Wilson LCB of label_agreement for label component; confidence-adjusted for score.
    LowerConfidenceBound,
}

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
    /// Which score value drives the keep/review/drop threshold comparison.
    pub decision_score: DecisionScore,
    /// Confidence level for Wilson LCB. Default 0.95.
    pub confidence_level: f64,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            score_scale: 1.0,
            thresholds: Thresholds::default(),
            weights: ScoreWeights::default(),
            confidence_k: 3.0,
            min_requirements: MinRequirements::default(),
            decision_score: DecisionScore::Raw,
            confidence_level: 0.95,
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
        if !self.confidence_level.is_finite() || !(0.0..=1.0).contains(&self.confidence_level) {
            return Err(crate::error::Error::InvalidThreshold(format!(
                "confidence_level ({}) must be in [0.0, 1.0]",
                self.confidence_level
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

fn percentile_of_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
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
    let (majority_label, label_agreement, label_margin, label_entropy, label_agreement_lcb) =
        if labels.is_empty() {
            (None, None, None, None, None)
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
            (
                Some(majority.to_string()),
                agreement,
                margin,
                entropy,
                label_agreement_lcb,
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
        label_agreement_lcb,
        label_margin,
        label_entropy,
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

/// Fleiss' kappa: inter-rater agreement corrected for chance (nominal labels, variable raters).
///
/// Returns `None` when fewer than 2 subjects have at least 2 ratings each (undefined).
/// Range: −1.0 to 1.0 (1.0 = perfect agreement, 0.0 = chance level).
pub fn compute_fleiss_kappa(observations: &[Observation]) -> Option<f64> {
    // Build per-subject label counts
    let mut subjects: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for obs in observations {
        if let Some(label) = obs.label.as_deref() {
            *subjects
                .entry(obs.sample_id.as_str())
                .or_default()
                .entry(label)
                .or_insert(0) += 1;
        }
    }

    let mut total_per_cat: HashMap<&str, usize> = HashMap::new();
    let mut total_ratings = 0usize;
    let mut p_bar = 0.0f64;
    let mut valid = 0usize;

    for counts in subjects.values() {
        let n_i: usize = counts.values().sum();
        if n_i < 2 {
            continue;
        }
        let observed: f64 =
            counts.values().map(|&c| (c * (c - 1)) as f64).sum::<f64>() / (n_i * (n_i - 1)) as f64;
        p_bar += observed;
        valid += 1;
        for (&cat, &cnt) in counts {
            *total_per_cat.entry(cat).or_insert(0) += cnt;
            total_ratings += cnt;
        }
    }

    if valid < 2 || total_ratings == 0 {
        return None;
    }
    p_bar /= valid as f64;

    let p_e: f64 = total_per_cat
        .values()
        .map(|&c| (c as f64 / total_ratings as f64).powi(2))
        .sum();

    if (1.0 - p_e).abs() < 1e-10 {
        return Some(1.0);
    }
    Some((p_bar - p_e) / (1.0 - p_e))
}

/// Krippendorff's alpha: reliability coefficient for nominal labels, variable raters.
///
/// Uses the coincidence-matrix formulation. Returns `None` when there are fewer than
/// 2 units with at least 2 ratings (undefined).
/// Range: −1.0 to 1.0 (1.0 = perfect agreement, 0.0 = chance level).
pub fn compute_krippendorff_alpha(observations: &[Observation]) -> Option<f64> {
    let mut subjects: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for obs in observations {
        if let Some(label) = obs.label.as_deref() {
            *subjects
                .entry(obs.sample_id.as_str())
                .or_default()
                .entry(label)
                .or_insert(0) += 1;
        }
    }

    // Marginals: n_c[cat] = total count across subjects with n_k >= 2
    let mut n_c: HashMap<&str, usize> = HashMap::new();
    let mut e_diag = 0.0f64; // sum of coincidence-matrix diagonal
    let mut n_total = 0usize;
    let mut valid = 0usize;

    for counts in subjects.values() {
        let n_k: usize = counts.values().sum();
        if n_k < 2 {
            continue;
        }
        valid += 1;
        let denom = (n_k - 1) as f64;
        for (&cat, &cnt) in counts {
            e_diag += (cnt * (cnt - 1)) as f64 / denom;
            *n_c.entry(cat).or_insert(0) += cnt;
            n_total += cnt;
        }
    }

    if valid < 2 || n_total < 2 {
        return None;
    }
    let n = n_total as f64;

    // Observed disagreement (nominal: d=1 for c≠c')
    let do_ = 1.0 - e_diag / n;
    // Expected disagreement
    let sum_nc_sq: f64 = n_c.values().map(|&c| (c * c) as f64).sum();
    let de = (n * n - sum_nc_sq) / (n * (n - 1.0));

    if de.abs() < 1e-10 {
        return Some(1.0);
    }
    Some(1.0 - do_ / de)
}

/// Compute per-evaluator reliability: fraction of evaluations matching the sample's majority label.
///
/// **Experimental**: reliability is computed against the majority label from the initial scoring,
/// not a gold standard. Results are informational only.
///
/// If an observation has `gold_label` set, it takes priority over the majority label.
pub fn compute_evaluator_reliability(
    observations: &[Observation],
    reports: &[StabilityReport],
) -> std::collections::HashMap<String, f64> {
    use std::collections::HashMap;

    // gold_label takes priority over majority_label for ground truth
    let gold_map: HashMap<&str, &str> = observations
        .iter()
        .filter_map(|o| o.gold_label.as_deref().map(|g| (o.sample_id.as_str(), g)))
        .collect();
    let majority_map: HashMap<&str, &str> = reports
        .iter()
        .filter_map(|r| {
            r.majority_label
                .as_deref()
                .map(|m| (r.sample_id.as_str(), m))
        })
        .collect();

    let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
    for obs in observations {
        if let (Some(eval_id), Some(label)) = (obs.evaluator_id.as_deref(), obs.label.as_deref()) {
            let truth = gold_map
                .get(obs.sample_id.as_str())
                .copied()
                .or_else(|| majority_map.get(obs.sample_id.as_str()).copied());
            if let Some(truth) = truth {
                let entry = counts.entry(eval_id.to_string()).or_insert((0, 0));
                entry.1 += 1;
                if label == truth {
                    entry.0 += 1;
                }
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
