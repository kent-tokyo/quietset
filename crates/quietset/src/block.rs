//! Block-level trajectory stability: groups observations by `block_id` (e.g. a 32-sample
//! training block) instead of `sample_id`, and classifies each block's training effect as
//! stable growth/shrink, sensitive to one specific condition axis, or pathological.

use crate::observation::Observation;
use crate::scoring::{induced_rate, sign_agreement};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Thresholds driving [`BlockClass`] classification. All are operational defaults, not derived
/// constants — tune via the `block-score` CLI flags or by constructing this directly.
#[derive(Debug, Clone)]
pub struct BlockThresholds {
    /// `block_stability` at or above this is "stable" (growth/shrink still to be determined by
    /// `trajectory_effect_mean`'s sign). Default `0.85`, matching `Decision`'s default keep
    /// threshold.
    pub stable: f64,
    /// A consistency factor (seed/shuffle/checkpoint) below this is "the problem" driving an
    /// unstable block's classification. Default `0.5`.
    pub sensitivity: f64,
    /// `dead_unit_rate`/`saturated_unit_rate` above this overrides every other signal —
    /// destructive regardless of measured stability. Default `0.5`.
    pub pathological_dead_rate: f64,
}

impl Default for BlockThresholds {
    fn default() -> Self {
        Self {
            stable: 0.85,
            sensitivity: 0.5,
            pathological_dead_rate: 0.5,
        }
    }
}

/// Classification of a training block's stability, from [`compute_block_report`].
///
/// **`StableGrowth`/`StableShrink` are not "good"/"bad" labels** — they describe
/// *reproducibility* of the sign of `trajectory_effect`, not whether that effect is desirable.
/// A shrink accompanied by rising `dead_unit_rate`/`saturated_unit_rate` is caught separately by
/// `Pathological`, but a shrink with clean unit health could just as well be healthy
/// regularization. Recommended handling, pending outcome validation (held-out teacher precision,
/// downstream match results) rather than trusting the classification alone:
///
/// | Class | Suggested handling |
/// |-------|---------------------|
/// | `Pathological` | Exclude from training |
/// | `Insufficient` | Collect more observations before deciding |
/// | `SeedSensitive` | Add another `seed` and re-classify |
/// | `OrderSensitive` | Add another `shuffle_seed` and re-classify |
/// | `TrajectorySensitive` | Re-evaluate at another checkpoint |
/// | `StableGrowth` | Send to downstream validation — do not auto-keep |
/// | `StableShrink` | Send to downstream validation — do not auto-drop |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockClass {
    /// Stable across seed/shuffle/checkpoint, with net positive `trajectory_effect`. Not
    /// inherently "good" — see the type-level doc for why growth/shrink need downstream
    /// validation, not an automatic keep/drop rule.
    StableGrowth,
    /// Stable across seed/shuffle/checkpoint, with net negative `trajectory_effect`. Not
    /// inherently "bad" — see the type-level doc.
    StableShrink,
    /// Unstable, and `checkpoint_reproducibility` is the lowest (or only sub-threshold) factor.
    TrajectorySensitive,
    /// Unstable, and `seed_effect_consistency` is the lowest (or only sub-threshold) factor.
    SeedSensitive,
    /// Unstable, and `shuffle_direction_consistency` is the lowest (or only sub-threshold) factor.
    OrderSensitive,
    /// `dead_unit_rate`/`saturated_unit_rate` exceeds `pathological_dead_rate` — destructive
    /// regardless of any other signal.
    Pathological,
    /// Not enough condition-axis diversity (or a growth/shrink sign) to classify further.
    Insufficient,
}

impl std::fmt::Display for BlockClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BlockClass::StableGrowth => "stable_growth",
            BlockClass::StableShrink => "stable_shrink",
            BlockClass::TrajectorySensitive => "trajectory_sensitive",
            BlockClass::SeedSensitive => "seed_sensitive",
            BlockClass::OrderSensitive => "order_sensitive",
            BlockClass::Pathological => "pathological",
            BlockClass::Insufficient => "insufficient",
        };
        write!(f, "{s}")
    }
}

/// Stability report for one training block, aggregated across all its observations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockStabilityReport {
    /// The block this report describes.
    pub block_id: String,
    /// Number of distinct `sample_id` values in this block.
    pub n_samples: usize,
    /// Number of observations used to compute this report.
    pub n_observations: usize,
    /// Sign-agreement of `trajectory_effect` across `seed` (init seed) groups' means. `None`
    /// if fewer than 2 distinct seeds carry `trajectory_effect`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_effect_consistency: Option<f64>,
    /// Sign-agreement of `update_cosine` across `shuffle_seed` groups' means. `None` if fewer
    /// than 2 distinct shuffle seeds carry `update_cosine`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shuffle_direction_consistency: Option<f64>,
    /// Sign-agreement of `trajectory_effect` across `model_id` (checkpoint) groups' means.
    /// `None` if fewer than 2 distinct checkpoints carry `trajectory_effect`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_reproducibility: Option<f64>,
    /// Product of whichever of the 3 factors above are present. `None` if none are.
    ///
    /// **Not a calibrated probability of anything** (e.g. of a downstream playing-strength
    /// improvement) — a quietset-specific operational heuristic that treats all three
    /// reproducibility axes as non-substitutable (see the doc comment at its computation site
    /// in `compute_block_report` for why the terms are multiplied rather than averaged).
    /// `stable`/`sensitivity`/`pathological_dead_rate` in [`BlockThresholds`] are operational
    /// defaults, not universal constants — validate on your own held-out data (and, for a game
    /// engine, actual match results) before trusting this classification to gate training data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_stability: Option<f64>,
    /// Mean of `trajectory_effect` across the block's observations — positive means net
    /// growth, negative net shrink. `None` if no observation carries it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_effect_mean: Option<f64>,
    /// Fraction of observations carrying `dead_unit_count` where it is `> 0.0`. `None` if none
    /// carry it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_unit_rate: Option<f64>,
    /// Fraction of observations carrying `saturated_unit_count` where it is `> 0.0`. `None` if
    /// none carry it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturated_unit_rate: Option<f64>,
    /// This block's stability classification.
    pub classification: BlockClass,
}

/// A loss-recipe consistency problem found within one block's observations. See
/// [`loss_recipe_issues`] and the override in [`compute_block_report`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum LossRecipeIssue {
    /// More than one distinct non-null `loss_recipe` present in this block. We do not
    /// aggregate different loss recipes into one block score because their effects are
    /// confounded with seed, shuffle order, and checkpoint variation — a block cannot be
    /// "seed-stable" or "pathological" in a way attributable to any one axis if the recipe
    /// itself also changed underneath it. Until recipe-level stability is modeled explicitly
    /// (a dedicated `loss_recipe_consistency` factor), a mixed block is `Insufficient` rather
    /// than evidence of instability.
    Mixed { recipes: Vec<String> },
    /// All present `loss_recipe` values agree, but some observations in this block carry
    /// none at all. Classification is unaffected — this is only an operator warning that the
    /// field isn't consistently populated.
    IncompleteCoverage { recipe: String },
}

/// Distinct non-null `loss_recipe` values across `obs`, sorted for deterministic comparison.
pub(crate) fn distinct_loss_recipes(obs: &[Observation]) -> Vec<String> {
    obs.iter()
        .filter_map(|o| o.loss_recipe.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// True when one block's `before`/`after` observations can be diffed as an ordinary trajectory
/// effect: neither side mixes `loss_recipe` internally, and when both sides populate it they
/// agree. Data that never uses `loss_recipe` at all (both sides empty) is comparable, same as
/// before this check existed. Extracted from [`crate::compute_trajectory_audit`] so `preflight`
/// reports the same rule rather than a copy that can drift from it.
pub fn loss_recipe_comparable(before: &[Observation], after: &[Observation]) -> bool {
    let before_recipes = distinct_loss_recipes(before);
    let after_recipes = distinct_loss_recipes(after);
    !(before_recipes.len() > 1
        || after_recipes.len() > 1
        || (!before_recipes.is_empty()
            && !after_recipes.is_empty()
            && before_recipes != after_recipes))
}

fn loss_recipe_issue(obs: &[Observation]) -> Option<LossRecipeIssue> {
    let recipes = distinct_loss_recipes(obs);
    match recipes.len() {
        0 => None,
        1 => obs.iter().any(|o| o.loss_recipe.is_none()).then(|| {
            LossRecipeIssue::IncompleteCoverage {
                recipe: recipes[0].clone(),
            }
        }),
        _ => Some(LossRecipeIssue::Mixed { recipes }),
    }
}

/// Scans every block in `observations` for a loss-recipe consistency problem (see
/// [`LossRecipeIssue`]), in first-seen block order. Used by the `block-score`/
/// `trajectory-audit` CLI commands to warn operators without failing the whole run over a
/// partial data problem.
pub fn loss_recipe_issues(observations: &[Observation]) -> Vec<(String, LossRecipeIssue)> {
    let groups = crate::group::group_by_block_id(observations.iter().cloned());
    groups
        .iter()
        .filter_map(|(id, obs)| loss_recipe_issue(obs).map(|issue| (id.clone(), issue)))
        .collect()
}

/// Groups `obs` by `key_fn`, averages `value_fn` within each group, and returns the
/// sign-agreement of those per-group means. `None` if fewer than 2 groups have a value.
/// This is the cross-condition-axis analogue of `scoring::compute_range_sensitivity`: instead
/// of a normalized range, it measures whether groups agree on the *sign* of their effect.
pub(crate) fn cross_group_effect_consistency<'a, K, FK, FV>(
    obs: &'a [Observation],
    key_fn: FK,
    value_fn: FV,
) -> Option<f64>
where
    K: std::hash::Hash + Eq,
    FK: Fn(&'a Observation) -> Option<K>,
    FV: Fn(&'a Observation) -> Option<f64>,
{
    let mut groups: HashMap<K, Vec<f64>> = HashMap::new();
    for o in obs {
        if let (Some(k), Some(v)) = (key_fn(o), value_fn(o)) {
            groups.entry(k).or_default().push(v);
        }
    }
    if groups.len() < 2 {
        return None;
    }
    let group_means: Vec<f64> = groups
        .values()
        .map(|vs| vs.iter().sum::<f64>() / vs.len() as f64)
        .collect();
    sign_agreement(&group_means)
}

/// First-match-wins classification decision tree, reading every field of `r` except
/// `classification` itself. See [`BlockClass`] variants for what each branch means; tie-break
/// among sub-threshold factors follows fixed declaration order (seed → shuffle → checkpoint),
/// the same pattern as `StabilityComponents::weakest`.
fn classify(r: &BlockStabilityReport, t: &BlockThresholds) -> BlockClass {
    if r.dead_unit_rate
        .is_some_and(|v| v > t.pathological_dead_rate)
        || r.saturated_unit_rate
            .is_some_and(|v| v > t.pathological_dead_rate)
    {
        return BlockClass::Pathological;
    }
    let Some(stability) = r.block_stability else {
        return BlockClass::Insufficient;
    };
    if stability >= t.stable {
        return match r.trajectory_effect_mean {
            Some(x) if x > 0.0 => BlockClass::StableGrowth,
            Some(x) if x < 0.0 => BlockClass::StableShrink,
            // Zero or absent: can't tell growth from shrink without the effect field.
            _ => BlockClass::Insufficient,
        };
    }
    // Unstable overall. Name the cause via whichever available factor is lowest, if it's below
    // `sensitivity` — a block can also land here with every individual factor above
    // `sensitivity` (their product still undercuts `stable`); that's reported as Insufficient
    // rather than guessing which axis is "the" cause.
    let candidates: [(BlockClass, Option<f64>); 3] = [
        (BlockClass::SeedSensitive, r.seed_effect_consistency),
        (BlockClass::OrderSensitive, r.shuffle_direction_consistency),
        (
            BlockClass::TrajectorySensitive,
            r.checkpoint_reproducibility,
        ),
    ];
    candidates
        .into_iter()
        .filter_map(|(class, val)| val.map(|v| (class, v)))
        .filter(|(_, v)| *v < t.sensitivity)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(class, _)| class)
        .unwrap_or(BlockClass::Insufficient)
}

/// Compute a [`BlockStabilityReport`] for one block from its observations.
///
/// # Trust boundary
///
/// Same contract as [`crate::scoring::compute_report`]: `obs` is expected to already be
/// validated (finite `trajectory_effect`/`update_cosine`/`dead_unit_count`/
/// `saturated_unit_count`) — [`crate::parse_jsonl`]/[`crate::parse_csv`] already do this via
/// [`Observation::validate`]. Checked via `debug_assert!` only (panics in debug builds,
/// silently accepted in release builds) for observations built directly through the Rust API.
pub fn compute_block_report(
    block_id: &str,
    obs: &[Observation],
    thresholds: &BlockThresholds,
) -> BlockStabilityReport {
    for o in obs {
        for (field, value) in [
            ("trajectory_effect", o.trajectory_effect),
            ("update_cosine", o.update_cosine),
            ("dead_unit_count", o.dead_unit_count),
            ("saturated_unit_count", o.saturated_unit_count),
        ] {
            debug_assert!(
                value.is_none_or(f64::is_finite),
                "compute_block_report: non-finite {field} (call Observation::validate() before scoring)"
            );
        }
    }
    let n_samples = obs
        .iter()
        .map(|o| o.sample_id.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len();

    let seed_effect_consistency =
        cross_group_effect_consistency(obs, |o| o.seed, |o| o.trajectory_effect);
    let shuffle_direction_consistency =
        cross_group_effect_consistency(obs, |o| o.shuffle_seed, |o| o.update_cosine);
    let checkpoint_reproducibility =
        cross_group_effect_consistency(obs, |o| o.model_id.as_deref(), |o| o.trajectory_effect);

    // We multiply the three consistency terms because block stability requires all three
    // forms of reproducibility; a high value in one dimension should not compensate for
    // instability in another (a block that reproduces perfectly across seeds but not at all
    // across checkpoints is not "50% stable" — it is not reproducible). The result is an
    // operational, quietset-specific score, not a calibrated probability of anything
    // downstream (e.g. of an actual playing-strength improvement) — see docs/metrics.md.
    let factors: Vec<f64> = [
        seed_effect_consistency,
        shuffle_direction_consistency,
        checkpoint_reproducibility,
    ]
    .into_iter()
    .flatten()
    .collect();
    let block_stability = if factors.is_empty() {
        None
    } else {
        Some(factors.iter().product())
    };

    let trajectory_effects: Vec<f64> = obs.iter().filter_map(|o| o.trajectory_effect).collect();
    let trajectory_effect_mean = if trajectory_effects.is_empty() {
        None
    } else {
        Some(trajectory_effects.iter().sum::<f64>() / trajectory_effects.len() as f64)
    };

    let dead_unit_rate = induced_rate(obs, |o| o.dead_unit_count);
    let saturated_unit_rate = induced_rate(obs, |o| o.saturated_unit_count);

    let mut report = BlockStabilityReport {
        block_id: block_id.to_string(),
        n_samples,
        n_observations: obs.len(),
        seed_effect_consistency,
        shuffle_direction_consistency,
        checkpoint_reproducibility,
        block_stability,
        trajectory_effect_mean,
        dead_unit_rate,
        saturated_unit_rate,
        classification: BlockClass::Insufficient,
    };
    report.classification = classify(&report, thresholds);
    // Mixed non-null loss_recipe values within a block override the classification above —
    // see LossRecipeIssue::Mixed for why the recipe axis can't be aggregated like model_id/
    // seed/shuffle_seed can. This takes priority even over Pathological: a mixed-recipe block's
    // dead/saturated-unit rate can't be trusted to mean what Pathological's doc says it means.
    if matches!(loss_recipe_issue(obs), Some(LossRecipeIssue::Mixed { .. })) {
        report.classification = BlockClass::Insufficient;
    }
    report
}

/// Compute block reports for every `block_id` present in `observations`, in first-seen order.
/// Observations with no `block_id` are ignored (see [`crate::group::group_by_block_id`]).
pub fn score_all_blocks(
    observations: Vec<Observation>,
    thresholds: &BlockThresholds,
) -> Vec<BlockStabilityReport> {
    let groups = crate::group::group_by_block_id(observations.into_iter());
    groups
        .iter()
        .map(|(id, obs)| compute_block_report(id, obs, thresholds))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(
        sample_id: &str,
        seed: Option<u64>,
        shuffle_seed: Option<u64>,
        model_id: Option<&str>,
        trajectory_effect: Option<f64>,
        update_cosine: Option<f64>,
    ) -> Observation {
        Observation {
            sample_id: sample_id.into(),
            block_id: Some("b1".into()),
            seed,
            shuffle_seed,
            model_id: model_id.map(String::from),
            trajectory_effect,
            update_cosine,
            ..Default::default()
        }
    }

    #[test]
    fn test_stable_growth() {
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
            obs("s2", Some(1), Some(1), Some("ckpt1"), Some(0.2), Some(0.7)),
            obs("s2", Some(2), Some(2), Some("ckpt2"), Some(0.5), Some(0.6)),
        ];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.classification, BlockClass::StableGrowth);
        assert_eq!(report.n_samples, 2);
        assert_eq!(report.block_stability, Some(1.0));
    }

    #[test]
    fn test_stable_shrink() {
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(-0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(-0.4), Some(0.8)),
        ];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.classification, BlockClass::StableShrink);
    }

    #[test]
    fn test_seed_sensitive() {
        // 3 distinct seeds with trajectory_effect signs +/-/0 -> sign_agreement = 1/3, clearly
        // below the 0.5 sensitivity threshold. shuffle_seed and model_id stay constant (1
        // group each -> None), so seed is the only candidate factor.
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.5), Some(0.9)),
            obs("s1", Some(2), Some(1), Some("ckpt1"), Some(-0.5), Some(0.9)),
            obs("s1", Some(3), Some(1), Some("ckpt1"), Some(0.0), Some(0.9)),
        ];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert!((report.seed_effect_consistency.unwrap() - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(report.classification, BlockClass::SeedSensitive);
    }

    #[test]
    fn test_order_sensitive() {
        // 3 distinct shuffle seeds with update_cosine signs +/-/0 -> sign_agreement = 1/3.
        // seed and model_id stay constant, so shuffle is the only candidate factor.
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), None, Some(0.5)),
            obs("s1", Some(1), Some(2), Some("ckpt1"), None, Some(-0.5)),
            obs("s1", Some(1), Some(3), Some("ckpt1"), None, Some(0.0)),
        ];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert!((report.shuffle_direction_consistency.unwrap() - 1.0 / 3.0).abs() < 1e-9);
        // seed/model_id have only 1 distinct value here -> None -> not candidates.
        assert_eq!(report.classification, BlockClass::OrderSensitive);
    }

    #[test]
    fn test_trajectory_sensitive() {
        // 3 distinct checkpoints with trajectory_effect signs +/-/0 -> sign_agreement = 1/3.
        // seed and shuffle_seed stay constant, so checkpoint is the only candidate factor.
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.5), Some(0.9)),
            obs("s1", Some(1), Some(1), Some("ckpt2"), Some(-0.5), Some(0.9)),
            obs("s1", Some(1), Some(1), Some("ckpt3"), Some(0.0), Some(0.9)),
        ];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert!((report.checkpoint_reproducibility.unwrap() - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(report.classification, BlockClass::TrajectorySensitive);
    }

    #[test]
    fn test_pathological_overrides_everything() {
        let mut observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
        ];
        // Even though the fixture is otherwise a clean stable-growth case, a high dead_unit_rate
        // must override it.
        observations[0].dead_unit_count = Some(5.0);
        observations[1].dead_unit_count = Some(3.0);
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.dead_unit_rate, Some(1.0));
        assert_eq!(report.classification, BlockClass::Pathological);
    }

    #[test]
    fn test_insufficient_with_no_condition_axes() {
        let observations = vec![Observation {
            sample_id: "s1".into(),
            block_id: Some("b1".into()),
            ..Default::default()
        }];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.block_stability, None);
        assert_eq!(report.classification, BlockClass::Insufficient);
    }

    #[test]
    fn test_score_all_blocks_skips_observations_without_block_id() {
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            Observation {
                sample_id: "s2".into(),
                ..Default::default()
            },
        ];
        let reports = score_all_blocks(observations, &BlockThresholds::default());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].n_observations, 1);
    }

    // --- edge cases: missing/insufficient data must never fabricate a score ---

    #[test]
    fn test_zero_observations_is_insufficient_not_a_panic() {
        let report = compute_block_report("b1", &[], &BlockThresholds::default());
        assert_eq!(report.n_samples, 0);
        assert_eq!(report.n_observations, 0);
        assert_eq!(report.seed_effect_consistency, None);
        assert_eq!(report.shuffle_direction_consistency, None);
        assert_eq!(report.checkpoint_reproducibility, None);
        assert_eq!(report.block_stability, None);
        assert_eq!(report.dead_unit_rate, None);
        assert_eq!(report.classification, BlockClass::Insufficient);
    }

    #[test]
    fn test_single_observation_is_insufficient_even_with_every_field_present() {
        // One observation can never populate 2 distinct groups on any axis, so every
        // consistency factor is None regardless of how many fields it carries.
        let observations = vec![obs(
            "s1",
            Some(1),
            Some(1),
            Some("ckpt1"),
            Some(0.5),
            Some(0.9),
        )];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.block_stability, None);
        assert_eq!(report.classification, BlockClass::Insufficient);
    }

    #[test]
    fn test_single_seed_only_leaves_seed_factor_none() {
        // 3 observations, but all share seed=1 -> only 1 distinct seed group -> None. The
        // other two axes vary, so they alone drive block_stability.
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(1), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
        ];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.seed_effect_consistency, None);
        assert!(report.shuffle_direction_consistency.is_some());
        assert!(report.checkpoint_reproducibility.is_some());
    }

    #[test]
    fn test_single_checkpoint_only_leaves_checkpoint_factor_none() {
        // All observations share model_id -> only 1 distinct checkpoint group -> None.
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt1"), Some(0.4), Some(0.8)),
        ];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.checkpoint_reproducibility, None);
        assert!(report.seed_effect_consistency.is_some());
        assert!(report.shuffle_direction_consistency.is_some());
    }

    #[test]
    fn test_partially_missing_fields_only_use_observations_that_carry_them() {
        // 3rd observation carries none of the trajectory fields at all -- it must be excluded
        // from every factor's computation, not treated as a 0.0 or averaged in.
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.5), Some(0.9)),
            obs("s1", Some(2), Some(1), Some("ckpt1"), Some(0.5), Some(0.9)),
            Observation {
                sample_id: "s1".into(),
                block_id: Some("b1".into()),
                seed: Some(3),
                ..Default::default()
            },
        ];
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        // Only seeds 1 and 2 carry trajectory_effect -> 2 groups, both positive -> agreement 1.0.
        // Seed 3's group has no trajectory_effect at all, so it never enters the grouping.
        assert_eq!(report.seed_effect_consistency, Some(1.0));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "call Observation::validate() before scoring")]
    fn test_compute_block_report_panics_on_non_finite_field_bypassing_parser() {
        let observations = vec![Observation {
            sample_id: "s1".into(),
            block_id: Some("b1".into()),
            dead_unit_count: Some(f64::NAN),
            ..Default::default()
        }];
        let _ = compute_block_report("b1", &observations, &BlockThresholds::default());
    }

    // --- classification boundaries ---
    //
    // Using round thresholds (0.5) rather than the BlockThresholds defaults (0.85/0.5) so the
    // exact boundary value is trivial to hit with a small, hand-verifiable fixture (an even
    // split across 2 groups gives sign_agreement == 0.5 exactly) instead of hunting for a
    // fraction that happens to equal 0.85.

    #[test]
    fn test_stable_boundary_at_exact_threshold_is_stable() {
        // Single factor (seed) present, others None (constant shuffle/model) -> block_stability
        // == seed_effect_consistency exactly. 2 seed groups, both positive -> 1.0 (well above
        // any reasonable threshold) is the easy case; the real boundary check is the `>=`
        // itself, exercised by setting the threshold to exactly the computed value.
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(1), Some("ckpt1"), Some(0.4), Some(0.8)),
        ];
        let thresholds = BlockThresholds {
            stable: 1.0, // exactly equal to the computed block_stability
            ..BlockThresholds::default()
        };
        let report = compute_block_report("b1", &observations, &thresholds);
        assert_eq!(report.block_stability, Some(1.0));
        assert_eq!(
            report.classification,
            BlockClass::StableGrowth,
            "block_stability == stable_threshold exactly must count as stable (>=, not >)"
        );
    }

    #[test]
    fn test_stable_boundary_just_below_threshold_is_not_stable() {
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(1), Some("ckpt1"), Some(0.4), Some(0.8)),
        ];
        let thresholds = BlockThresholds {
            stable: 1.0 + 1e-9, // just above the computed 1.0 block_stability
            sensitivity: 0.0,   // no factor should register as "the problem" at this sensitivity
            ..BlockThresholds::default()
        };
        let report = compute_block_report("b1", &observations, &thresholds);
        assert_ne!(report.classification, BlockClass::StableGrowth);
        assert_ne!(report.classification, BlockClass::StableShrink);
    }

    #[test]
    fn test_sensitivity_boundary_exact_value_is_not_flagged() {
        // 2 groups split 1/1 -> sign_agreement == 0.5 exactly. The filter is strict `<
        // sensitivity`, so sensitivity == 0.5 must NOT flag this factor as the culprit.
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.5), Some(0.9)),
            obs("s1", Some(2), Some(1), Some("ckpt1"), Some(-0.5), Some(0.9)),
        ];
        let thresholds = BlockThresholds {
            stable: 2.0, // unreachable, forces the unstable branch
            sensitivity: 0.5,
            ..BlockThresholds::default()
        };
        let report = compute_block_report("b1", &observations, &thresholds);
        assert_eq!(report.seed_effect_consistency, Some(0.5));
        assert_eq!(
            report.classification,
            BlockClass::Insufficient,
            "a factor exactly at `sensitivity` must not be flagged (strict <), so no cause \
             qualifies here"
        );
    }

    #[test]
    fn test_sensitivity_boundary_just_below_value_is_flagged() {
        let observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.5), Some(0.9)),
            obs("s1", Some(2), Some(1), Some("ckpt1"), Some(-0.5), Some(0.9)),
        ];
        let thresholds = BlockThresholds {
            stable: 2.0,
            sensitivity: 0.5 + 1e-9, // just above 0.5, so the 0.5 factor now qualifies
            ..BlockThresholds::default()
        };
        let report = compute_block_report("b1", &observations, &thresholds);
        assert_eq!(report.classification, BlockClass::SeedSensitive);
    }

    // --- loss_recipe consistency: mixed recipes must not be silently aggregated ---

    #[test]
    fn test_mixed_loss_recipe_forces_insufficient_even_over_stable_growth() {
        // Otherwise a clean stable_growth fixture (see test_stable_growth) -- but the two
        // observations disagree on loss_recipe, so the classification must not be trusted.
        let mut observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
        ];
        observations[0].loss_recipe = Some("recipe_a".into());
        observations[1].loss_recipe = Some("recipe_b".into());
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.classification, BlockClass::Insufficient);
    }

    #[test]
    fn test_mixed_loss_recipe_overrides_pathological_too() {
        // Even a block that would otherwise read as Pathological must not be trusted once
        // its loss_recipe is mixed -- the dead_unit_rate itself is confounded.
        let mut observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
        ];
        observations[0].dead_unit_count = Some(5.0);
        observations[1].dead_unit_count = Some(3.0);
        observations[0].loss_recipe = Some("recipe_a".into());
        observations[1].loss_recipe = Some("recipe_b".into());
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.classification, BlockClass::Insufficient);
    }

    #[test]
    fn test_incomplete_loss_recipe_coverage_does_not_change_classification() {
        // Same stable_growth fixture, but only one of the two observations carries
        // loss_recipe (the other has none) -- both present values agree, so this is a
        // coverage gap, not a mix, and must not affect the classification.
        let mut observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
        ];
        observations[0].loss_recipe = Some("recipe_a".into());
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.classification, BlockClass::StableGrowth);
    }

    #[test]
    fn test_loss_recipe_issues_reports_mixed_and_incomplete_but_not_clean_blocks() {
        let mut observations = vec![
            // B1: mixed non-null recipes.
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
            // B2: one recipe, one missing -- incomplete coverage.
            obs("s2", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s2", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
            // B3: no loss_recipe at all on either observation -- clean, no issue.
            obs("s3", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s3", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
        ];
        for o in observations.iter_mut() {
            o.block_id = Some(
                match o.sample_id.as_str() {
                    "s1" => "B1",
                    "s2" => "B2",
                    _ => "B3",
                }
                .into(),
            );
        }
        observations[0].loss_recipe = Some("recipe_a".into());
        observations[1].loss_recipe = Some("recipe_b".into());
        observations[2].loss_recipe = Some("recipe_a".into());
        // observations[3] (B2's other half) intentionally left with no loss_recipe.

        let issues = loss_recipe_issues(&observations);
        assert_eq!(issues.len(), 2, "{issues:?}");
        let b1 = issues.iter().find(|(id, _)| id == "B1").unwrap();
        assert!(matches!(&b1.1, LossRecipeIssue::Mixed { recipes }
                if *recipes == vec!["recipe_a".to_string(), "recipe_b".to_string()]));
        let b2 = issues.iter().find(|(id, _)| id == "B2").unwrap();
        assert!(
            matches!(&b2.1, LossRecipeIssue::IncompleteCoverage { recipe } if recipe == "recipe_a")
        );
        assert!(issues.iter().all(|(id, _)| id != "B3"));
    }

    #[test]
    fn test_loss_recipe_comparable_matches_mixed_and_cross_side_cases() {
        // Extracted from compute_trajectory_audit -- covered end-to-end via that function's
        // own tests in trajectory_audit.rs; this pins the extracted function directly.
        let mut same_recipe_before = vec![obs("s1", Some(1), None, None, None, None)];
        let mut same_recipe_after = vec![obs("s1", Some(1), None, None, None, None)];
        same_recipe_before[0].loss_recipe = Some("baseline".into());
        same_recipe_after[0].loss_recipe = Some("baseline".into());
        assert!(loss_recipe_comparable(
            &same_recipe_before,
            &same_recipe_after
        ));

        let mut mismatched_before = vec![obs("s1", Some(1), None, None, None, None)];
        let mut mismatched_after = vec![obs("s1", Some(1), None, None, None, None)];
        mismatched_before[0].loss_recipe = Some("baseline".into());
        mismatched_after[0].loss_recipe = Some("teacher_conflict_masking".into());
        assert!(!loss_recipe_comparable(
            &mismatched_before,
            &mismatched_after
        ));

        let mut mixed_within_before = vec![
            obs("s1", Some(1), None, None, None, None),
            obs("s2", Some(1), None, None, None, None),
        ];
        mixed_within_before[0].loss_recipe = Some("recipe_a".into());
        mixed_within_before[1].loss_recipe = Some("recipe_b".into());
        let clean_after = vec![obs("s1", Some(1), None, None, None, None)];
        assert!(!loss_recipe_comparable(&mixed_within_before, &clean_after));

        let no_recipe_either_side = vec![obs("s1", Some(1), None, None, None, None)];
        assert!(loss_recipe_comparable(
            &no_recipe_either_side,
            &no_recipe_either_side
        ));
    }

    #[test]
    fn test_multiple_layer_ids_within_one_block_is_normal() {
        // A block naturally spans multiple layers (see group_by_block_id's doc) -- this must
        // not error, warn, or affect classification, unlike a mixed loss_recipe.
        let mut observations = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
        ];
        observations[0].layer_id = Some("ft".into());
        observations[1].layer_id = Some("l2".into());
        let report = compute_block_report("b1", &observations, &BlockThresholds::default());
        assert_eq!(report.classification, BlockClass::StableGrowth);
        assert!(loss_recipe_issues(&observations).is_empty());
    }

    // --- determinism: computed values must not depend on input observation order ---

    #[test]
    fn test_compute_block_report_is_order_independent() {
        let forward = vec![
            obs("s1", Some(1), Some(1), Some("ckpt1"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckpt2"), Some(0.4), Some(0.8)),
            obs("s2", Some(1), Some(1), Some("ckpt1"), Some(0.2), Some(0.7)),
            obs("s2", Some(2), Some(2), Some("ckpt2"), Some(0.5), Some(0.6)),
        ];
        let mut reversed = forward.clone();
        reversed.reverse();

        let thresholds = BlockThresholds::default();
        let a = compute_block_report("b1", &forward, &thresholds);
        let b = compute_block_report("b1", &reversed, &thresholds);
        assert_eq!(a.seed_effect_consistency, b.seed_effect_consistency);
        assert_eq!(
            a.shuffle_direction_consistency,
            b.shuffle_direction_consistency
        );
        assert_eq!(a.checkpoint_reproducibility, b.checkpoint_reproducibility);
        assert_eq!(a.block_stability, b.block_stability);
        // trajectory_effect_mean sums floats in observation order, so forward vs. reversed can
        // differ at the ULP level (float addition isn't associative) -- the same property
        // score_mean already has elsewhere in this codebase. Epsilon comparison, not exact.
        assert!(
            (a.trajectory_effect_mean.unwrap() - b.trajectory_effect_mean.unwrap()).abs() < 1e-9,
            "{:?} vs {:?}",
            a.trajectory_effect_mean,
            b.trajectory_effect_mean
        );
        assert_eq!(a.classification, b.classification);
    }

    #[test]
    fn test_score_all_blocks_values_are_order_independent_per_block_id() {
        // The returned Vec's *order* legitimately follows first-seen block_id order (an
        // IndexMap, like group_by_sample_id) -- that's a documented, intentional property, not
        // a determinism bug. What must not change is each block's own computed values.
        let mut observations = vec![
            obs("s1", Some(1), Some(1), Some("ckptA"), Some(0.3), Some(0.9)),
            obs("s1", Some(2), Some(2), Some("ckptB"), Some(0.4), Some(0.8)),
            obs("q1", Some(1), Some(1), Some("ckptA"), Some(-0.3), Some(0.9)),
            obs("q1", Some(2), Some(2), Some("ckptB"), Some(-0.4), Some(0.8)),
        ];
        observations[0].block_id = Some("B1".into());
        observations[1].block_id = Some("B1".into());
        observations[2].block_id = Some("B2".into());
        observations[3].block_id = Some("B2".into());

        let mut shuffled = observations.clone();
        shuffled.swap(0, 3);
        shuffled.swap(1, 2);

        let thresholds = BlockThresholds::default();
        let forward: std::collections::HashMap<String, BlockStabilityReport> =
            score_all_blocks(observations, &thresholds)
                .into_iter()
                .map(|r| (r.block_id.clone(), r))
                .collect();
        let reordered: std::collections::HashMap<String, BlockStabilityReport> =
            score_all_blocks(shuffled, &thresholds)
                .into_iter()
                .map(|r| (r.block_id.clone(), r))
                .collect();

        for block_id in ["B1", "B2"] {
            let f = &forward[block_id];
            let r = &reordered[block_id];
            assert_eq!(f.block_stability, r.block_stability, "block {block_id}");
            assert_eq!(f.classification, r.classification, "block {block_id}");
        }
    }
}
