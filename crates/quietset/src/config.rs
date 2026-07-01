use crate::decision::Thresholds;

/// Which dispersion metric drives the `score_consistency` stability component.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ScoreDispersion {
    /// Standard deviation (default, backward-compatible).
    #[default]
    Std,
    /// Median absolute deviation — more robust to occasional score outliers.
    Mad,
    /// Interquartile range — more robust to occasional score outliers.
    Iqr,
}

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

/// Configuration for [`score_all`](crate::score_all) and [`compute_report`](crate::compute_report).
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
    /// Which dispersion metric drives the score_consistency component. Default: Std.
    pub score_dispersion: ScoreDispersion,
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
            score_dispersion: ScoreDispersion::Std,
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
