use crate::decision::Thresholds;
use crate::observation::Observation;
use crate::schema::{Decision, StabilityReport};

/// Weights for the eight urgency signals. All default to `1.0`, matching each signal
/// contributing equally unless a caller opts out (weight `0.0`).
#[derive(Debug, Clone)]
pub struct ActiveReviewWeights {
    pub lcb: f64,
    pub entropy: f64,
    pub score_mad: f64,
    pub budget_sensitivity: f64,
    pub seed_sensitivity: f64,
    /// Order-dependency: weight for `shuffle_seed_sensitivity`.
    pub order_sensitivity: f64,
    /// Weight for `1 - teacher_residual_stability`.
    pub teacher_conflict: f64,
    /// Weight for `1 - gradient_sign_agreement`.
    pub gradient_instability: f64,
}

impl Default for ActiveReviewWeights {
    fn default() -> Self {
        Self {
            lcb: 1.0,
            entropy: 1.0,
            score_mad: 1.0,
            budget_sensitivity: 1.0,
            seed_sensitivity: 1.0,
            order_sensitivity: 1.0,
            teacher_conflict: 1.0,
            gradient_instability: 1.0,
        }
    }
}

/// Per-resource cost used to compute `utility = benefit / cost`. All default to `1.0`
/// (equal cost), so `utility` reduces to plain expected benefit unless a caller supplies
/// real relative costs. `evaluator` also backs the `add_model` action; `seed` also backs
/// `add_init_seed` (both are "run one more seed" remedies) and `gold_label` also backs
/// `flag_teacher_conflict` (both are "route to human review" remedies) — the original task
/// only named four cost dimensions for five actions, and this extension keeps that pattern
/// rather than adding a cost field per new action.
#[derive(Debug, Clone)]
pub struct ActiveReviewCosts {
    pub seed: f64,
    pub budget: f64,
    pub evaluator: f64,
    pub gold_label: f64,
    /// Cost of `add_shuffle_seed`.
    pub shuffle_seed: f64,
}

impl Default for ActiveReviewCosts {
    fn default() -> Self {
        Self {
            seed: 1.0,
            budget: 1.0,
            evaluator: 1.0,
            gold_label: 1.0,
            shuffle_seed: 1.0,
        }
    }
}

/// Sort order for [`rank_active_review`]'s output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RankBy {
    /// Sort by `urgency_score` descending (today's behavior).
    #[default]
    Urgency,
    /// Sort by `utility` descending — where extra review effort pays off most.
    Utility,
}

/// One sample's active-review entry: today's urgency heuristic plus expected-value fields.
#[derive(Debug, Clone)]
pub struct ActiveReviewEntry {
    pub sample_id: String,
    pub urgency_score: f64,
    pub primary_reason: &'static str,
    pub suggested_action: &'static str,
    pub label_agreement_lcb: Option<f64>,
    pub label_entropy: Option<f64>,
    pub budget_sensitivity: Option<f64>,
    pub seed_sensitivity: Option<f64>,
    /// Distance-to-threshold ratio in `[0.0, 1.0]`, `1.0` exactly at a keep/drop threshold and
    /// shrinking toward `0.0` further away. **Not a probability** — only `label_agreement`'s
    /// sampling uncertainty (the Wilson LCB gap) is modeled here; `stability_score`'s other
    /// components (score dispersion, budget/seed sensitivity, model/evaluator agreement) have
    /// no uncertainty model in this codebase, so this ratio under- or over-states true
    /// decision-flip risk whenever those other components are what's actually driving
    /// `stability_score`. `None` when the sample has no `label_agreement`/`label_agreement_lcb`.
    pub label_agreement_flip_ratio: Option<f64>,
    /// `1 / n_total` if this sample's `decision` (read verbatim from the input, not
    /// re-derived) is not `Keep`, else `0.0` — the exact coverage change from flipping one
    /// non-Keep sample to Keep.
    pub expected_coverage_gain: f64,
    /// `1 / n_keep` if this sample is a `Keep` whose `label_agreement_lcb` falls below the
    /// given keep threshold (an "at-risk keep", the same condition `stable-wrong-risk`'s
    /// `lcb_keep_demotions` already counts), else `0.0` — the exact stable-wrong-rate change
    /// from catching one such sample.
    pub expected_risk_reduction: f64,
    /// Cost of `suggested_action`, from [`ActiveReviewCosts`].
    pub cost: f64,
    /// `label_agreement_flip_ratio.unwrap_or(0.0) * (expected_coverage_gain +
    /// expected_risk_reduction) / cost`. A ranking score, not a literal expected-value
    /// calculation — ordering by it is sound (closer-to-threshold, higher-payoff, cheaper
    /// actions rank higher), but the absolute number should not be read as a calibrated
    /// expectation.
    pub utility: f64,
    /// Resource-sizing targets for `suggested_action`, from [`action_target_fields`]. `None`
    /// for every field unless the caller supplies raw observations (`--observations` on the
    /// CLI) — `StabilityReport` only carries derived stats, not the raw budget/seed/evaluator/
    /// model values needed to size a concrete target, the same gap `audit --observations`
    /// fills for agreement stats. Flattened onto the entry (not a nested struct) for JSON
    /// output simplicity, consistent with how `latent_truth_demotion_reason` sits directly on
    /// `StabilityReport`.
    pub target_budget: Option<f64>,
    pub target_seed: Option<u64>,
    pub target_shuffle_seed: Option<u64>,
    pub target_evaluator_slot: Option<usize>,
    pub target_model_slot: Option<usize>,
}

/// Distance-to-threshold ratio for `label_agreement`, given its Wilson LCB and the
/// keep/drop thresholds. `1.0` exactly at either threshold (`nearest == 0.0`), shrinking
/// toward `0.0` as `label_agreement` sits further from both thresholds relative to its own
/// margin of error. Guards the degenerate `margin == nearest == 0.0` case (both exactly zero)
/// by returning `0.0` rather than dividing by zero.
pub(crate) fn label_agreement_flip_ratio(
    label_agreement: f64,
    label_agreement_lcb: f64,
    keep_threshold: f64,
    drop_threshold: f64,
) -> f64 {
    let margin = label_agreement - label_agreement_lcb;
    let dist_keep = (keep_threshold - label_agreement).abs();
    let dist_drop = (drop_threshold - label_agreement).abs();
    let nearest = dist_keep.min(dist_drop);
    let denom = margin + nearest;
    if denom <= 0.0 { 0.0 } else { margin / denom }
}

/// Rank samples by how urgently they'd benefit from additional review, given already-scored
/// `reports`. `thresholds` are used only to compute `label_agreement_flip_ratio` and the
/// at-risk-keep gate for `expected_risk_reduction` — they do **not** re-derive `decision`,
/// which is read verbatim from each report (whatever thresholds `score` used originally).
/// `n_total`/`n_keep` for `expected_coverage_gain`/`expected_risk_reduction` are computed over
/// the full `reports` batch, before `unstable_only` filtering.
pub fn rank_active_review(
    reports: &[StabilityReport],
    weights: &ActiveReviewWeights,
    costs: &ActiveReviewCosts,
    thresholds: &Thresholds,
    unstable_only: bool,
    rank_by: RankBy,
) -> Vec<ActiveReviewEntry> {
    let n_total = reports.len();
    let n_keep = reports
        .iter()
        .filter(|r| r.decision == Decision::Keep)
        .count();

    let mut entries: Vec<ActiveReviewEntry> = Vec::new();
    for r in reports {
        if unstable_only && r.decision == Decision::Keep {
            continue;
        }

        let signals: &[(&'static str, f64, &'static str, f64)] = &[
            (
                "low_lcb",
                r.label_agreement_lcb
                    .map(|v| (1.0 - v) * weights.lcb)
                    .unwrap_or(0.0),
                "request_gold_label",
                costs.gold_label,
            ),
            (
                "high_entropy",
                r.label_entropy.map(|v| v * weights.entropy).unwrap_or(0.0),
                "add_evaluator",
                costs.evaluator,
            ),
            (
                "high_score_mad",
                r.score_mad
                    .map(|v| v.min(1.0) * weights.score_mad)
                    .unwrap_or(0.0),
                "add_model",
                costs.evaluator,
            ),
            (
                "high_budget_sensitivity",
                r.budget_sensitivity
                    .map(|v| v * weights.budget_sensitivity)
                    .unwrap_or(0.0),
                "increase_budget",
                costs.budget,
            ),
            (
                "high_seed_sensitivity",
                r.seed_sensitivity
                    .map(|v| v * weights.seed_sensitivity)
                    .unwrap_or(0.0),
                "add_seed",
                costs.seed,
            ),
            (
                "high_order_sensitivity",
                r.shuffle_seed_sensitivity
                    .map(|v| v * weights.order_sensitivity)
                    .unwrap_or(0.0),
                "add_shuffle_seed",
                costs.shuffle_seed,
            ),
            (
                "high_teacher_conflict",
                r.teacher_residual_stability
                    .map(|v| (1.0 - v) * weights.teacher_conflict)
                    .unwrap_or(0.0),
                "flag_teacher_conflict",
                costs.gold_label,
            ),
            (
                "high_gradient_instability",
                r.gradient_sign_agreement
                    .map(|v| (1.0 - v) * weights.gradient_instability)
                    .unwrap_or(0.0),
                "add_init_seed",
                costs.seed,
            ),
        ];

        let total_w: f64 = [
            r.label_agreement_lcb.map(|_| weights.lcb).unwrap_or(0.0),
            r.label_entropy.map(|_| weights.entropy).unwrap_or(0.0),
            r.score_mad.map(|_| weights.score_mad).unwrap_or(0.0),
            r.budget_sensitivity
                .map(|_| weights.budget_sensitivity)
                .unwrap_or(0.0),
            r.seed_sensitivity
                .map(|_| weights.seed_sensitivity)
                .unwrap_or(0.0),
            r.shuffle_seed_sensitivity
                .map(|_| weights.order_sensitivity)
                .unwrap_or(0.0),
            r.teacher_residual_stability
                .map(|_| weights.teacher_conflict)
                .unwrap_or(0.0),
            r.gradient_sign_agreement
                .map(|_| weights.gradient_instability)
                .unwrap_or(0.0),
        ]
        .iter()
        .sum();

        let raw_sum: f64 = signals.iter().map(|(_, v, _, _)| v).sum();
        let urgency = if total_w > 0.0 {
            raw_sum / total_w
        } else {
            0.0
        };

        let (primary_reason, _, suggested_action, cost) = *signals
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        let label_agreement_flip_ratio = match (r.label_agreement, r.label_agreement_lcb) {
            (Some(la), Some(lcb)) => Some(label_agreement_flip_ratio(
                la,
                lcb,
                thresholds.keep,
                thresholds.drop,
            )),
            _ => None,
        };

        let expected_coverage_gain = if r.decision != Decision::Keep {
            1.0 / n_total as f64
        } else {
            0.0
        };
        let is_at_risk_keep = r.decision == Decision::Keep
            && r.label_agreement_lcb.is_some_and(|v| v < thresholds.keep);
        let expected_risk_reduction = if is_at_risk_keep {
            1.0 / n_keep as f64
        } else {
            0.0
        };

        let utility = label_agreement_flip_ratio.unwrap_or(0.0)
            * (expected_coverage_gain + expected_risk_reduction)
            / cost.max(1e-9);

        entries.push(ActiveReviewEntry {
            sample_id: r.sample_id.clone(),
            urgency_score: urgency,
            primary_reason,
            suggested_action,
            label_agreement_lcb: r.label_agreement_lcb,
            label_entropy: r.label_entropy,
            budget_sensitivity: r.budget_sensitivity,
            seed_sensitivity: r.seed_sensitivity,
            label_agreement_flip_ratio,
            expected_coverage_gain,
            expected_risk_reduction,
            cost,
            utility,
            target_budget: None,
            target_seed: None,
            target_shuffle_seed: None,
            target_evaluator_slot: None,
            target_model_slot: None,
        });
    }

    match rank_by {
        RankBy::Urgency => entries.sort_by(|a, b| {
            b.urgency_score
                .partial_cmp(&a.urgency_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.sample_id.cmp(&b.sample_id))
        }),
        RankBy::Utility => entries.sort_by(|a, b| {
            b.utility
                .partial_cmp(&a.utility)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.sample_id.cmp(&b.sample_id))
        }),
    }
    entries
}

/// Greedily select entries by utility descending, within a total `budget`: entries are
/// scanned in utility order and taken whenever their `cost` still fits the remaining budget
/// (skipping — not stopping at — one that doesn't fit, so a cheaper lower-utility entry later
/// in the list can still be included). A standard greedy approximation for the knapsack-style
/// problem of maximizing total utility under a cost constraint; not guaranteed globally
/// optimal, but sound for ranking "what to spend review budget on next." Independent of
/// whatever order `entries` arrived in — always re-sorts by utility internally.
pub fn select_within_budget(entries: &[ActiveReviewEntry], budget: f64) -> Vec<ActiveReviewEntry> {
    let mut sorted: Vec<&ActiveReviewEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        b.utility
            .partial_cmp(&a.utility)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.sample_id.cmp(&b.sample_id))
    });
    let mut remaining = budget;
    let mut selected = Vec::new();
    for e in sorted {
        if e.cost <= remaining {
            remaining -= e.cost;
            selected.push(e.clone());
        }
    }
    selected
}

/// Computes [`ActiveReviewEntry`]'s `target_*` fields for one sample's `suggested_action`,
/// from that sample's own raw observations. Only the field matching `suggested_action` is
/// ever populated — `request_gold_label` gets none, since there's no resource count to size
/// for a label request. `None` (not a fabricated 0 or a guessed default) whenever the
/// relevant raw values are absent from `observations`, since a made-up number here would be
/// exactly the kind of unchecked hand-typed value this project's own history warns against.
pub fn action_target_fields(suggested_action: &str, observations: &[Observation]) -> ActionTargets {
    match suggested_action {
        "increase_budget" => ActionTargets {
            target_budget: observations
                .iter()
                .filter_map(|o| o.budget)
                .fold(None, |max: Option<f64>, b| {
                    Some(max.map_or(b, |m| m.max(b)))
                })
                .map(|max_budget| max_budget * 2.0),
            ..ActionTargets::default()
        },
        // add_init_seed reuses target_seed: both are "run one more `seed` value" remedies.
        "add_seed" | "add_init_seed" => ActionTargets {
            target_seed: observations
                .iter()
                .filter_map(|o| o.seed)
                .max()
                .map(|max_seed| max_seed + 1),
            ..ActionTargets::default()
        },
        "add_shuffle_seed" => ActionTargets {
            target_shuffle_seed: observations
                .iter()
                .filter_map(|o| o.shuffle_seed)
                .max()
                .map(|max_seed| max_seed + 1),
            ..ActionTargets::default()
        },
        "add_evaluator" => ActionTargets {
            target_evaluator_slot: distinct_count(
                observations
                    .iter()
                    .filter_map(|o| o.evaluator_id.as_deref()),
            ),
            ..ActionTargets::default()
        },
        "add_model" => ActionTargets {
            target_model_slot: distinct_count(
                observations.iter().filter_map(|o| o.model_id.as_deref()),
            ),
            ..ActionTargets::default()
        },
        _ => ActionTargets::default(),
    }
}

/// `Some(n + 1)` where `n` is the number of distinct values in `ids` — "this would be the
/// (n+1)th slot" — or `None` if `ids` is empty (no evidence to size a slot count from).
fn distinct_count<'a>(ids: impl Iterator<Item = &'a str>) -> Option<usize> {
    let n = ids.collect::<std::collections::HashSet<_>>().len();
    if n == 0 { None } else { Some(n + 1) }
}

/// Resource-sizing targets from [`action_target_fields`]. See [`ActiveReviewEntry`]'s
/// `target_*` fields for what each one means.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActionTargets {
    pub target_budget: Option<f64>,
    pub target_seed: Option<u64>,
    pub target_shuffle_seed: Option<u64>,
    pub target_evaluator_slot: Option<usize>,
    pub target_model_slot: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ScoreConfig;
    use crate::observation::Observation;
    use crate::scoring::score_all;

    fn zeroed_weights_low_lcb_only() -> ActiveReviewWeights {
        ActiveReviewWeights {
            lcb: 1.0,
            entropy: 0.0,
            score_mad: 0.0,
            budget_sensitivity: 0.0,
            seed_sensitivity: 0.0,
            order_sensitivity: 0.0,
            teacher_conflict: 0.0,
            gradient_instability: 0.0,
        }
    }

    #[test]
    fn test_coverage_gain_and_utility_for_non_keep_sample() {
        // a: 4 agreeing "win" observations -> label_agreement=1.0, stability_score=1.0 -> Keep.
        // b: 1 "win" + 1 "loss" -> label_agreement=0.5, stability_score=0.5 -> Review.
        let mut obs = Vec::new();
        for i in 0..4 {
            obs.push(Observation {
                sample_id: "a".into(),
                label: Some("win".into()),
                evaluator_id: Some(format!("e{i}")),
                ..Default::default()
            });
        }
        obs.push(Observation {
            sample_id: "b".into(),
            label: Some("win".into()),
            evaluator_id: Some("e0".into()),
            ..Default::default()
        });
        obs.push(Observation {
            sample_id: "b".into(),
            label: Some("loss".into()),
            evaluator_id: Some("e1".into()),
            ..Default::default()
        });

        let config = ScoreConfig::default();
        let reports = score_all(obs, &config);

        let thresholds = Thresholds {
            keep: 0.5,
            drop: 0.40,
        };
        let costs = ActiveReviewCosts {
            gold_label: 2.0,
            ..ActiveReviewCosts::default()
        };
        let entries = rank_active_review(
            &reports,
            &zeroed_weights_low_lcb_only(),
            &costs,
            &thresholds,
            false,
            RankBy::Urgency,
        );

        let a = entries.iter().find(|e| e.sample_id == "a").unwrap();
        let b = entries.iter().find(|e| e.sample_id == "b").unwrap();

        assert_eq!(a.expected_coverage_gain, 0.0, "a is already Keep");

        assert_eq!(b.primary_reason, "low_lcb");
        assert_eq!(b.suggested_action, "request_gold_label");
        assert!(
            (b.label_agreement_flip_ratio.unwrap() - 1.0).abs() < 1e-9,
            "keep_threshold == b's label_agreement -> nearest == 0 -> ratio == 1.0 exactly, got {:?}",
            b.label_agreement_flip_ratio
        );
        assert!(
            (b.expected_coverage_gain - 0.5).abs() < 1e-9,
            "1/n_total = 1/2, got {}",
            b.expected_coverage_gain
        );
        assert_eq!(b.expected_risk_reduction, 0.0);
        assert!(
            (b.utility - 0.25).abs() < 1e-9,
            "1.0 * (0.5 + 0.0) / 2.0 = 0.25, got {}",
            b.utility
        );
    }

    #[test]
    fn test_risk_reduction_and_utility_for_at_risk_keep_sample() {
        // c: 3 "win" + 1 "loss", all score 0.9, no evaluator_id (so evaluator_agreement
        // doesn't also enter the mix) -> label_agreement=0.75, score_consistency=1.0
        // -> stability_score=0.875 -> Keep (default keep_threshold is 0.85).
        let mut obs = Vec::new();
        for _ in 0..3 {
            obs.push(Observation {
                sample_id: "c".into(),
                label: Some("win".into()),
                score: Some(0.9),
                ..Default::default()
            });
        }
        obs.push(Observation {
            sample_id: "c".into(),
            label: Some("loss".into()),
            score: Some(0.9),
            ..Default::default()
        });

        let config = ScoreConfig::default();
        let reports = score_all(obs, &config);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].decision, Decision::Keep);

        let thresholds = Thresholds {
            keep: 0.75,
            drop: 0.40,
        };
        let costs = ActiveReviewCosts {
            gold_label: 4.0,
            ..ActiveReviewCosts::default()
        };
        let entries = rank_active_review(
            &reports,
            &zeroed_weights_low_lcb_only(),
            &costs,
            &thresholds,
            false,
            RankBy::Urgency,
        );
        let c = &entries[0];

        assert_eq!(c.expected_coverage_gain, 0.0, "c is already Keep");
        assert!(
            (c.label_agreement_flip_ratio.unwrap() - 1.0).abs() < 1e-9,
            "keep_threshold == c's label_agreement -> ratio == 1.0 exactly, got {:?}",
            c.label_agreement_flip_ratio
        );
        assert!(
            (c.expected_risk_reduction - 1.0).abs() < 1e-9,
            "1/n_keep = 1/1 = 1.0, got {}",
            c.expected_risk_reduction
        );
        assert!(
            (c.utility - 0.25).abs() < 1e-9,
            "1.0 * (0.0 + 1.0) / 4.0 = 0.25, got {}",
            c.utility
        );
    }

    #[test]
    fn test_rank_by_utility_reorders_vs_urgency() {
        // "cheap": a Keep sample sitting exactly at keep_threshold (label_agreement=0.9,
        // moderate entropy) whose fix is cheap (add_evaluator). "pricey": a unanimous-label,
        // seed-sensitive Review sample, heavily weighted via seed_sensitivity so it's more
        // urgent, but its fix is expensive (add_seed) and it isn't at either threshold, so
        // its flip ratio is diluted by a larger `nearest`. Weights.lcb is 0 so Wilson-CI
        // noise doesn't drive urgency ranking (only used, via `thresholds`, for flip_ratio
        // and the at-risk-keep gate).
        let mut obs = Vec::new();
        for _ in 0..9 {
            obs.push(Observation {
                sample_id: "cheap".into(),
                label: Some("win".into()),
                ..Default::default()
            });
        }
        obs.push(Observation {
            sample_id: "cheap".into(),
            label: Some("loss".into()),
            ..Default::default()
        });
        for i in 0..4 {
            obs.push(Observation {
                sample_id: "pricey".into(),
                label: Some("win".into()),
                seed: Some((i % 2) as u64),
                score: Some(if i % 2 == 0 { 0.9 } else { 0.1 }),
                ..Default::default()
            });
        }

        let config = ScoreConfig::default();
        let reports = score_all(obs, &config);
        let cheap_report = reports.iter().find(|r| r.sample_id == "cheap").unwrap();
        assert_eq!(
            cheap_report.decision,
            Decision::Keep,
            "label_agreement=0.9 with no other components should clear the default 0.85 keep threshold"
        );

        let weights = ActiveReviewWeights {
            lcb: 0.0,
            entropy: 1.0,
            score_mad: 0.0,
            budget_sensitivity: 0.0,
            seed_sensitivity: 5.0,
            order_sensitivity: 0.0,
            teacher_conflict: 0.0,
            gradient_instability: 0.0,
        };
        let costs = ActiveReviewCosts {
            evaluator: 1.0,
            seed: 100.0,
            ..ActiveReviewCosts::default()
        };
        let thresholds = Thresholds {
            keep: 0.9,
            drop: 0.40,
        };

        let by_urgency = rank_active_review(
            &reports,
            &weights,
            &costs,
            &thresholds,
            false,
            RankBy::Urgency,
        );
        let by_utility = rank_active_review(
            &reports,
            &weights,
            &costs,
            &thresholds,
            false,
            RankBy::Utility,
        );

        assert_eq!(
            by_urgency[0].sample_id, "pricey",
            "pricey's heavily-weighted seed_sensitivity signal should dominate its unanimous \
             (zero-entropy) label, making it more urgent than cheap"
        );
        assert_eq!(
            by_utility[0].sample_id, "cheap",
            "cheap sits exactly at keep_threshold with a cheap fix, so it should rank highest \
             by utility despite ranking below pricey by urgency"
        );
    }

    #[test]
    fn test_no_labels_sample_has_none_flip_ratio_and_zero_expected_fields() {
        let obs = vec![
            Observation {
                sample_id: "s".into(),
                score: Some(0.5),
                evaluator_id: Some("e0".into()),
                ..Default::default()
            },
            Observation {
                sample_id: "s".into(),
                score: Some(0.6),
                evaluator_id: Some("e1".into()),
                ..Default::default()
            },
        ];
        let config = ScoreConfig::default();
        let reports = score_all(obs, &config);
        let entries = rank_active_review(
            &reports,
            &ActiveReviewWeights::default(),
            &ActiveReviewCosts::default(),
            &Thresholds::default(),
            false,
            RankBy::Urgency,
        );
        let s = &entries[0];
        assert!(s.label_agreement_flip_ratio.is_none());
        assert_eq!(s.expected_coverage_gain, 0.0);
        assert_eq!(s.expected_risk_reduction, 0.0);
    }

    #[test]
    fn test_all_five_signals_map_to_distinct_suggested_actions() {
        // For each signal, build a single-sample fixture that activates only that signal
        // (weight 1.0 on it, 0.0 on the rest) and confirm rank_active_review actually picks
        // the expected primary_reason/suggested_action pair -- a regression here would mean
        // the real mapping drifted, not just a hardcoded literal list.
        let zero_weights = |only: &str| ActiveReviewWeights {
            lcb: if only == "lcb" { 1.0 } else { 0.0 },
            entropy: if only == "entropy" { 1.0 } else { 0.0 },
            score_mad: if only == "score_mad" { 1.0 } else { 0.0 },
            budget_sensitivity: if only == "budget" { 1.0 } else { 0.0 },
            seed_sensitivity: if only == "seed" { 1.0 } else { 0.0 },
            order_sensitivity: if only == "order" { 1.0 } else { 0.0 },
            teacher_conflict: if only == "teacher" { 1.0 } else { 0.0 },
            gradient_instability: if only == "gradient" { 1.0 } else { 0.0 },
        };

        let cases: [(&str, Vec<Observation>, &str, &str); 8] = [
            (
                "lcb",
                vec![
                    Observation {
                        sample_id: "s".into(),
                        label: Some("win".into()),
                        ..Default::default()
                    },
                    Observation {
                        sample_id: "s".into(),
                        label: Some("win".into()),
                        ..Default::default()
                    },
                ],
                "low_lcb",
                "request_gold_label",
            ),
            (
                "entropy",
                vec![
                    Observation {
                        sample_id: "s".into(),
                        label: Some("win".into()),
                        ..Default::default()
                    },
                    Observation {
                        sample_id: "s".into(),
                        label: Some("loss".into()),
                        ..Default::default()
                    },
                ],
                "high_entropy",
                "add_evaluator",
            ),
            (
                "score_mad",
                vec![
                    Observation {
                        sample_id: "s".into(),
                        score: Some(0.1),
                        ..Default::default()
                    },
                    Observation {
                        sample_id: "s".into(),
                        score: Some(0.9),
                        ..Default::default()
                    },
                ],
                "high_score_mad",
                "add_model",
            ),
            (
                "budget",
                vec![
                    Observation {
                        sample_id: "s".into(),
                        score: Some(0.1),
                        budget: Some(1.0),
                        ..Default::default()
                    },
                    Observation {
                        sample_id: "s".into(),
                        score: Some(0.9),
                        budget: Some(2.0),
                        ..Default::default()
                    },
                ],
                "high_budget_sensitivity",
                "increase_budget",
            ),
            (
                "seed",
                vec![
                    Observation {
                        sample_id: "s".into(),
                        score: Some(0.1),
                        seed: Some(1),
                        ..Default::default()
                    },
                    Observation {
                        sample_id: "s".into(),
                        score: Some(0.9),
                        seed: Some(2),
                        ..Default::default()
                    },
                ],
                "high_seed_sensitivity",
                "add_seed",
            ),
            (
                "order",
                vec![
                    Observation {
                        sample_id: "s".into(),
                        score: Some(0.1),
                        shuffle_seed: Some(1),
                        ..Default::default()
                    },
                    Observation {
                        sample_id: "s".into(),
                        score: Some(0.9),
                        shuffle_seed: Some(2),
                        ..Default::default()
                    },
                ],
                "high_order_sensitivity",
                "add_shuffle_seed",
            ),
            (
                "teacher",
                vec![
                    Observation {
                        sample_id: "s".into(),
                        teacher_residual: Some(0.9),
                        ..Default::default()
                    },
                    Observation {
                        sample_id: "s".into(),
                        teacher_residual: Some(-0.9),
                        ..Default::default()
                    },
                ],
                "high_teacher_conflict",
                "flag_teacher_conflict",
            ),
            (
                "gradient",
                vec![
                    Observation {
                        sample_id: "s".into(),
                        gradient_sign: Some(0.1),
                        ..Default::default()
                    },
                    Observation {
                        sample_id: "s".into(),
                        gradient_sign: Some(-0.1),
                        ..Default::default()
                    },
                ],
                "high_gradient_instability",
                "add_init_seed",
            ),
        ];

        let mut seen_actions = std::collections::HashSet::new();
        for (only, obs, expected_reason, expected_action) in cases {
            let reports = score_all(obs, &ScoreConfig::default());
            let entries = rank_active_review(
                &reports,
                &zero_weights(only),
                &ActiveReviewCosts::default(),
                &Thresholds::default(),
                false,
                RankBy::Urgency,
            );
            assert_eq!(
                entries[0].primary_reason, expected_reason,
                "signal {only} should select primary_reason {expected_reason}"
            );
            assert_eq!(
                entries[0].suggested_action, expected_action,
                "signal {only} should map to suggested_action {expected_action}"
            );
            seen_actions.insert(entries[0].suggested_action);
        }
        assert_eq!(
            seen_actions.len(),
            8,
            "all eight signals must map to distinct suggested_action strings"
        );
    }

    fn dummy_entry(sample_id: &str, utility: f64, cost: f64) -> ActiveReviewEntry {
        ActiveReviewEntry {
            sample_id: sample_id.into(),
            urgency_score: 0.0,
            primary_reason: "low_lcb",
            suggested_action: "request_gold_label",
            label_agreement_lcb: None,
            label_entropy: None,
            budget_sensitivity: None,
            seed_sensitivity: None,
            label_agreement_flip_ratio: None,
            expected_coverage_gain: 0.0,
            expected_risk_reduction: 0.0,
            cost,
            utility,
            target_budget: None,
            target_seed: None,
            target_shuffle_seed: None,
            target_evaluator_slot: None,
            target_model_slot: None,
        }
    }

    #[test]
    fn test_select_within_budget_skips_unaffordable_and_continues_scanning() {
        // Sorted by utility desc: C(0.9,5.0), A(0.5,2.0), B(0.3,1.0). Budget=6.0: take C
        // (remaining 1.0), skip A (2.0 > 1.0, doesn't stop scanning), take B (1.0 <= 1.0).
        let entries = vec![
            dummy_entry("a", 0.5, 2.0),
            dummy_entry("b", 0.3, 1.0),
            dummy_entry("c", 0.9, 5.0),
        ];
        let selected = select_within_budget(&entries, 6.0);
        let ids: Vec<&str> = selected.iter().map(|e| e.sample_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["c", "b"],
            "should take the highest-utility affordable entry, skip the one that doesn't fit, \
             and keep scanning for a cheaper one that does"
        );
    }

    #[test]
    fn test_select_within_budget_unbounded_includes_everything_sorted_by_utility() {
        let entries = vec![
            dummy_entry("a", 0.5, 2.0),
            dummy_entry("b", 0.3, 1.0),
            dummy_entry("c", 0.9, 5.0),
        ];
        let selected = select_within_budget(&entries, f64::INFINITY);
        let ids: Vec<&str> = selected.iter().map(|e| e.sample_id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[test]
    fn test_select_within_budget_zero_budget_selects_nothing() {
        let entries = vec![dummy_entry("a", 0.5, 2.0)];
        assert!(select_within_budget(&entries, 0.0).is_empty());
    }

    fn obs_with_budget(budget: f64) -> Observation {
        Observation {
            sample_id: "s".into(),
            budget: Some(budget),
            ..Default::default()
        }
    }

    fn obs_with_seed(seed: u64) -> Observation {
        Observation {
            sample_id: "s".into(),
            seed: Some(seed),
            ..Default::default()
        }
    }

    fn obs_with_evaluator(evaluator_id: &str) -> Observation {
        Observation {
            sample_id: "s".into(),
            evaluator_id: Some(evaluator_id.into()),
            ..Default::default()
        }
    }

    fn obs_with_model(model_id: &str) -> Observation {
        Observation {
            sample_id: "s".into(),
            model_id: Some(model_id.into()),
            ..Default::default()
        }
    }

    #[test]
    fn test_action_target_fields_increase_budget_doubles_max_observed_budget() {
        let obs = vec![
            obs_with_budget(2.0),
            obs_with_budget(4.0),
            obs_with_budget(3.0),
        ];
        let targets = action_target_fields("increase_budget", &obs);
        assert_eq!(targets.target_budget, Some(8.0));
        assert_eq!(targets.target_seed, None);
        assert_eq!(targets.target_evaluator_slot, None);
        assert_eq!(targets.target_model_slot, None);
    }

    #[test]
    fn test_action_target_fields_add_seed_increments_max_observed_seed() {
        let obs = vec![obs_with_seed(1), obs_with_seed(5), obs_with_seed(2)];
        let targets = action_target_fields("add_seed", &obs);
        assert_eq!(targets.target_seed, Some(6));
        assert_eq!(targets.target_budget, None);
    }

    #[test]
    fn test_action_target_fields_add_evaluator_counts_distinct_plus_one() {
        let obs = vec![
            obs_with_evaluator("e1"),
            obs_with_evaluator("e2"),
            obs_with_evaluator("e1"),
            obs_with_evaluator("e3"),
        ];
        let targets = action_target_fields("add_evaluator", &obs);
        assert_eq!(targets.target_evaluator_slot, Some(4));
        assert_eq!(targets.target_model_slot, None);
    }

    #[test]
    fn test_action_target_fields_add_model_counts_distinct_plus_one() {
        let obs = vec![obs_with_model("m1"), obs_with_model("m2")];
        let targets = action_target_fields("add_model", &obs);
        assert_eq!(targets.target_model_slot, Some(3));
        assert_eq!(targets.target_evaluator_slot, None);
    }

    #[test]
    fn test_action_target_fields_request_gold_label_has_no_targets() {
        let obs = vec![obs_with_budget(2.0), obs_with_seed(1)];
        let targets = action_target_fields("request_gold_label", &obs);
        assert_eq!(targets.target_budget, None);
        assert_eq!(targets.target_seed, None);
        assert_eq!(targets.target_evaluator_slot, None);
        assert_eq!(targets.target_model_slot, None);
    }

    #[test]
    fn test_action_target_fields_none_when_relevant_raw_value_absent() {
        // suggested_action is increase_budget but none of these observations carry a budget —
        // None (not a fabricated default), since there's no evidence to size a target from.
        let obs = vec![obs_with_seed(1), obs_with_evaluator("e1")];
        let targets = action_target_fields("increase_budget", &obs);
        assert_eq!(targets.target_budget, None);
    }

    #[test]
    fn test_rank_active_review_is_order_independent() {
        // n_total/n_keep (used by expected_coverage_gain/expected_risk_reduction) and the final
        // sort (urgency_score desc, tie-broken by sample_id) are both order-independent by
        // construction, so shuffling the input reports must produce an identical output Vec,
        // not just an identical *set* of entries — a tool meant to measure order/seed
        // sensitivity would be a bad joke if it were itself sensitive to input order.
        let mut obs = Vec::new();
        for i in 0..4 {
            obs.push(Observation {
                sample_id: "a".into(),
                label: Some("win".into()),
                evaluator_id: Some(format!("e{i}")),
                ..Default::default()
            });
        }
        obs.push(Observation {
            sample_id: "b".into(),
            label: Some("win".into()),
            evaluator_id: Some("e0".into()),
            ..Default::default()
        });
        obs.push(Observation {
            sample_id: "b".into(),
            label: Some("loss".into()),
            evaluator_id: Some("e1".into()),
            ..Default::default()
        });
        obs.push(Observation {
            sample_id: "c".into(),
            score: Some(0.1),
            seed: Some(1),
            ..Default::default()
        });
        obs.push(Observation {
            sample_id: "c".into(),
            score: Some(0.9),
            seed: Some(2),
            ..Default::default()
        });

        let config = ScoreConfig::default();
        let reports = score_all(obs, &config);
        let mut reversed = reports.clone();
        reversed.reverse();

        let weights = ActiveReviewWeights::default();
        let costs = ActiveReviewCosts::default();
        let thresholds = Thresholds::default();

        let extract = |entries: &[ActiveReviewEntry]| -> Vec<(String, f64, &'static str, f64)> {
            entries
                .iter()
                .map(|e| {
                    (
                        e.sample_id.clone(),
                        e.urgency_score,
                        e.suggested_action,
                        e.utility,
                    )
                })
                .collect()
        };

        let a = rank_active_review(
            &reports,
            &weights,
            &costs,
            &thresholds,
            false,
            RankBy::Urgency,
        );
        let b = rank_active_review(
            &reversed,
            &weights,
            &costs,
            &thresholds,
            false,
            RankBy::Urgency,
        );
        assert_eq!(extract(&a), extract(&b));

        let a_util = rank_active_review(
            &reports,
            &weights,
            &costs,
            &thresholds,
            false,
            RankBy::Utility,
        );
        let b_util = rank_active_review(
            &reversed,
            &weights,
            &costs,
            &thresholds,
            false,
            RankBy::Utility,
        );
        assert_eq!(extract(&a_util), extract(&b_util));
    }
}
