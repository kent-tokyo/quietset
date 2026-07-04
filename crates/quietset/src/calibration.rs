use crate::config::{DecisionScore, ScoreConfig};
use crate::observation::Observation;
use crate::schema::StabilityReport;
use crate::score_all_gold_weighted;
use crate::score_all_latent_truth;
use crate::scoring::score_all;

/// Result of threshold calibration against gold labels.
#[derive(Debug)]
pub struct CalibrationResult {
    pub keep_threshold: f64,
    pub drop_threshold: f64,
    pub decision_score_name: &'static str,
    pub achieved_precision: f64,
    /// Wilson score interval around `achieved_precision` at the given `confidence_level`.
    /// Widens automatically when few gold-labeled samples are kept.
    pub precision_ci_low: f64,
    pub precision_ci_high: f64,
    pub coverage: f64,
    pub n_keep: usize,
    pub n_total: usize,
}

/// Grid-search keep_threshold (0.99 down to 0.50, step 0.01) to meet a precision or coverage target.
/// Uses gold_label from observations. Returns None when no gold_label is present or target unmet.
pub fn compute_calibration(
    observations: &[Observation],
    decision_score: &DecisionScore,
    confidence_level: f64,
    target_precision: f64,
    target_coverage: Option<f64>,
    drop_threshold: f64,
) -> Option<CalibrationResult> {
    let gold: std::collections::HashMap<&str, &str> = observations
        .iter()
        .filter_map(|o| o.gold_label.as_deref().map(|g| (o.sample_id.as_str(), g)))
        .collect();
    if gold.is_empty() {
        return None;
    }

    // Score once with keep=0.0 to get all samples' scores regardless of threshold
    let config = ScoreConfig {
        thresholds: crate::decision::Thresholds {
            keep: 0.0,
            drop: -1.0,
        },
        decision_score: decision_score.clone(),
        confidence_level,
        ..ScoreConfig::default()
    };
    let reports = match decision_score {
        DecisionScore::GoldWeighted => score_all_gold_weighted(observations.to_vec(), &config),
        DecisionScore::LatentTruth => score_all_latent_truth(observations.to_vec(), &config),
        _ => score_all(observations.to_vec(), &config),
    };
    let n_total = reports.len();
    if n_total == 0 {
        return None;
    }

    let decision_score_name = match decision_score {
        DecisionScore::Raw => "raw",
        DecisionScore::Adjusted => "adjusted",
        DecisionScore::LowerConfidenceBound => "lcb",
        DecisionScore::GoldWeighted => "gold-weighted",
        DecisionScore::LatentTruth => "latent-truth",
    };

    // Try thresholds from 0.99 down to 0.50 — return the loosest that meets target
    for i in 0..=49usize {
        let t = 0.99 - i as f64 * 0.01;
        let score_val = |r: &StabilityReport| match decision_score {
            DecisionScore::Adjusted => r.adjusted_stability_score,
            _ => r.stability_score,
        };
        let kept: Vec<&StabilityReport> = reports.iter().filter(|r| score_val(r) >= t).collect();
        if kept.is_empty() {
            continue;
        }
        let n_keep = kept.len();
        let coverage = n_keep as f64 / n_total as f64;

        if let Some(tc) = target_coverage
            && coverage < tc
        {
            continue;
        }

        let matches = kept
            .iter()
            .filter(|r| {
                let label = r
                    .weighted_majority_label
                    .as_deref()
                    .or(r.latent_truth_label.as_deref())
                    .or(r.majority_label.as_deref());
                gold.get(r.sample_id.as_str())
                    .and_then(|&g| label.map(|m| m == g))
                    .unwrap_or(false)
            })
            .count();
        let precision = matches as f64 / n_keep as f64;

        if precision >= target_precision {
            let (precision_ci_low, precision_ci_high) =
                crate::scoring::wilson_ci(matches, n_keep, confidence_level);
            return Some(CalibrationResult {
                keep_threshold: t,
                drop_threshold,
                decision_score_name,
                achieved_precision: precision,
                precision_ci_low,
                precision_ci_high,
                coverage,
                n_keep,
                n_total,
            });
        }
    }
    None
}
