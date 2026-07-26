use crate::config::{DecisionScore, ScoreConfig};
use crate::observation::Observation;
use crate::schema::StabilityReport;
use crate::score_all_gold_weighted;
use crate::score_all_latent_truth;
use crate::scoring::score_all;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Above this absolute gap between in-sample (training) precision and held-out precision at
/// the same threshold, `note` flags the selection as likely overfit. Below it, the gap is
/// treated as ordinary sampling noise and no note is attached.
const TRAIN_HELDOUT_GAP_WARNING_THRESHOLD: f64 = 0.05;

/// Which `Observation` field a calibration/held-out split is checked for leakage on. A closed
/// set rather than a free-form field name: `Observation` has no dynamic field lookup, and only
/// these fields carry the "samples in one group are not independent" contract that makes a
/// split across them a leak. Mirrors [`DecisionScore`] — a third grouping field is one new
/// variant plus one match arm, not a new abstraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKey {
    SourceRootId,
    OpeningFamily,
}

impl GroupKey {
    /// The `Observation` field name this key reads, so a saved result records what was checked.
    pub fn field_name(self) -> &'static str {
        match self {
            GroupKey::SourceRootId => "source_root_id",
            GroupKey::OpeningFamily => "opening_family",
        }
    }

    fn extract(self, o: &Observation) -> Option<&str> {
        match self {
            GroupKey::SourceRootId => o.source_root_id.as_deref(),
            GroupKey::OpeningFamily => o.opening_family.as_deref(),
        }
    }
}

/// Optional, additive settings for [`compute_calibration_with_options`]. Kept as its own struct
/// (rather than more positional parameters on `compute_calibration`) so a future addition here
/// never breaks either function's signature — see `compute_calibration`'s doc for why the plain
/// function still exists and never gains new parameters itself.
#[derive(Debug, Clone, Default)]
pub struct CalibrationOptions {
    /// When set together with a `heldout` set, checks the split for group-key leakage — see
    /// [`GroupLeakage`].
    pub group_by: Option<GroupKey>,
}

/// At most this many leaked group keys are embedded in [`GroupLeakage::leaked_keys_sample`].
/// `leaked_group_count` is always the exact total regardless of this cap — the cap only bounds
/// the size of the report itself (memory and `CalibrationResult`/CLI JSON output), so a dataset
/// with a very large number of leaked groups can't make an ordinary calibration run's output
/// grow without limit. Use `leaked_group_keys` directly if you need the complete, uncapped list.
const MAX_LEAKED_KEYS_SAMPLE: usize = 100;

/// How much a calibration/held-out split shares group keys, and how completely that could be
/// checked. A group key appearing on both sides means correlated samples (e.g. nearby positions
/// from one game record) were split across the boundary, so the held-out precision is partly
/// measuring data the threshold was already selected against, and reads higher than a genuinely
/// independent estimate would.
///
/// This is a report, not a repair: quietset does not move observations between the two sets.
/// Every count here is a **row** count (raw `Observation`s passed in), not a `sample_id`/report
/// count — `calibration`/`heldout` are the same raw slices `compute_calibration` grid-searches
/// and measures against.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupLeakage {
    /// The `Observation` field checked (`"source_root_id"` / `"opening_family"`).
    pub group_field: &'static str,
    /// Rows in the calibration (primary) input.
    pub train_rows: usize,
    /// Rows in the held-out file.
    pub heldout_rows: usize,
    /// Rows (both sides combined) that carry a non-null `group_field` value.
    pub rows_with_group: usize,
    /// Rows (both sides combined) with no `group_field` value at all. These can't be placed in
    /// either "leaked" or "clean" — see `coverage`. **Not folded into "no leak found"**: a
    /// `leaked_group_count` of `0` alongside a nonzero `rows_missing_group` means "leakage could
    /// only be checked on part of the data," not "the split is verified clean."
    pub rows_missing_group: usize,
    /// Distinct group keys on the calibration side (the primary input).
    pub train_unique_groups: usize,
    /// Distinct group keys on the held-out side.
    pub heldout_unique_groups: usize,
    /// Distinct group keys present on both sides — the leaked groups. `0` only means "no leak
    /// found among the rows that carry the field" — check `rows_missing_group`/`coverage` before
    /// reading it as "the split is clean." This is always the exact total, computed over every
    /// leaked group, regardless of how many are shown in `leaked_keys_sample` — never derive
    /// "how many leaked" from the sample's length.
    pub leaked_group_count: usize,
    /// Up to `MAX_LEAKED_KEYS_SAMPLE` leaked group keys, sorted, so the operator can find the
    /// records to move. Bounded so an ordinary calibration run's report size doesn't scale with
    /// the number of leaked groups — see `leaked_group_keys` for the complete, uncapped list.
    pub leaked_keys_sample: Vec<String>,
    /// `true` when `leaked_group_count > leaked_keys_sample.len()` — i.e. `leaked_keys_sample`
    /// is a prefix, not the complete set.
    pub leaked_keys_truncated: bool,
    /// `rows_with_group / (train_rows + heldout_rows)`; `0.0` if both are empty. `1.0` means
    /// every row on both sides carried a value, so `leaked_group_count` reflects the whole
    /// dataset; anything less means the check is partial.
    pub coverage: f64,
}

/// Group keys present in both `calibration` and `heldout` for `key`, sorted, and each side's
/// full key set. Shared by [`group_leakage`] (which caps the leaked set for its embedded report)
/// and `leaked_group_keys` (which doesn't) so both compute the same intersection once.
fn group_key_sets<'a>(
    calibration: &'a [Observation],
    heldout: &'a [Observation],
    key: GroupKey,
) -> (BTreeSet<&'a str>, BTreeSet<&'a str>, BTreeSet<&'a str>) {
    let calibration_keys: BTreeSet<&str> =
        calibration.iter().filter_map(|o| key.extract(o)).collect();
    let heldout_keys: BTreeSet<&str> = heldout.iter().filter_map(|o| key.extract(o)).collect();
    let leaked: BTreeSet<&str> = calibration_keys
        .intersection(&heldout_keys)
        .copied()
        .collect();
    (calibration_keys, heldout_keys, leaked)
}

/// Every group key leaked between `calibration` and `heldout` for `key`, sorted, with no cap.
/// [`group_leakage`]'s embedded `GroupLeakage.leaked_keys_sample` truncates at
/// `MAX_LEAKED_KEYS_SAMPLE` to keep `CalibrationResult`/CLI output bounded regardless of
/// dataset size — call this directly if you need the complete list (e.g. to locate every
/// affected record).
pub fn leaked_group_keys(
    calibration: &[Observation],
    heldout: &[Observation],
    key: GroupKey,
) -> Vec<String> {
    let (_, _, leaked) = group_key_sets(calibration, heldout, key);
    leaked.into_iter().map(String::from).collect()
}

/// Distinct-group-key overlap between a calibration set and a held-out set. Pure function over
/// the two slices — [`compute_calibration_with_options`] calls this and attaches the result, but
/// it is public so a caller doing its own split can check it directly, and so it is
/// unit-testable without running a calibration.
pub fn group_leakage(
    calibration: &[Observation],
    heldout: &[Observation],
    key: GroupKey,
) -> GroupLeakage {
    let (calibration_keys, heldout_keys, leaked) = group_key_sets(calibration, heldout, key);

    let train_rows_missing = calibration
        .iter()
        .filter(|o| key.extract(o).is_none())
        .count();
    let heldout_rows_missing = heldout.iter().filter(|o| key.extract(o).is_none()).count();
    let rows_missing_group = train_rows_missing + heldout_rows_missing;
    let train_rows = calibration.len();
    let heldout_rows = heldout.len();
    let rows_with_group = (train_rows + heldout_rows) - rows_missing_group;

    let coverage = if train_rows + heldout_rows == 0 {
        0.0
    } else {
        rows_with_group as f64 / (train_rows + heldout_rows) as f64
    };

    let leaked_group_count = leaked.len();
    let leaked_keys_sample: Vec<String> = leaked
        .into_iter()
        .take(MAX_LEAKED_KEYS_SAMPLE)
        .map(String::from)
        .collect();
    let leaked_keys_truncated = leaked_group_count > leaked_keys_sample.len();

    GroupLeakage {
        group_field: key.field_name(),
        train_rows,
        heldout_rows,
        rows_with_group,
        rows_missing_group,
        train_unique_groups: calibration_keys.len(),
        heldout_unique_groups: heldout_keys.len(),
        leaked_group_count,
        leaked_keys_sample,
        leaked_keys_truncated,
        coverage,
    }
}

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
    /// always optimistic to some degree), when `heldout` was given but its precision fell more
    /// than [`TRAIN_HELDOUT_GAP_WARNING_THRESHOLD`] below the in-sample precision at the same
    /// threshold (the selection likely overfit), when `group_leakage` found a leaked group, or
    /// when `group_leakage`'s `coverage` is incomplete (the leakage check itself only covered
    /// part of the data). Sentences are joined with a space when more than one applies. `None`
    /// only when `heldout` was given, the gap was ordinary sampling noise, and either no
    /// `group_by` was given or the leakage check found nothing to flag on fully-covered data.
    pub note: Option<String>,
    /// Group-key overlap between `observations` and `heldout`. `Some` only when both a
    /// `group_by` key and a `heldout` set were supplied to [`compute_calibration_with_options`]
    /// — `None` means nothing was checked, which is distinct from "checked, nothing found"
    /// (`Some` with `leaked_group_count: 0`).
    pub group_leakage: Option<GroupLeakage>,
}

/// Grid-search keep_threshold (0.99 down to 0.50, step 0.01) to meet a precision or coverage
/// target. Uses gold_label from observations. Returns None when no gold_label is present or
/// target unmet.
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
///
/// This is the stable, original API — its parameter list never grows. A new, additive check
/// (like `--group-by` leakage detection) is a reason to add a field to [`CalibrationOptions`]
/// and call [`compute_calibration_with_options`] instead, not a reason to change this
/// signature and break every existing caller.
pub fn compute_calibration(
    observations: &[Observation],
    decision_score: &DecisionScore,
    confidence_level: f64,
    target_precision: f64,
    target_coverage: Option<f64>,
    drop_threshold: f64,
    heldout: Option<&[Observation]>,
) -> Option<CalibrationResult> {
    compute_calibration_with_options(
        observations,
        decision_score,
        confidence_level,
        target_precision,
        target_coverage,
        drop_threshold,
        heldout,
        &CalibrationOptions::default(),
    )
}

/// Same as [`compute_calibration`], plus `options` for additive, opt-in checks (currently just
/// `--group-by` leakage detection — see [`CalibrationOptions`]). `compute_calibration` itself is
/// exactly `compute_calibration_with_options(..., &CalibrationOptions::default())`.
#[allow(clippy::too_many_arguments)]
pub fn compute_calibration_with_options(
    observations: &[Observation],
    decision_score: &DecisionScore,
    confidence_level: f64,
    target_precision: f64,
    target_coverage: Option<f64>,
    drop_threshold: f64,
    heldout: Option<&[Observation]>,
    options: &CalibrationOptions,
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

    // Computed once, independent of the threshold grid search below.
    let leakage: Option<GroupLeakage> = match (heldout, options.group_by) {
        (Some(ho), Some(key)) => Some(group_leakage(observations, ho, key)),
        _ => None,
    };
    let leakage_note: Option<String> = leakage.as_ref().and_then(|gl| {
        let leak_sentence = (gl.leaked_group_count > 0).then(|| {
            format!(
                "{} group key(s) ({}) appear in both the calibration input and --heldout — those \
                 held-out samples are correlated with samples the threshold was selected on, so \
                 achieved_precision is optimistic.",
                gl.leaked_group_count, gl.group_field
            )
        });
        let coverage_sentence = (gl.rows_missing_group > 0).then(|| {
            format!(
                "{} of {} row(s) have no {} value ({:.1}% coverage) — leakage was checked only on \
                 the rows that do, so the true leak (if any) could be larger than reported.",
                gl.rows_missing_group,
                gl.train_rows + gl.heldout_rows,
                gl.group_field,
                gl.coverage * 100.0
            )
        });
        match (leak_sentence, coverage_sentence) {
            (Some(l), Some(c)) => Some(format!("{l} {c}")),
            (Some(l), None) => Some(l),
            (None, Some(c)) => Some(c),
            (None, None) => None,
        }
    });

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
                let overfit_note = (gap > TRAIN_HELDOUT_GAP_WARNING_THRESHOLD).then(|| {
                    format!(
                        "in-sample precision at this threshold was {precision:.3}, held-out \
                         precision is {ho_precision:.3} (gap {gap:.3}) — the training-selected \
                         threshold likely overfit the training gold labels"
                    )
                });
                let note = match (&leakage_note, &overfit_note) {
                    (Some(lk), Some(of)) => Some(format!("{lk} {of}")),
                    (Some(lk), None) => Some(lk.clone()),
                    (None, Some(of)) => Some(of.clone()),
                    (None, None) => None,
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
                    group_leakage: leakage,
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
                group_leakage: None,
            });
        }
    }
    None
}
