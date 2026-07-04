use crate::observation::Observation;
use indexmap::IndexMap;
use ordered_float::OrderedFloat;
use std::collections::{HashMap, HashSet};

/// One `evaluator_id` value's stable-wrong stats among `Keep`-decision samples that include at
/// least one observation with this `evaluator_id`. `n_keep` here counts samples *touching this
/// evaluator specifically* — not the same quantity as the top-level `n_keep`, and rows across
/// `by_evaluator` do not sum to it: a sample with multiple evaluators contributes to more than
/// one row's `n_keep` simultaneously. This is deliberate — the breakdown answers "which
/// evaluators show up in wrong-but-confident samples," not a blame partition.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatorStableWrongRate {
    pub evaluator_id: String,
    pub n_keep: usize,
    pub n_stable_wrong: usize,
    pub stable_wrong_rate: f64,
}

/// Same semantics as [`EvaluatorStableWrongRate`], keyed by `model_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelStableWrongRate {
    pub model_id: String,
    pub n_keep: usize,
    pub n_stable_wrong: usize,
    pub stable_wrong_rate: f64,
}

/// Same semantics as [`EvaluatorStableWrongRate`], keyed by `budget`.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetStableWrongRate {
    pub budget: f64,
    pub n_keep: usize,
    pub n_stable_wrong: usize,
    pub stable_wrong_rate: f64,
}

/// `stable_wrong_rate_among_keep`, broken down by `evaluator_id`/`model_id`/`budget`. See
/// [`EvaluatorStableWrongRate`] for the non-partition caveat that applies to all three arrays.
#[derive(Debug, Clone, Default)]
pub struct StableWrongBreakdown {
    pub by_evaluator: Vec<EvaluatorStableWrongRate>,
    pub by_model: Vec<ModelStableWrongRate>,
    pub by_budget: Vec<BudgetStableWrongRate>,
}

/// Computes [`StableWrongBreakdown`] from already-scored samples' raw observations.
///
/// `observations_by_sample` maps each `Keep`-or-not sample_id to its original observations
/// (e.g. via [`crate::group::group_by_sample_id`]); `keep_sample_ids` and
/// `stable_wrong_sample_ids` identify which sample_ids are `Decision::Keep` and which of those
/// are stably wrong (majority/consensus label disagrees with `gold_label`) — computed by the
/// caller, since the exact consensus-label fallback chain and gold lookup already live there.
///
/// For each dimension, a sample contributes at most once per distinct key value it touches
/// (an evaluator with 5 observations on one sample still counts as 1 toward that key's
/// `n_keep`). A key value that never appears on any `Keep` sample is omitted entirely — not
/// shown as a vacuous `0/0` row.
pub fn stable_wrong_breakdown(
    observations_by_sample: &IndexMap<String, Vec<Observation>>,
    keep_sample_ids: &HashSet<String>,
    stable_wrong_sample_ids: &HashSet<String>,
) -> StableWrongBreakdown {
    let mut evaluator_keep: HashMap<String, usize> = HashMap::new();
    let mut evaluator_wrong: HashMap<String, usize> = HashMap::new();
    let mut model_keep: HashMap<String, usize> = HashMap::new();
    let mut model_wrong: HashMap<String, usize> = HashMap::new();
    let mut budget_keep: HashMap<OrderedFloat<f64>, usize> = HashMap::new();
    let mut budget_wrong: HashMap<OrderedFloat<f64>, usize> = HashMap::new();

    for sample_id in keep_sample_ids {
        let Some(obs) = observations_by_sample.get(sample_id) else {
            continue;
        };
        let is_wrong = stable_wrong_sample_ids.contains(sample_id);

        let evaluators: HashSet<&str> = obs
            .iter()
            .filter_map(|o| o.evaluator_id.as_deref())
            .collect();
        for e in evaluators {
            *evaluator_keep.entry(e.to_string()).or_insert(0) += 1;
            if is_wrong {
                *evaluator_wrong.entry(e.to_string()).or_insert(0) += 1;
            }
        }

        let models: HashSet<&str> = obs.iter().filter_map(|o| o.model_id.as_deref()).collect();
        for m in models {
            *model_keep.entry(m.to_string()).or_insert(0) += 1;
            if is_wrong {
                *model_wrong.entry(m.to_string()).or_insert(0) += 1;
            }
        }

        let budgets: HashSet<OrderedFloat<f64>> = obs
            .iter()
            .filter_map(|o| o.budget.map(OrderedFloat))
            .collect();
        for b in budgets {
            *budget_keep.entry(b).or_insert(0) += 1;
            if is_wrong {
                *budget_wrong.entry(b).or_insert(0) += 1;
            }
        }
    }

    let mut by_evaluator: Vec<EvaluatorStableWrongRate> = evaluator_keep
        .into_iter()
        .map(|(evaluator_id, n_keep)| {
            let n_stable_wrong = evaluator_wrong.get(&evaluator_id).copied().unwrap_or(0);
            EvaluatorStableWrongRate {
                stable_wrong_rate: n_stable_wrong as f64 / n_keep as f64,
                evaluator_id,
                n_keep,
                n_stable_wrong,
            }
        })
        .collect();
    by_evaluator.sort_by(|a, b| {
        b.stable_wrong_rate
            .partial_cmp(&a.stable_wrong_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.evaluator_id.cmp(&b.evaluator_id))
    });

    let mut by_model: Vec<ModelStableWrongRate> = model_keep
        .into_iter()
        .map(|(model_id, n_keep)| {
            let n_stable_wrong = model_wrong.get(&model_id).copied().unwrap_or(0);
            ModelStableWrongRate {
                stable_wrong_rate: n_stable_wrong as f64 / n_keep as f64,
                model_id,
                n_keep,
                n_stable_wrong,
            }
        })
        .collect();
    by_model.sort_by(|a, b| {
        b.stable_wrong_rate
            .partial_cmp(&a.stable_wrong_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.model_id.cmp(&b.model_id))
    });

    let mut by_budget: Vec<BudgetStableWrongRate> = budget_keep
        .into_iter()
        .map(|(budget, n_keep)| {
            let n_stable_wrong = budget_wrong.get(&budget).copied().unwrap_or(0);
            BudgetStableWrongRate {
                stable_wrong_rate: n_stable_wrong as f64 / n_keep as f64,
                budget: budget.into_inner(),
                n_keep,
                n_stable_wrong,
            }
        })
        .collect();
    by_budget.sort_by(|a, b| {
        b.stable_wrong_rate
            .partial_cmp(&a.stable_wrong_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.budget
                    .partial_cmp(&b.budget)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    StableWrongBreakdown {
        by_evaluator,
        by_model,
        by_budget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(sample_id: &str, evaluator_id: Option<&str>, budget: Option<f64>) -> Observation {
        Observation {
            sample_id: sample_id.into(),
            evaluator_id: evaluator_id.map(String::from),
            budget,
            ..Default::default()
        }
    }

    #[test]
    fn test_hand_derived_evaluator_and_model_rates() {
        // s1, s2: evaluator m1 / model mod1, both Keep, s1 is stable-wrong.
        // s3: evaluator m2 / model mod2, Keep, correct.
        let mut observations_by_sample = IndexMap::new();
        observations_by_sample.insert(
            "s1".to_string(),
            vec![Observation {
                model_id: Some("mod1".into()),
                ..obs("s1", Some("m1"), None)
            }],
        );
        observations_by_sample.insert(
            "s2".to_string(),
            vec![Observation {
                model_id: Some("mod1".into()),
                ..obs("s2", Some("m1"), None)
            }],
        );
        observations_by_sample.insert(
            "s3".to_string(),
            vec![Observation {
                model_id: Some("mod2".into()),
                ..obs("s3", Some("m2"), None)
            }],
        );
        let keep_sample_ids: HashSet<String> =
            ["s1", "s2", "s3"].iter().map(|s| s.to_string()).collect();
        let stable_wrong_sample_ids: HashSet<String> =
            ["s1"].iter().map(|s| s.to_string()).collect();

        let breakdown = stable_wrong_breakdown(
            &observations_by_sample,
            &keep_sample_ids,
            &stable_wrong_sample_ids,
        );

        assert_eq!(
            breakdown.by_evaluator,
            vec![
                EvaluatorStableWrongRate {
                    evaluator_id: "m1".into(),
                    n_keep: 2,
                    n_stable_wrong: 1,
                    stable_wrong_rate: 0.5,
                },
                EvaluatorStableWrongRate {
                    evaluator_id: "m2".into(),
                    n_keep: 1,
                    n_stable_wrong: 0,
                    stable_wrong_rate: 0.0,
                },
            ]
        );
        assert_eq!(
            breakdown.by_model,
            vec![
                ModelStableWrongRate {
                    model_id: "mod1".into(),
                    n_keep: 2,
                    n_stable_wrong: 1,
                    stable_wrong_rate: 0.5,
                },
                ModelStableWrongRate {
                    model_id: "mod2".into(),
                    n_keep: 1,
                    n_stable_wrong: 0,
                    stable_wrong_rate: 0.0,
                },
            ]
        );
        assert!(breakdown.by_budget.is_empty());
    }

    #[test]
    fn test_hand_derived_budget_rates_with_dedup() {
        // s1: two observations both budget=4.0 (should dedup to a single count), Keep, wrong.
        // s2: budget=8.0, Keep, correct.
        let mut observations_by_sample = IndexMap::new();
        observations_by_sample.insert(
            "s1".to_string(),
            vec![obs("s1", None, Some(4.0)), obs("s1", None, Some(4.0))],
        );
        observations_by_sample.insert("s2".to_string(), vec![obs("s2", None, Some(8.0))]);
        let keep_sample_ids: HashSet<String> = ["s1", "s2"].iter().map(|s| s.to_string()).collect();
        let stable_wrong_sample_ids: HashSet<String> =
            ["s1"].iter().map(|s| s.to_string()).collect();

        let breakdown = stable_wrong_breakdown(
            &observations_by_sample,
            &keep_sample_ids,
            &stable_wrong_sample_ids,
        );

        assert_eq!(
            breakdown.by_budget,
            vec![
                BudgetStableWrongRate {
                    budget: 4.0,
                    n_keep: 1,
                    n_stable_wrong: 1,
                    stable_wrong_rate: 1.0,
                },
                BudgetStableWrongRate {
                    budget: 8.0,
                    n_keep: 1,
                    n_stable_wrong: 0,
                    stable_wrong_rate: 0.0,
                },
            ]
        );
    }

    #[test]
    fn test_non_keep_samples_never_contribute_no_vacuous_rows() {
        let mut observations_by_sample = IndexMap::new();
        observations_by_sample.insert(
            "s_review".to_string(),
            vec![obs("s_review", Some("m3"), None)],
        );
        let keep_sample_ids: HashSet<String> = HashSet::new(); // s_review is not Keep
        let stable_wrong_sample_ids: HashSet<String> = HashSet::new();

        let breakdown = stable_wrong_breakdown(
            &observations_by_sample,
            &keep_sample_ids,
            &stable_wrong_sample_ids,
        );

        assert!(
            breakdown.by_evaluator.is_empty(),
            "m3 never appears on a Keep sample, so it should be absent, not a 0/0 row"
        );
    }

    #[test]
    fn test_multi_evaluator_sample_contributes_to_both_non_partition() {
        let mut observations_by_sample = IndexMap::new();
        observations_by_sample.insert(
            "s1".to_string(),
            vec![obs("s1", Some("m1"), None), obs("s1", Some("m2"), None)],
        );
        let keep_sample_ids: HashSet<String> = ["s1"].iter().map(|s| s.to_string()).collect();
        let stable_wrong_sample_ids: HashSet<String> = HashSet::new();

        let breakdown = stable_wrong_breakdown(
            &observations_by_sample,
            &keep_sample_ids,
            &stable_wrong_sample_ids,
        );

        let total_n_keep: usize = breakdown.by_evaluator.iter().map(|e| e.n_keep).sum();
        assert_eq!(
            total_n_keep, 2,
            "one Keep sample touching 2 evaluators contributes to both rows' n_keep, so the \
             sum (2) exceeds the actual number of Keep samples (1) -- not a partition"
        );
    }

    #[test]
    fn test_sort_order_ties_break_by_key_ascending() {
        let mut observations_by_sample = IndexMap::new();
        observations_by_sample.insert("s1".to_string(), vec![obs("s1", Some("zeta"), None)]);
        observations_by_sample.insert("s2".to_string(), vec![obs("s2", Some("alpha"), None)]);
        let keep_sample_ids: HashSet<String> = ["s1", "s2"].iter().map(|s| s.to_string()).collect();
        let stable_wrong_sample_ids: HashSet<String> = HashSet::new(); // both rate 0.0, tied

        let breakdown = stable_wrong_breakdown(
            &observations_by_sample,
            &keep_sample_ids,
            &stable_wrong_sample_ids,
        );

        let ids: Vec<&str> = breakdown
            .by_evaluator
            .iter()
            .map(|e| e.evaluator_id.as_str())
            .collect();
        assert_eq!(ids, vec!["alpha", "zeta"]);
    }
}
