//! quietset — filter datasets by label stability.
//!
//! # Quick start
//!
//! ```rust
//! use quietset::{Observation, ScoreConfig, score_all};
//!
//! let obs = vec![
//!     Observation { sample_id: "a".into(), label: Some("win".into()), score: Some(0.9), ..Default::default() },
//!     Observation { sample_id: "a".into(), label: Some("win".into()), score: Some(0.88), ..Default::default() },
//! ];
//! let reports = score_all(obs, &ScoreConfig::default());
//! assert_eq!(reports[0].decision, quietset::Decision::Keep);
//! ```

pub mod decision;
pub mod error;
pub mod group;
pub mod metrics;
pub mod observation;
pub mod schema;
pub mod stream;

pub use decision::Thresholds;
pub use error::{Error, Result};
pub use metrics::{
    CalibrationResult, DecisionScore, MinRequirements, ScoreConfig, ScoreDispersion, ScoreWeights,
    compute_calibration, compute_evaluator_reliability, compute_evaluator_weights,
    compute_fleiss_kappa, compute_krippendorff_alpha, compute_report, compute_weighted_majority,
    score_all,
};
pub use observation::{Observation, parse_csv, parse_jsonl};
pub use schema::{Decision, StabilityComponents, StabilityReport};
pub use stream::StreamingScorer;

use std::collections::HashMap;

/// Score all samples with reliability-weighted majority voting (2-pass).
///
/// Pass 1: standard scoring to determine majority labels (or use gold_label if present).
/// Pass 2: compute per-evaluator reliability weights, then fill weighted_majority_label
/// and related fields on each report.
pub fn score_all_weighted(
    observations: Vec<Observation>,
    config: &ScoreConfig,
) -> Vec<StabilityReport> {
    // Pass 1: standard scoring
    let mut reports = metrics::score_all(observations.clone(), config);

    // Build truth map: gold_label takes priority over majority_label
    let majority_map: HashMap<String, String> = reports
        .iter()
        .filter_map(|r| r.majority_label.clone().map(|ml| (r.sample_id.clone(), ml)))
        .collect();
    let gold_map: HashMap<String, String> = observations
        .iter()
        .filter_map(|o| o.gold_label.clone().map(|g| (o.sample_id.clone(), g)))
        .collect();
    let truth: HashMap<String, String> = majority_map
        .into_iter()
        .map(|(id, ml)| {
            let label = gold_map.get(&id).cloned().unwrap_or(ml);
            (id, label)
        })
        .collect();

    let evaluator_weights = metrics::compute_evaluator_weights(&observations, &truth);
    let groups = group::group_by_sample_id(observations.into_iter());

    // Pass 2: fill weighted_* fields
    for report in &mut reports {
        if let Some(obs) = groups.get(&report.sample_id) {
            let (wml, wlc, wld, conflict) = metrics::compute_weighted_majority(
                obs,
                report.majority_label.as_deref(),
                &evaluator_weights,
            );
            report.weighted_majority_label = wml;
            report.weighted_label_confidence = wlc;
            report.weighted_label_distribution = wld;
            report.majority_weighted_conflict = conflict;
        }
    }
    reports
}
