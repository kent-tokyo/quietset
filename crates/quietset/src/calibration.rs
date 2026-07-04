use crate::config::{DecisionScore, ScoreConfig};
use crate::observation::Observation;
use crate::schema::StabilityReport;
use crate::score_all_gold_weighted;
use crate::score_all_latent_truth;
use crate::scoring::score_all;

/// Above this absolute gap between in-sample (training) precision and held-out precision at
/// the same threshold, `note` flags the selection as likely overfit. Below it, the gap is
/// treated as ordinary sampling noise and no note is attached.
const TRAIN_HELDOUT_GAP_WARNING_THRESHOLD: f64 = 0.05;

/// Result of threshold calibration against gold labels.
///
/// `achieved_precision`/`precision_ci_low`/`precision_ci_high`/`coverage`/`n_keep`/`n_total`
/// describe `observations` unless a `heldout` set was supplied to [`compute_calibration`], in
/// which case they describe `heldout` instead — measured at the same `keep_threshold`, which
/// is always selected from `observations` regardless.
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
    /// In-sample precision at `keep_threshold`, measured on `observations` (the set the
    /// threshold was selected from). `Some` only when `heldout` was supplied — otherwise it
    /// would be identical to `achieved_precision` and is omitted as redundant. Lets a caller
    /// see the training-vs-held-out gap directly instead of only through `note`'s prose.
    pub train_precision: Option<f64>,
    /// Machine-readable caveat about how trustworthy `achieved_precision` is. `Some` when no
    /// `heldout` was given (the threshold was selected and measured on the same gold labels —
    /// always optimistic to some degree) or when `heldout` was given but its precision fell
    /// more than [`TRAIN_HELDOUT_GAP_WARNING_THRESHOLD`] below the in-sample precision at the
    /// same threshold (the selection likely overfit). `None` when `heldout` was given and the
    /// gap was small enough to be ordinary sampling noise.
    pub note: Option<String>,
}

/// Grid-search keep_threshold (0.99 down to 0.50, step 0.01) to meet a precision or coverage target.
/// Uses gold_label from observations. Returns None when no gold_label is present or target unmet.
///
/// `heldout`, if given, does not affect threshold *selection* (still grid-searched against
/// `observations` only) but replaces what's *measured*: once a threshold is found,
/// `achieved_precision` and friends are computed from `heldout`'s own `gold_label`s at that
/// threshold instead of from `observations`. This is what makes the result a non-circular
/// estimate — the threshold was picked on `observations`, so measuring precision on the same
/// data would be optimistic (see `tasks/lessons.md`). The returned precision can legitimately
/// land below `target_precision` when the `observations`-selected threshold overfits; that's
/// the correct, expected signal, not a bug. Returns `None` if `heldout` is `Some` but has no
/// `gold_label` at all, mirroring the same convention for `observations`.
pub fn compute_calibration(
    observations: &[Observation],
    decision_score: &DecisionScore,
    confidence_level: f64,
    target_precision: f64,
    target_coverage: Option<f64>,
    drop_threshold: f64,
    heldout: Option<&[Observation]>,
) -> Option<CalibrationResult> {
    let gold: std::collections::HashMap<&str, &str> = observations
        .iter()
        .filter_map(|o| o.gold_label.as_deref().map(|g| (o.sample_id.as_str(), g)))
        .collect();
    if gold.is_empty() {
        return None;
    }
    let heldout_gold: Option<std::collections::HashMap<&str, &str>> = match heldout {
        Some(ho) => {
            let m: std::collections::HashMap<&str, &str> = ho
                .iter()
                .filter_map(|o| o.gold_label.as_deref().map(|g| (o.sample_id.as_str(), g)))
                .collect();
            if m.is_empty() {
                return None;
            }
            Some(m)
        }
        None => None,
    };

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
            if let (Some(ho), Some(ho_gold)) = (heldout, &heldout_gold) {
                let ho_reports = match decision_score {
                    DecisionScore::GoldWeighted => score_all_gold_weighted(ho.to_vec(), &config),
                    DecisionScore::LatentTruth => score_all_latent_truth(ho.to_vec(), &config),
                    _ => score_all(ho.to_vec(), &config),
                };
                let ho_n_total = ho_reports.len();
                let ho_kept: Vec<&StabilityReport> =
                    ho_reports.iter().filter(|r| score_val(r) >= t).collect();
                let ho_n_keep = ho_kept.len();
                let ho_coverage = if ho_n_total == 0 {
                    0.0
                } else {
                    ho_n_keep as f64 / ho_n_total as f64
                };
                let ho_matches = ho_kept
                    .iter()
                    .filter(|r| {
                        let label = r
                            .weighted_majority_label
                            .as_deref()
                            .or(r.latent_truth_label.as_deref())
                            .or(r.majority_label.as_deref());
                        ho_gold
                            .get(r.sample_id.as_str())
                            .and_then(|&g| label.map(|m| m == g))
                            .unwrap_or(false)
                    })
                    .count();
                let ho_precision = if ho_n_keep == 0 {
                    0.0
                } else {
                    ho_matches as f64 / ho_n_keep as f64
                };
                let (precision_ci_low, precision_ci_high) =
                    crate::scoring::wilson_ci(ho_matches, ho_n_keep, confidence_level);
                let gap = precision - ho_precision;
                let note = if gap > TRAIN_HELDOUT_GAP_WARNING_THRESHOLD {
                    Some(format!(
                        "in-sample precision at this threshold was {precision:.3}, held-out \
                         precision is {ho_precision:.3} (gap {gap:.3}) — the training-selected \
                         threshold likely overfit the training gold labels"
                    ))
                } else {
                    None
                };
                return Some(CalibrationResult {
                    keep_threshold: t,
                    drop_threshold,
                    decision_score_name,
                    achieved_precision: ho_precision,
                    precision_ci_low,
                    precision_ci_high,
                    coverage: ho_coverage,
                    n_keep: ho_n_keep,
                    n_total: ho_n_total,
                    train_precision: Some(precision),
                    note,
                });
            }
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
                train_precision: None,
                note: Some(
                    "achieved_precision and its confidence interval were selected and measured \
                     on the same gold-labeled data; the threshold search makes this optimistic. \
                     Pass --heldout <path> for an independent estimate."
                        .to_string(),
                ),
            });
        }
    }
    None
}
