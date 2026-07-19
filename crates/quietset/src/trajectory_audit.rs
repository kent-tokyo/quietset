//! Destructive-sample detection: diffs block-level trajectory stability between a "before" and
//! "after" observation set (e.g. two training checkpoints), surfacing blocks whose dead/
//! saturated-unit rates spiked or whose net trajectory effect flipped.

use crate::block::{BlockClass, BlockThresholds, compute_block_report};
use crate::group::group_by_block_id;
use crate::observation::Observation;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Before/after diff for one block, from [`compute_trajectory_audit`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryAuditEntry {
    /// The block this entry describes.
    pub block_id: String,
    /// Per `layer_id`: `after_rate - before_rate` for `dead_unit_rate`. Layers seen on only one
    /// side are treated as `0.0` on the other, so a brand-new problem layer still shows its
    /// full `after_rate` as the delta.
    pub dead_unit_delta: IndexMap<String, f64>,
    /// Per `layer_id`: `after_rate - before_rate` for `saturated_unit_rate`.
    pub saturated_unit_delta: IndexMap<String, f64>,
    /// `after.trajectory_effect_mean - before.trajectory_effect_mean`. `None` if either side
    /// lacks `trajectory_effect`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_effect_delta: Option<f64>,
    pub before_classification: BlockClass,
    pub after_classification: BlockClass,
    /// `seed_effect_consistency` on the "after" side, but only when at least 3 distinct init
    /// seeds are present there — `None` otherwise, since fewer than 3 seeds is too little
    /// evidence to call something "3-seed reproducible".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reproducibility_3seed: Option<f64>,
}

/// Groups `obs` by `layer_id` and computes the induction rate (fraction of present values
/// `> 0.0`) of `field` within each layer. Layers with no observation carrying `field` are
/// omitted.
fn per_layer_rate(
    obs: &[Observation],
    field: impl Fn(&Observation) -> Option<f64>,
) -> IndexMap<String, f64> {
    let mut groups: IndexMap<String, (usize, usize)> = IndexMap::new();
    for o in obs {
        if let (Some(layer), Some(v)) = (o.layer_id.as_deref(), field(o)) {
            let entry = groups.entry(layer.to_string()).or_insert((0, 0));
            entry.1 += 1;
            if v > 0.0 {
                entry.0 += 1;
            }
        }
    }
    groups
        .into_iter()
        .map(|(layer, (induced, total))| (layer, induced as f64 / total as f64))
        .collect()
}

/// Per-layer `after - before`, over the union of layers seen on either side (missing side
/// treated as `0.0`), in before-then-newly-seen-after order.
fn per_layer_delta(
    before: &IndexMap<String, f64>,
    after: &IndexMap<String, f64>,
) -> IndexMap<String, f64> {
    let mut delta: IndexMap<String, f64> = IndexMap::new();
    for (layer, before_rate) in before {
        let after_rate = after.get(layer).copied().unwrap_or(0.0);
        delta.insert(layer.clone(), after_rate - before_rate);
    }
    for (layer, after_rate) in after {
        delta.entry(layer.clone()).or_insert(*after_rate);
    }
    delta
}

/// Diff block-level trajectory stability between `before` and `after` observation sets, matched
/// by `block_id`. Sorted by the largest `dead_unit_delta`/`saturated_unit_delta` magnitude
/// (across all layers in that block) descending — the most-destructive blocks first.
pub fn compute_trajectory_audit(
    before: Vec<Observation>,
    after: Vec<Observation>,
    thresholds: &BlockThresholds,
) -> Vec<TrajectoryAuditEntry> {
    let before_groups = group_by_block_id(before.into_iter());
    let after_groups = group_by_block_id(after.into_iter());

    let mut entries: Vec<TrajectoryAuditEntry> = Vec::new();
    for (block_id, after_obs) in &after_groups {
        let Some(before_obs) = before_groups.get(block_id) else {
            continue;
        };
        let before_report = compute_block_report(block_id, before_obs, thresholds);
        let after_report = compute_block_report(block_id, after_obs, thresholds);

        let dead_unit_delta = per_layer_delta(
            &per_layer_rate(before_obs, |o| o.dead_unit_count),
            &per_layer_rate(after_obs, |o| o.dead_unit_count),
        );
        let saturated_unit_delta = per_layer_delta(
            &per_layer_rate(before_obs, |o| o.saturated_unit_count),
            &per_layer_rate(after_obs, |o| o.saturated_unit_count),
        );
        let trajectory_effect_delta = before_report
            .trajectory_effect_mean
            .zip(after_report.trajectory_effect_mean)
            .map(|(b, a)| a - b);

        let distinct_after_seeds = after_obs
            .iter()
            .filter_map(|o| o.seed)
            .collect::<std::collections::HashSet<_>>()
            .len();
        let reproducibility_3seed = if distinct_after_seeds >= 3 {
            after_report.seed_effect_consistency
        } else {
            None
        };

        entries.push(TrajectoryAuditEntry {
            block_id: block_id.clone(),
            dead_unit_delta,
            saturated_unit_delta,
            trajectory_effect_delta,
            before_classification: before_report.classification,
            after_classification: after_report.classification,
            reproducibility_3seed,
        });
    }

    entries.sort_by(|a, b| {
        max_abs_delta(b)
            .partial_cmp(&max_abs_delta(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.block_id.cmp(&b.block_id))
    });
    entries
}

fn max_abs_delta(e: &TrajectoryAuditEntry) -> f64 {
    e.dead_unit_delta
        .values()
        .chain(e.saturated_unit_delta.values())
        .map(|v| v.abs())
        .fold(0.0_f64, f64::max)
}

/// Warns (returns block_ids, doesn't panic) for any block whose distinct `sample_id` count on
/// either side doesn't match `group_size` — a mismatch signals a malformed block grouping
/// (e.g. a truncated 32-position block) rather than a real destructive-training signal.
pub fn group_size_mismatches(
    before: &[Observation],
    after: &[Observation],
    group_size: usize,
) -> Vec<(String, usize, usize)> {
    let before_groups = group_by_block_id(before.iter().cloned());
    let after_groups = group_by_block_id(after.iter().cloned());
    let mut mismatches = Vec::new();
    for (block_id, obs) in before_groups.iter().chain(after_groups.iter()) {
        let n_samples = obs
            .iter()
            .map(|o| o.sample_id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len();
        if n_samples != group_size
            && !mismatches
                .iter()
                .any(|(id, _, _): &(String, usize, usize)| id == block_id)
        {
            mismatches.push((block_id.clone(), n_samples, group_size));
        }
    }
    mismatches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(
        sample_id: &str,
        block_id: &str,
        layer_id: Option<&str>,
        dead_unit_count: Option<f64>,
        seed: Option<u64>,
        trajectory_effect: Option<f64>,
    ) -> Observation {
        Observation {
            sample_id: sample_id.into(),
            block_id: Some(block_id.into()),
            layer_id: layer_id.map(String::from),
            dead_unit_count,
            seed,
            trajectory_effect,
            ..Default::default()
        }
    }

    #[test]
    fn test_destructive_block_sorts_first() {
        let before = vec![
            obs("p1", "B5", Some("ft"), Some(0.0), Some(1), Some(-0.1)),
            obs("p2", "B5", Some("ft"), Some(0.0), Some(2), Some(-0.1)),
            obs("q1", "B6", Some("ft"), Some(0.0), Some(1), Some(0.1)),
            obs("q2", "B6", Some("ft"), Some(0.0), Some(2), Some(0.1)),
        ];
        let after = vec![
            // B5 develops new dead units on every observation.
            obs("p1", "B5", Some("ft"), Some(5.0), Some(1), Some(-0.5)),
            obs("p2", "B5", Some("ft"), Some(4.0), Some(2), Some(-0.6)),
            // B6 stays clean.
            obs("q1", "B6", Some("ft"), Some(0.0), Some(1), Some(0.2)),
            obs("q2", "B6", Some("ft"), Some(0.0), Some(2), Some(0.3)),
        ];
        let entries = compute_trajectory_audit(before, after, &BlockThresholds::default());
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].block_id, "B5",
            "most-destructive block sorts first"
        );
        assert_eq!(entries[0].dead_unit_delta.get("ft"), Some(&1.0));
        assert_eq!(entries[1].block_id, "B6");
        assert_eq!(entries[1].dead_unit_delta.get("ft"), Some(&0.0));
    }

    #[test]
    fn test_only_matched_blocks_are_reported() {
        let before = vec![obs("p1", "B5", None, None, None, None)];
        let after = vec![obs("p1", "B7", None, None, None, None)];
        let entries = compute_trajectory_audit(before, after, &BlockThresholds::default());
        assert!(entries.is_empty(), "B5/B7 never match -> nothing to diff");
    }

    #[test]
    fn test_reproducibility_3seed_requires_at_least_three_seeds() {
        let two_seed_after = vec![
            obs("p1", "B5", None, None, Some(1), Some(0.3)),
            obs("p2", "B5", None, None, Some(2), Some(0.4)),
        ];
        let before = two_seed_after.clone();
        let entries = compute_trajectory_audit(before, two_seed_after, &BlockThresholds::default());
        assert!(entries[0].reproducibility_3seed.is_none());

        let three_seed_after = vec![
            obs("p1", "B5", None, None, Some(1), Some(0.3)),
            obs("p2", "B5", None, None, Some(2), Some(0.4)),
            obs("p3", "B5", None, None, Some(3), Some(0.5)),
        ];
        let entries = compute_trajectory_audit(
            three_seed_after.clone(),
            three_seed_after,
            &BlockThresholds::default(),
        );
        assert!(entries[0].reproducibility_3seed.is_some());
    }

    #[test]
    fn test_group_size_mismatch_detected() {
        let before = vec![
            obs("p1", "B5", None, None, None, None),
            obs("p2", "B5", None, None, None, None),
        ];
        let after = before.clone();
        let mismatches = group_size_mismatches(&before, &after, 32);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0], ("B5".to_string(), 2, 32));
    }

    #[test]
    fn test_group_size_match_reports_nothing() {
        let obs32: Vec<Observation> = (0..32)
            .map(|i| obs(&format!("p{i}"), "B5", None, None, None, None))
            .collect();
        let mismatches = group_size_mismatches(&obs32, &obs32, 32);
        assert!(mismatches.is_empty());
    }

    // --- edge cases ---

    #[test]
    fn test_empty_before_and_after_produces_no_entries() {
        let entries = compute_trajectory_audit(vec![], vec![], &BlockThresholds::default());
        assert!(entries.is_empty());
    }

    #[test]
    fn test_missing_layer_id_on_one_side_treats_it_as_zero_not_a_crash() {
        // "ft" only appears on the after side -- its before rate must read as 0.0, not panic
        // or silently drop the layer.
        let before = vec![obs("p1", "B5", None, None, Some(1), None)];
        let after = vec![obs("p1", "B5", Some("ft"), Some(3.0), Some(1), None)];
        let entries = compute_trajectory_audit(before, after, &BlockThresholds::default());
        assert_eq!(entries[0].dead_unit_delta.get("ft"), Some(&1.0));
    }

    // --- determinism: entries must not depend on input observation order ---

    #[test]
    fn test_compute_trajectory_audit_is_order_independent() {
        let before = vec![
            obs("p1", "B5", Some("ft"), Some(0.0), Some(1), Some(-0.1)),
            obs("p2", "B5", Some("ft"), Some(0.0), Some(2), Some(-0.1)),
            obs("q1", "B6", Some("ft"), Some(0.0), Some(1), Some(0.1)),
            obs("q2", "B6", Some("ft"), Some(0.0), Some(2), Some(0.1)),
        ];
        let after = vec![
            obs("p1", "B5", Some("ft"), Some(5.0), Some(1), Some(-0.5)),
            obs("p2", "B5", Some("ft"), Some(4.0), Some(2), Some(-0.6)),
            obs("q1", "B6", Some("ft"), Some(0.0), Some(1), Some(0.2)),
            obs("q2", "B6", Some("ft"), Some(0.0), Some(2), Some(0.3)),
        ];
        let mut before_shuffled = before.clone();
        before_shuffled.reverse();
        let mut after_shuffled = after.clone();
        after_shuffled.swap(0, 3);
        after_shuffled.swap(1, 2);

        let thresholds = BlockThresholds::default();
        let a = compute_trajectory_audit(before.clone(), after.clone(), &thresholds);
        let b = compute_trajectory_audit(before_shuffled, after_shuffled, &thresholds);

        // The final sort key (max abs delta desc, then block_id asc) is fully deterministic,
        // so the whole Vec's *order* should match too, not just each block's values.
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.block_id, y.block_id);
            assert_eq!(x.dead_unit_delta, y.dead_unit_delta);
            assert_eq!(x.saturated_unit_delta, y.saturated_unit_delta);
            assert_eq!(x.before_classification, y.before_classification);
            assert_eq!(x.after_classification, y.after_classification);
        }
    }
}
