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
    /// Mean of all numeric scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_mean: Option<f64>,
    /// Population standard deviation of numeric scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_std: Option<f64>,
    /// Difference between the maximum and minimum score (`max - min`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_range: Option<f64>,
    /// Normalized range of per-budget mean scores; `None` if fewer than two budget levels. (`[0.0, 1.0]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_sensitivity: Option<f64>,
    /// Normalized range of per-seed mean scores; `None` if fewer than two seeds. (`[0.0, 1.0]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_sensitivity: Option<f64>,
    /// Fraction of models whose majority label matches the overall majority; `None` if fewer than two models. (`[0.0, 1.0]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_agreement: Option<f64>,
    /// Fraction of evaluators whose majority label matches the overall majority; `None` if fewer than two evaluators. (`[0.0, 1.0]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluator_agreement: Option<f64>,
    /// `1.0 - stability_score`.
    pub disagreement_score: f64,
    /// Overall stability score in `[0.0, 1.0]`. Higher is more stable.
    pub stability_score: f64,
    /// Filtering decision derived from `stability_score` and the configured thresholds.
    pub decision: Decision,
}
