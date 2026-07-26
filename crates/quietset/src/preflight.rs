//! Instrumentation preflight: dataset-level coverage and completeness checks, run *before* a
//! real `trajectory-audit`, so a missing `block_id`/`shuffle_seed` is caught while the
//! experiment can still be re-instrumented rather than discovered after the fact.
//!
//! This module computes no new statistics. It composes what `block-score`/`trajectory-audit`
//! already do — [`group_by_block_id`], [`loss_recipe_issues`], [`loss_recipe_comparable`] — and
//! reports the coverage those commands silently assume. Several things are silent today and are
//! surfaced here: observations dropped for having no `block_id`, blocks present on only one
//! side of a before/after pair, blocks whose before/after `run_id` identity disagrees, and
//! blocks with too few distinct seeds for the seed-based metrics to compute at all.
//!
//! Findings are split into **blocking** (trajectory-audit's output on this input cannot be
//! trusted at all — a required field is completely absent, a `block_id` aggregates two
//! different runs, zero blocks correspond between `--before`/`--after`, or a matched block's
//! identity or `loss_recipe` is contradictory) and **warning** (degraded but not wrong — partial
//! field coverage, too few seeds for one reproducibility metric, some blocks one-sided). See
//! [`PreflightReport::ready`].

use crate::block::{LossRecipeIssue, loss_recipe_comparable, loss_recipe_issues};
use crate::group::group_by_block_id;
use crate::observation::Observation;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

/// Which downstream command a [`PreflightReport`] was produced for — determines which fields
/// are checked for coverage and which per-block thresholds apply. A `--for block-score` variant
/// would be one new variant plus one arm in [`required_fields`]; it is not added speculatively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreflightTarget {
    /// The fields and structure `trajectory-audit` reads.
    TrajectoryAudit,
}

/// The `Observation` fields `target`'s command actually reads, paired with their presence test,
/// in report order. Exposed so `--help` text and the docs cannot drift from what is checked.
#[allow(clippy::type_complexity)]
pub fn required_fields(
    target: PreflightTarget,
) -> &'static [(&'static str, fn(&Observation) -> bool)] {
    match target {
        PreflightTarget::TrajectoryAudit => &[
            ("block_id", |o| o.block_id.is_some()),
            ("seed", |o| o.seed.is_some()),
            ("shuffle_seed", |o| o.shuffle_seed.is_some()),
            ("model_id", |o| o.model_id.is_some()),
            ("layer_id", |o| o.layer_id.is_some()),
            ("loss_recipe", |o| o.loss_recipe.is_some()),
            ("trajectory_effect", |o| o.trajectory_effect.is_some()),
            ("update_cosine", |o| o.update_cosine.is_some()),
            ("dead_unit_count", |o| o.dead_unit_count.is_some()),
            ("saturated_unit_count", |o| o.saturated_unit_count.is_some()),
        ],
    }
}

/// Non-null coverage of one `Observation` field across one side's observations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldCoverage {
    pub field: String,
    pub n_present: usize,
    pub n_total: usize,
    /// `n_present / n_total`; `0.0` when `n_total` is 0.
    pub coverage: f64,
}

/// A `block_id` uniqueness problem, detected via `run_id`.
///
/// `block_id`'s documented contract is that it is unique within one input dataset — a producer
/// combining independent training runs into one file must namespace it itself, e.g.
/// `"{run_id}:{block_id}"`. That contract has no code enforcement anywhere else in quietset;
/// this is the only check for it, and it can only fire when `run_id` is populated. When every
/// observation lacks `run_id` (see [`SidePreflight::n_missing_run_id`]), an empty issue list
/// means "not checkable", not "verified unique". Deliberately mirrors [`LossRecipeIssue`]'s
/// `Mixed`/`IncompleteCoverage` pair — same shape of problem, same shape of report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BlockRunIdIssue {
    /// More than one distinct `run_id` under one `block_id` — the namespacing above did not
    /// happen, so two unrelated runs' blocks are being aggregated as one block. Blocking: the
    /// block's own stability numbers are already meaningless, not just degraded.
    Mixed { run_ids: Vec<String> },
    /// All present `run_id` values in this block agree, but some observations carry none, so
    /// the namespacing contract cannot be verified for those. Not evidence of a collision —
    /// a warning, not blocking.
    IncompleteCoverage { run_id: String },
}

fn block_run_id_issue(obs: &[Observation]) -> Option<BlockRunIdIssue> {
    let run_ids: Vec<String> = obs
        .iter()
        .filter_map(|o| o.run_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    match run_ids.len() {
        0 => None,
        1 => obs
            .iter()
            .any(|o| o.run_id.is_none())
            .then(|| BlockRunIdIssue::IncompleteCoverage {
                run_id: run_ids[0].clone(),
            }),
        _ => Some(BlockRunIdIssue::Mixed { run_ids }),
    }
}

/// A block with too few distinct seed values for the seed-based metrics. Always a warning, never
/// blocking on its own: `trajectory-audit` still runs and reports on the block, just without
/// `reproducibility_3seed` (and `block-score`'s `seed_effect_consistency` degrades gracefully
/// too) — a coverage gap, not a reason to distrust the whole input.
///
/// `block-score`'s `seed_effect_consistency`/`shuffle_direction_consistency` need >= 2 distinct
/// values on their respective axis; `trajectory-audit`'s `reproducibility_3seed` needs >= 3
/// distinct `seed` values on the after side. A block is listed here when `n_distinct_seeds < 3`;
/// the raw counts let a reader tell "2 seeds — block-score works, reproducibility_3seed will
/// not" from "0 seeds — nothing computes".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedCoverage {
    pub block_id: String,
    pub n_distinct_seeds: usize,
    pub n_distinct_shuffle_seeds: usize,
}

/// Everything checkable from one file on its own. Computed independently for the primary input
/// and for `--after`, because a checkpoint pair where only the after file lost `shuffle_seed` is
/// exactly the failure preflight exists to catch — a merged figure would hide it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidePreflight {
    pub n_observations: usize,
    /// Per-field non-null coverage, in [`required_fields`]'s fixed order.
    pub field_coverage: Vec<FieldCoverage>,
    /// Observations with no `block_id`. [`group_by_block_id`] drops these with no warning, so
    /// every block-level number below — and every `block-score`/`trajectory-audit` result —
    /// simply never sees them.
    pub n_missing_block_id: usize,
    pub n_blocks: usize,
    /// Per `block_id`, in first-seen order; only blocks with an issue appear.
    pub block_run_id_issues: Vec<(String, BlockRunIdIssue)>,
    /// Observations with no `run_id` at all. When this equals `n_observations`, the
    /// `block_run_id_issues` check could not run.
    pub n_missing_run_id: usize,
    /// Blocks with fewer than 3 distinct `seed` values, in first-seen order.
    pub blocks_below_seed_threshold: Vec<SeedCoverage>,
    /// From [`loss_recipe_issues`], unchanged.
    pub loss_recipe_issues: Vec<(String, LossRecipeIssue)>,
}

fn compute_side_preflight(observations: &[Observation], target: PreflightTarget) -> SidePreflight {
    let n_observations = observations.len();
    let field_coverage = required_fields(target)
        .iter()
        .map(|(field, present)| {
            let n_present = observations.iter().filter(|o| present(o)).count();
            FieldCoverage {
                field: field.to_string(),
                n_present,
                n_total: n_observations,
                coverage: if n_observations == 0 {
                    0.0
                } else {
                    n_present as f64 / n_observations as f64
                },
            }
        })
        .collect();

    let n_missing_block_id = observations.iter().filter(|o| o.block_id.is_none()).count();
    let groups = group_by_block_id(observations.iter().cloned());
    let n_blocks = groups.len();

    let n_missing_run_id = observations.iter().filter(|o| o.run_id.is_none()).count();
    let block_run_id_issues = groups
        .iter()
        .filter_map(|(id, obs)| block_run_id_issue(obs).map(|issue| (id.clone(), issue)))
        .collect();

    let blocks_below_seed_threshold = groups
        .iter()
        .filter_map(|(id, obs)| {
            let n_distinct_seeds = obs
                .iter()
                .filter_map(|o| o.seed)
                .collect::<BTreeSet<_>>()
                .len();
            let n_distinct_shuffle_seeds = obs
                .iter()
                .filter_map(|o| o.shuffle_seed)
                .collect::<BTreeSet<_>>()
                .len();
            (n_distinct_seeds < 3).then(|| SeedCoverage {
                block_id: id.clone(),
                n_distinct_seeds,
                n_distinct_shuffle_seeds,
            })
        })
        .collect();

    SidePreflight {
        n_observations,
        field_coverage,
        n_missing_block_id,
        n_blocks,
        block_run_id_issues,
        n_missing_run_id,
        blocks_below_seed_threshold,
        loss_recipe_issues: loss_recipe_issues(observations),
    }
}

/// Cross-file checks. Only produced when an `after` set was supplied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointCorrespondence {
    /// Blocks present on both sides — the only blocks `trajectory-audit` would report on.
    pub n_blocks_matched: usize,
    pub n_blocks_only_before: usize,
    pub n_blocks_only_after: usize,
    /// Up to 10 `block_id`s, in first-seen order. `trajectory-audit` skips these silently today
    /// (a block on only one side never gets matched, no warning), so a half-instrumented
    /// checkpoint pair looks like a clean run that simply had fewer blocks.
    pub blocks_only_before: Vec<String>,
    pub blocks_only_after: Vec<String>,
    /// Matched blocks that `trajectory-audit` would mark `comparable: false` — their
    /// `loss_recipe` differs between the two sides, or is internally mixed on either side. Uses
    /// [`loss_recipe_comparable`], the same rule `compute_trajectory_audit` applies.
    pub blocks_not_comparable: Vec<String>,
    /// Matched blocks whose `run_id` identity contradicts itself across `--before`/`--after`:
    /// both sides carry a `run_id`, and the two sides' `run_id` sets are completely disjoint.
    /// This is the cross-file analogue of [`BlockRunIdIssue::Mixed`] — the same `block_id`
    /// string names two actually-different training runs, so diffing it as one block's
    /// before/after trajectory is comparing unrelated data, not measuring training progress.
    ///
    /// **Contract this check enforces:** `trajectory-audit`'s `--before`/`--after` model is for
    /// two checkpoints of the *same* training run (the same `block_id` is expected to carry the
    /// same `run_id` on both sides) — not a general tool for comparing reproducibility across
    /// distinct runs. That's why a disjoint `run_id` set is blocking rather than a warning: it
    /// reads as an accidental `block_id` collision between two unrelated runs, never as a
    /// legitimate comparison. If you need to compare reproducibility *across* runs, `block_id`
    /// alone can't express that — you need an explicit shared key (e.g. a lineage/recipe/seed
    /// identifier) this check doesn't have; treat cross-run comparison as unsupported until one
    /// is named and added, rather than relying on `run_id` disagreeing to mean "different but
    /// comparable" data.
    pub identity_mismatched_blocks: Vec<String>,
}

const MAX_CORRESPONDENCE_SAMPLE: usize = 10;

fn compute_checkpoint_correspondence(
    before: &[Observation],
    after: &[Observation],
) -> CheckpointCorrespondence {
    let before_groups = group_by_block_id(before.iter().cloned());
    let after_groups = group_by_block_id(after.iter().cloned());

    let before_ids: HashSet<String> = before_groups.keys().cloned().collect();
    let after_ids: HashSet<String> = after_groups.keys().cloned().collect();

    let matched: Vec<String> = before_groups
        .keys()
        .filter(|id| after_ids.contains(*id))
        .cloned()
        .collect();
    let only_before: Vec<String> = before_groups
        .keys()
        .filter(|id| !after_ids.contains(*id))
        .cloned()
        .collect();
    let only_after: Vec<String> = after_groups
        .keys()
        .filter(|id| !before_ids.contains(*id))
        .cloned()
        .collect();

    let blocks_not_comparable: Vec<String> = matched
        .iter()
        .filter(|id| {
            !loss_recipe_comparable(&before_groups[id.as_str()], &after_groups[id.as_str()])
        })
        .cloned()
        .collect();

    let identity_mismatched_blocks: Vec<String> = matched
        .iter()
        .filter(|id| {
            let before_run_ids: BTreeSet<&str> = before_groups[id.as_str()]
                .iter()
                .filter_map(|o| o.run_id.as_deref())
                .collect();
            let after_run_ids: BTreeSet<&str> = after_groups[id.as_str()]
                .iter()
                .filter_map(|o| o.run_id.as_deref())
                .collect();
            !before_run_ids.is_empty()
                && !after_run_ids.is_empty()
                && before_run_ids.is_disjoint(&after_run_ids)
        })
        .cloned()
        .collect();

    CheckpointCorrespondence {
        n_blocks_matched: matched.len(),
        n_blocks_only_before: only_before.len(),
        n_blocks_only_after: only_after.len(),
        blocks_only_before: only_before
            .into_iter()
            .take(MAX_CORRESPONDENCE_SAMPLE)
            .collect(),
        blocks_only_after: only_after
            .into_iter()
            .take(MAX_CORRESPONDENCE_SAMPLE)
            .collect(),
        blocks_not_comparable,
        identity_mismatched_blocks,
    }
}

/// Dataset-level instrumentation report for `target`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightReport {
    pub target: PreflightTarget,
    pub input: SidePreflight,
    /// `None` in single-file mode.
    pub after: Option<SidePreflight>,
    /// `None` in single-file mode.
    pub checkpoint_correspondence: Option<CheckpointCorrespondence>,
    /// `true` when `blocking_issues` is empty — `trajectory-audit`'s output on this input can be
    /// trusted (though `warnings` may still list degraded-but-not-wrong signals). Drives the CLI
    /// exit code: non-zero when `false`, unless `--report-only` was passed.
    pub ready: bool,
    pub blocking_issue_count: usize,
    pub warning_count: usize,
    /// Problems that make `trajectory-audit`'s output untrustworthy, not just degraded: a
    /// required field entirely absent, a `block_id` that aggregates more than one `run_id`, a
    /// block's own `loss_recipe` internally mixed, zero blocks corresponding between
    /// `--before`/`--after`, a matched block's `run_id` identity contradicting itself across
    /// sides, or a matched block's `loss_recipe` mismatched across sides.
    pub blocking_issues: Vec<String>,
    /// Problems that degrade specific metrics without invalidating the run: partial field
    /// coverage, a block with too few distinct seeds for `reproducibility_3seed`, some (not all)
    /// blocks present on only one side, or partial `run_id`/`loss_recipe` coverage within a
    /// block.
    pub warnings: Vec<String>,
    /// Matched blocks `trajectory-audit` would report on as `comparable: true`. `None` without
    /// `--after` — there is nothing to compare.
    pub comparable_block_count: Option<usize>,
    /// Matched blocks `trajectory-audit` would mark `comparable: false`. `None` without
    /// `--after`.
    pub incomparable_block_count: Option<usize>,
}

/// Pushes findings from one side's [`SidePreflight`] into `blocking`/`warnings`. `block_id` at
/// 0% coverage is the one field whose total absence is blocking (see [`PreflightReport`]'s doc)
/// — every other field, and every partial-coverage case, is a warning: the analysis still runs,
/// just with that dimension unavailable.
fn push_side_issues(
    blocking: &mut Vec<String>,
    warnings: &mut Vec<String>,
    side: &str,
    report: &SidePreflight,
) {
    for fc in &report.field_coverage {
        if fc.n_total == 0 || fc.n_present == fc.n_total {
            continue;
        }
        if fc.n_present == 0 && fc.field == "block_id" {
            blocking.push(format!(
                "{side}: block_id is present on 0/{} observations — trajectory-audit cannot group this input into blocks at all",
                fc.n_total
            ));
        } else {
            warnings.push(format!(
                "{side}: {} coverage is {}/{} ({:.0}%)",
                fc.field,
                fc.n_present,
                fc.n_total,
                fc.coverage * 100.0
            ));
        }
    }
    for (block_id, issue) in &report.block_run_id_issues {
        match issue {
            BlockRunIdIssue::Mixed { run_ids } => blocking.push(format!(
                "{side}: block {block_id} spans {} distinct run_id values ({}) — likely an unnamespaced block_id collision",
                run_ids.len(),
                run_ids.join(", ")
            )),
            BlockRunIdIssue::IncompleteCoverage { run_id } => warnings.push(format!(
                "{side}: block {block_id} has run_id {run_id} on some observations but not all"
            )),
        }
    }
    for sc in &report.blocks_below_seed_threshold {
        warnings.push(format!(
            "{side}: block {} has only {} distinct seed value(s) (need >= 3 for reproducibility_3seed)",
            sc.block_id, sc.n_distinct_seeds
        ));
    }
    for (block_id, issue) in &report.loss_recipe_issues {
        match issue {
            LossRecipeIssue::Mixed { recipes } => blocking.push(format!(
                "{side}: block {block_id} mixes loss_recipe values ({}) — classification forced to insufficient",
                recipes.join(", ")
            )),
            LossRecipeIssue::IncompleteCoverage { recipe } => warnings.push(format!(
                "{side}: block {block_id} has loss_recipe {recipe} on some observations but not all"
            )),
        }
    }
}

/// Run the instrumentation preflight. `after` is `None` for a single-file check; the cross-file
/// sections of the report are then `None` rather than empty, so "not checked" stays
/// distinguishable from "checked, nothing found".
pub fn compute_preflight(
    observations: &[Observation],
    after: Option<&[Observation]>,
    target: PreflightTarget,
) -> PreflightReport {
    let input = compute_side_preflight(observations, target);
    let after_side = after.map(|a| compute_side_preflight(a, target));
    let checkpoint_correspondence =
        after.map(|a| compute_checkpoint_correspondence(observations, a));

    let mut blocking_issues = Vec::new();
    let mut warnings = Vec::new();
    push_side_issues(&mut blocking_issues, &mut warnings, "input", &input);
    if let Some(a) = &after_side {
        push_side_issues(&mut blocking_issues, &mut warnings, "after", a);
    }

    let (comparable_block_count, incomparable_block_count) = if let Some(cc) =
        &checkpoint_correspondence
    {
        if cc.n_blocks_matched == 0 {
            blocking_issues.push(
                "--after was given but zero blocks correspond between --before and --after; \
                     trajectory-audit would report nothing"
                    .to_string(),
            );
        }
        if cc.n_blocks_only_before > 0 {
            warnings.push(format!(
                    "{} block(s) present only in --before would be silently skipped by trajectory-audit",
                    cc.n_blocks_only_before
                ));
        }
        if cc.n_blocks_only_after > 0 {
            warnings.push(format!(
                "{} block(s) present only in --after would be silently skipped by trajectory-audit",
                cc.n_blocks_only_after
            ));
        }
        if !cc.identity_mismatched_blocks.is_empty() {
            blocking_issues.push(format!(
                "{} matched block(s) have contradictory run_id identity between --before and \
                     --after (same block_id, disjoint run_ids) — trajectory-audit's before/after \
                     model assumes both sides are checkpoints from the same training run, so this \
                     reads as an unnamespaced block_id collision across two different runs, not a \
                     legitimate cross-run comparison: {}",
                cc.identity_mismatched_blocks.len(),
                cc.identity_mismatched_blocks.join(", ")
            ));
        }
        if !cc.blocks_not_comparable.is_empty() {
            blocking_issues.push(format!(
                    "{} matched block(s) have a loss_recipe mismatch and would be reported as not comparable",
                    cc.blocks_not_comparable.len()
                ));
        }
        (
            Some(cc.n_blocks_matched - cc.blocks_not_comparable.len()),
            Some(cc.blocks_not_comparable.len()),
        )
    } else {
        (None, None)
    };

    let blocking_issue_count = blocking_issues.len();
    let warning_count = warnings.len();

    PreflightReport {
        target,
        input,
        after: after_side,
        checkpoint_correspondence,
        ready: blocking_issue_count == 0,
        blocking_issue_count,
        warning_count,
        blocking_issues,
        warnings,
        comparable_block_count,
        incomparable_block_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(block_id: Option<&str>, seed: Option<u64>, run_id: Option<&str>) -> Observation {
        Observation {
            sample_id: "s".into(),
            block_id: block_id.map(String::from),
            seed,
            run_id: run_id.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn test_field_coverage_reports_fraction_present() {
        let observations = vec![
            obs(Some("b1"), Some(1), None),
            obs(None, Some(2), None),
            obs(None, Some(3), None),
        ];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        let block_id_coverage = report
            .input
            .field_coverage
            .iter()
            .find(|fc| fc.field == "block_id")
            .unwrap();
        assert_eq!(block_id_coverage.n_present, 1);
        assert_eq!(block_id_coverage.n_total, 3);
        assert!((block_id_coverage.coverage - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn test_missing_block_id_observations_are_counted_not_silently_dropped() {
        let observations = vec![obs(Some("b1"), Some(1), None), obs(None, Some(2), None)];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert_eq!(report.input.n_missing_block_id, 1);
        assert_eq!(report.input.n_blocks, 1);
    }

    #[test]
    fn test_block_id_entirely_absent_is_blocking() {
        let observations = vec![
            Observation {
                sample_id: "a".into(),
                ..Default::default()
            },
            Observation {
                sample_id: "b".into(),
                ..Default::default()
            },
        ];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert!(!report.ready);
        assert!(report.blocking_issue_count > 0);
        assert!(
            report
                .blocking_issues
                .iter()
                .any(|f| f.contains("block_id") && f.contains("0/2")),
            "{:?}",
            report.blocking_issues
        );
    }

    #[test]
    fn test_partial_field_coverage_is_warning_not_blocking() {
        let observations = vec![obs(Some("b1"), Some(1), None), obs(Some("b2"), None, None)];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert!(report.ready, "partial seed coverage must not be blocking");
        assert!(report.warning_count > 0);
    }

    #[test]
    fn test_block_spanning_two_run_ids_is_reported_as_mixed_and_blocking() {
        let observations = vec![
            obs(Some("b1"), Some(1), Some("run1")),
            obs(Some("b1"), Some(2), Some("run2")),
        ];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert_eq!(report.input.block_run_id_issues.len(), 1);
        assert!(matches!(
            &report.input.block_run_id_issues[0].1,
            BlockRunIdIssue::Mixed { run_ids } if run_ids.len() == 2
        ));
        assert!(!report.ready);
        assert!(report.blocking_issues.iter().any(|f| f.contains("run_id")));
    }

    #[test]
    fn test_block_with_partial_run_id_is_incomplete_coverage_and_a_warning() {
        let observations = vec![
            obs(Some("b1"), Some(1), Some("run1")),
            obs(Some("b1"), Some(2), None),
        ];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert_eq!(report.input.block_run_id_issues.len(), 1);
        assert!(matches!(
            &report.input.block_run_id_issues[0].1,
            BlockRunIdIssue::IncompleteCoverage { run_id } if run_id == "run1"
        ));
        assert!(
            report.ready,
            "incomplete run_id coverage alone is a warning, not blocking"
        );
    }

    #[test]
    fn test_run_id_check_is_inert_when_run_id_absent_everywhere() {
        let observations = vec![
            obs(Some("b1"), Some(1), None),
            obs(Some("b1"), Some(2), None),
        ];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert!(report.input.block_run_id_issues.is_empty());
        assert_eq!(report.input.n_missing_run_id, observations.len());
    }

    #[test]
    fn test_blocks_with_fewer_than_three_seeds_are_listed_and_not_blocking() {
        let observations = vec![
            obs(Some("b1"), Some(1), None),
            obs(Some("b1"), Some(2), None),
        ];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert_eq!(report.input.blocks_below_seed_threshold.len(), 1);
        assert_eq!(
            report.input.blocks_below_seed_threshold[0].n_distinct_seeds,
            2
        );
        assert!(report.ready, "seed shortage alone must not be blocking");
    }

    #[test]
    fn test_checkpoint_correspondence_lists_blocks_present_on_only_one_side() {
        let before = vec![
            obs(Some("b1"), Some(1), None),
            obs(Some("b2"), Some(1), None),
        ];
        let after = vec![
            obs(Some("b1"), Some(1), None),
            obs(Some("b3"), Some(1), None),
        ];
        let report = compute_preflight(&before, Some(&after), PreflightTarget::TrajectoryAudit);
        let cc = report.checkpoint_correspondence.expect("after was given");
        assert_eq!(cc.n_blocks_matched, 1);
        assert_eq!(cc.blocks_only_before, vec!["b2".to_string()]);
        assert_eq!(cc.blocks_only_after, vec!["b3".to_string()]);
        assert!(
            report.ready,
            "some (not all) one-sided blocks is a warning, not blocking"
        );
    }

    #[test]
    fn test_zero_matched_blocks_is_blocking() {
        let before = vec![obs(Some("b1"), Some(1), None)];
        let after = vec![obs(Some("b2"), Some(1), None)];
        let report = compute_preflight(&before, Some(&after), PreflightTarget::TrajectoryAudit);
        assert!(!report.ready);
        assert_eq!(
            report.checkpoint_correspondence.unwrap().n_blocks_matched,
            0
        );
        assert!(
            report
                .blocking_issues
                .iter()
                .any(|f| f.contains("zero blocks correspond"))
        );
    }

    #[test]
    fn test_checkpoint_correspondence_is_none_without_after() {
        let observations = vec![obs(Some("b1"), Some(1), None)];
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert!(report.after.is_none());
        assert!(report.checkpoint_correspondence.is_none());
        assert!(report.comparable_block_count.is_none());
        assert!(report.incomparable_block_count.is_none());
    }

    #[test]
    fn test_after_side_coverage_is_reported_separately_from_input_side() {
        let before = vec![Observation {
            sample_id: "s".into(),
            block_id: Some("b1".into()),
            shuffle_seed: Some(1),
            ..Default::default()
        }];
        let after = vec![Observation {
            sample_id: "s".into(),
            block_id: Some("b1".into()),
            shuffle_seed: None,
            ..Default::default()
        }];
        let report = compute_preflight(&before, Some(&after), PreflightTarget::TrajectoryAudit);
        let input_shuffle = report
            .input
            .field_coverage
            .iter()
            .find(|fc| fc.field == "shuffle_seed")
            .unwrap();
        let after_shuffle = report
            .after
            .as_ref()
            .unwrap()
            .field_coverage
            .iter()
            .find(|fc| fc.field == "shuffle_seed")
            .unwrap();
        assert_eq!(input_shuffle.n_present, 1);
        assert_eq!(after_shuffle.n_present, 0);
    }

    #[test]
    fn test_loss_recipe_mismatch_block_is_not_comparable_and_blocking() {
        let before = vec![Observation {
            sample_id: "s".into(),
            block_id: Some("b1".into()),
            loss_recipe: Some("baseline".into()),
            ..Default::default()
        }];
        let after = vec![Observation {
            sample_id: "s".into(),
            block_id: Some("b1".into()),
            loss_recipe: Some("teacher_conflict_masking".into()),
            ..Default::default()
        }];
        let report = compute_preflight(&before, Some(&after), PreflightTarget::TrajectoryAudit);
        let cc = report.checkpoint_correspondence.expect("after was given");
        assert_eq!(cc.blocks_not_comparable, vec!["b1".to_string()]);
        assert!(!report.ready);
        assert_eq!(report.comparable_block_count, Some(0));
        assert_eq!(report.incomparable_block_count, Some(1));
    }

    #[test]
    fn test_before_after_identity_mismatch_is_detected_and_blocking() {
        // Same block_id on both sides, but the run_id sets are completely disjoint -- this is
        // an accidental block_id collision across two unrelated training runs, not a real
        // before/after pair of the same block.
        let before = vec![
            obs(Some("b1"), Some(1), Some("run1")),
            obs(Some("b1"), Some(2), Some("run1")),
        ];
        let after = vec![
            obs(Some("b1"), Some(1), Some("run2")),
            obs(Some("b1"), Some(2), Some("run2")),
        ];
        let report = compute_preflight(&before, Some(&after), PreflightTarget::TrajectoryAudit);
        let cc = report.checkpoint_correspondence.expect("after was given");
        assert_eq!(cc.identity_mismatched_blocks, vec!["b1".to_string()]);
        assert!(!report.ready);
        assert!(
            report
                .blocking_issues
                .iter()
                .any(|f| f.contains("contradictory run_id identity"))
        );
    }

    #[test]
    fn test_one_sided_run_id_is_not_an_identity_mismatch() {
        // Only the after side has run_id -- nothing to contradict, so this must not be flagged.
        let before = vec![obs(Some("b1"), Some(1), None)];
        let after = vec![obs(Some("b1"), Some(1), Some("run2"))];
        let report = compute_preflight(&before, Some(&after), PreflightTarget::TrajectoryAudit);
        let cc = report.checkpoint_correspondence.expect("after was given");
        assert!(cc.identity_mismatched_blocks.is_empty());
    }

    #[test]
    fn test_findings_are_empty_and_ready_on_a_fully_instrumented_dataset() {
        let make = |seed: u64| Observation {
            sample_id: format!("s{seed}"),
            block_id: Some("b1".into()),
            seed: Some(seed),
            shuffle_seed: Some(seed),
            model_id: Some("ckpt1".into()),
            layer_id: Some("ft".into()),
            loss_recipe: Some("baseline".into()),
            trajectory_effect: Some(0.1),
            update_cosine: Some(0.9),
            dead_unit_count: Some(0.0),
            saturated_unit_count: Some(0.0),
            run_id: Some("run1".into()),
            ..Default::default()
        };
        let observations: Vec<Observation> = (0..3).map(make).collect();
        let report = compute_preflight(&observations, None, PreflightTarget::TrajectoryAudit);
        assert!(report.ready);
        assert_eq!(report.blocking_issue_count, 0);
        assert_eq!(report.warning_count, 0);
        assert!(report.blocking_issues.is_empty());
        assert!(report.warnings.is_empty());
    }
}
