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

pub mod active_review;
pub mod agreement;
pub mod calibration;
pub mod config;
pub mod decision;
pub mod error;
pub mod group;
pub mod latent_truth;
pub mod observation;
pub mod schema;
pub mod scoring;
pub mod stable_wrong;
pub mod stream;
pub mod weighting;

pub use active_review::{
    ActiveReviewCosts, ActiveReviewEntry, ActiveReviewWeights, RankBy, rank_active_review,
    select_within_budget,
};
pub use agreement::{compute_fleiss_kappa, compute_krippendorff_alpha};
pub use calibration::{CalibrationResult, compute_calibration};
pub use config::{
    DecisionScore, LatentTruthSafety, MinRequirements, ScoreConfig, ScoreDispersion, ScoreWeights,
};
pub use decision::Thresholds;
pub use error::{Error, Result};
pub use latent_truth::{LatentTruthEstimate, compute_latent_truth};
pub use observation::{Observation, parse_csv, parse_jsonl};
pub use schema::{Decision, StabilityComponents, StabilityReport};
pub use scoring::{compute_report, score_all};
pub use stable_wrong::{
    BudgetStableWrongRate, EvaluatorStableWrongRate, ModelStableWrongRate, StableWrongBreakdown,
    stable_wrong_breakdown,
};
pub use stream::StreamingScorer;
pub use weighting::{
    compute_evaluator_label_weights, compute_evaluator_reliability, compute_evaluator_weights,
    compute_weighted_majority, compute_weighted_majority_by_label,
};

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
    let mut reports = scoring::score_all(observations.clone(), config);

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

    let evaluator_weights = weighting::compute_evaluator_weights(&observations, &truth);
    let groups = group::group_by_sample_id(observations.into_iter());

    // Pass 2: fill weighted_* fields
    for report in &mut reports {
        if let Some(obs) = groups.get(&report.sample_id) {
            let (wml, wlc, wld, conflict) = weighting::compute_weighted_majority(
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

/// Score all samples with the gold-label-reliability-weighted vote driving `stability_score`/
/// `decision` (2-pass). See [`DecisionScore::GoldWeighted`].
///
/// Pass 1: standard scoring to determine majority labels.
/// Pass 2: build a truth map from `gold_label` only (unlike [`score_all_weighted`], which also
/// falls back to `majority_label`), compute per-(evaluator, predicted label) reliability weights
/// from it (see [`compute_evaluator_label_weights`]), then re-score each sample with the
/// resulting weighted-label confidence baked into `stability_score`/`decision`. When no
/// `gold_label` is present anywhere in the batch, the truth map is empty and every evaluator
/// gets the neutral default weight, so the result is identical to [`score_all`] with
/// `DecisionScore::Raw`.
pub fn score_all_gold_weighted(
    observations: Vec<Observation>,
    config: &ScoreConfig,
) -> Vec<StabilityReport> {
    let mut reports = scoring::score_all(observations.clone(), config);

    let truth: HashMap<String, String> = observations
        .iter()
        .filter_map(|o| o.gold_label.clone().map(|g| (o.sample_id.clone(), g)))
        .collect();
    let label_weights = weighting::compute_evaluator_label_weights(&observations, &truth);
    let groups = group::group_by_sample_id(observations.into_iter());

    for report in &mut reports {
        if let Some(obs) = groups.get(&report.sample_id) {
            let (wml, wlc, wld, conflict) = weighting::compute_weighted_majority_by_label(
                obs,
                report.majority_label.as_deref(),
                &label_weights,
            );
            *report = scoring::compute_report_inner(&report.sample_id, obs, config, wlc);
            report.weighted_majority_label = wml;
            report.weighted_label_confidence = wlc;
            report.weighted_label_distribution = wld;
            report.majority_weighted_conflict = conflict;
        }
    }
    reports
}

/// Score all samples with the Dawid-Skene EM latent-truth vote driving `stability_score`/
/// `decision` (2-pass). See [`DecisionScore::LatentTruth`].
///
/// Pass 1: standard scoring to determine majority labels. Pass 2: run
/// [`latent_truth::compute_latent_truth`] over all observations (no `gold_label` involved),
/// then re-score each sample with the resulting posterior confidence baked into
/// `stability_score`/`decision`. When EM doesn't run at all (fewer than 2 distinct
/// evaluators or labels in the batch), every sample is left exactly as [`score_all`] with
/// `DecisionScore::Raw` produced it.
pub fn score_all_latent_truth(
    observations: Vec<Observation>,
    config: &ScoreConfig,
) -> Vec<StabilityReport> {
    let mut reports = scoring::score_all(observations.clone(), config);

    let Some(estimates) = latent_truth::compute_latent_truth(&observations) else {
        return reports;
    };
    let groups = group::group_by_sample_id(observations.into_iter());

    for report in &mut reports {
        let Some(obs) = groups.get(&report.sample_id) else {
            continue;
        };
        if let Some(estimate) = estimates.get(&report.sample_id) {
            *report = scoring::compute_report_inner(
                &report.sample_id,
                obs,
                config,
                Some(estimate.confidence),
            );
            report.majority_latent_conflict = report
                .majority_label
                .as_deref()
                .map(|m| m != estimate.label.as_str());
            report.latent_truth_label = Some(estimate.label.clone());
            report.latent_truth_confidence = Some(estimate.confidence);
            report.latent_truth_label_distribution = Some(estimate.distribution.clone());
            report.evaluator_effective_n = Some(estimate.evaluator_effective_n);
            report.correlated_evaluator_warning = Some(estimate.correlated_evaluator_warning);
            report.latent_truth_converged = Some(estimate.converged);
            report.latent_truth_iterations = Some(estimate.iterations);
            report.latent_truth_convergence_delta = Some(estimate.convergence_delta);
            if report.decision == Decision::Keep
                && let Some(reason) = latent_truth::latent_truth_demotion_reason(
                    estimate,
                    &config.latent_truth_safety,
                )
            {
                report.decision = Decision::Review;
                report.latent_truth_demotion_reason = Some(reason.to_string());
            }
        }
    }
    reports
}
