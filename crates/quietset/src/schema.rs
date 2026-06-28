use serde::{Deserialize, Serialize};

/// The filtering decision for a sample based on its stability score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Stable sample — include in the filtered dataset.
    Keep,
    /// Borderline sample — manual review recommended.
    Review,
    /// Unstable sample — exclude from the filtered dataset.
    Drop,
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Decision::Keep => write!(f, "keep"),
            Decision::Review => write!(f, "review"),
            Decision::Drop => write!(f, "drop"),
        }
    }
}

/// Per-dimension sub-scores that contributed to `stability_score`.
///
/// Each value is in `[0.0, 1.0]` where `1.0` is fully stable.
/// Fields are omitted when the dimension was not computable (e.g. no labels, no budgets).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StabilityComponents {
    /// Label agreement across observations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<f64>,
    /// Score consistency (`1 - normalized_score_std`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_consistency: Option<f64>,
    /// Budget robustness (`1 - budget_sensitivity`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_robustness: Option<f64>,
    /// Seed robustness (`1 - seed_sensitivity`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_robustness: Option<f64>,
    /// Label agreement across models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_agreement: Option<f64>,
    /// Label agreement across evaluators.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluator_agreement: Option<f64>,
}

impl StabilityComponents {
    /// Returns the name and value of the weakest (lowest) component, if any exist.
    ///
    /// Ties are resolved by the fixed component declaration order:
    /// `label` → `score_consistency` → `budget_robustness` → `seed_robustness`
    /// → `model_agreement` → `evaluator_agreement`.
    pub fn weakest(&self) -> Option<(&'static str, f64)> {
        let candidates = [
            ("label", self.label),
            ("score_consistency", self.score_consistency),
            ("budget_robustness", self.budget_robustness),
            ("seed_robustness", self.seed_robustness),
            ("model_agreement", self.model_agreement),
            ("evaluator_agreement", self.evaluator_agreement),
        ];
        candidates
            .into_iter()
            .filter_map(|(name, val)| val.map(|v| (name, v)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }
}

/// Stability report for one sample, aggregated across all its observations.
///
/// All `Option` fields are omitted from JSON output when absent so that
/// downstream tools only see metrics that were actually computable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityReport {
    /// The sample this report describes.
    pub sample_id: String,
    /// Number of observations used to compute this report.
    pub n_observations: usize,
    /// The label that appeared most often across observations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub majority_label: Option<String>,
    /// Fraction of observations that carry the majority label (`[0.0, 1.0]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_agreement: Option<f64>,
    /// Wilson confidence interval lower bound of label_agreement at `confidence_level`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_agreement_lcb: Option<f64>,
    /// (majority_count - runner_up_count) / total_labels. 1.0 if only one label type. None if no labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_margin: Option<f64>,
    /// Normalized Shannon entropy of label distribution [0, 1]. 0.0 = unanimous, 1.0 = uniform. None if no labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_entropy: Option<f64>,
    /// Mean of all numeric scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_mean: Option<f64>,
    /// Population standard deviation of numeric scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_std: Option<f64>,
    /// Difference between the maximum and minimum score (`max - min`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_range: Option<f64>,
    /// Median absolute deviation of numeric scores. More robust to outliers than score_std.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_mad: Option<f64>,
    /// Interquartile range of numeric scores (Q3 - Q1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_iqr: Option<f64>,
    /// Normalized range of per-budget mean scores; `None` if fewer than two budget levels. (`[0.0, 1.0]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_sensitivity: Option<f64>,
    /// Slope of (budget, mean_score) trend. None if < 2 budget levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_slope: Option<f64>,
    /// Normalized range of per-seed mean scores; `None` if fewer than two seeds. (`[0.0, 1.0]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_sensitivity: Option<f64>,
    /// Fraction of models whose majority label matches the overall majority; `None` if fewer than two models. (`[0.0, 1.0]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_agreement: Option<f64>,
    /// Fraction of evaluators whose majority label matches the overall majority; `None` if fewer than two evaluators. (`[0.0, 1.0]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluator_agreement: Option<f64>,
    /// Reliability confidence based on observation count: n / (n + confidence_k). In [0, 1].
    pub confidence: f64,
    /// stability_score adjusted for confidence: stability_score * confidence + 0.5 * (1 - confidence)
    pub adjusted_stability_score: f64,
    /// `1.0 - stability_score`.
    pub disagreement_score: f64,
    /// Overall stability score in `[0.0, 1.0]`. Higher is more stable.
    pub stability_score: f64,
    /// Filtering decision derived from `stability_score` and the configured thresholds.
    pub decision: Decision,
    /// Per-dimension sub-scores that contributed to `stability_score`.
    pub components: StabilityComponents,
}
