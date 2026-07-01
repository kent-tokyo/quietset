use crate::observation::Observation;
use crate::schema::StabilityReport;
use indexmap::IndexMap;
use std::collections::HashMap;

/// Compute per-evaluator reliability weights from observations against a truth map.
///
/// `truth` maps `sample_id` → correct label (gold_label if available, else majority_label).
/// Weight = smoothed accuracy: `(matches + 0.5) / (total + 1)`.
/// Evaluators not present in observations get no entry; callers should default to 1.0.
pub fn compute_evaluator_weights(
    observations: &[Observation],
    truth: &HashMap<String, String>,
) -> HashMap<String, f64> {
    let mut counts: HashMap<&str, (usize, usize)> = HashMap::new(); // (matches, total)
    for obs in observations {
        if let (Some(eval_id), Some(label)) = (obs.evaluator_id.as_deref(), obs.label.as_deref())
            && let Some(true_label) = truth.get(&obs.sample_id)
        {
            let entry = counts.entry(eval_id).or_insert((0, 0));
            entry.1 += 1;
            if label == true_label.as_str() {
                entry.0 += 1;
            }
        }
    }
    counts
        .into_iter()
        .map(|(id, (matches, total))| {
            let w = (matches as f64 + 0.5) / (total as f64 + 1.0);
            (id.to_string(), w)
        })
        .collect()
}

/// Compute reliability-weighted majority label for one sample's observations.
///
/// Returns `(weighted_majority_label, weighted_label_confidence, weighted_label_distribution, majority_weighted_conflict)`.
/// Observations without `evaluator_id` use weight 1.0 (neutral).
/// Returns all-None if no labeled observations exist.
#[allow(clippy::type_complexity)]
pub fn compute_weighted_majority(
    obs: &[Observation],
    majority_label: Option<&str>,
    evaluator_weights: &HashMap<String, f64>,
) -> (
    Option<String>,
    Option<f64>,
    Option<IndexMap<String, f64>>,
    Option<bool>,
) {
    let labeled: Vec<(&str, f64)> = obs
        .iter()
        .filter_map(|o| {
            let label = o.label.as_deref()?;
            let w = o
                .evaluator_id
                .as_deref()
                .and_then(|id| evaluator_weights.get(id))
                .copied()
                .unwrap_or(1.0);
            Some((label, w))
        })
        .collect();

    if labeled.is_empty() {
        return (None, None, None, None);
    }

    let mut weighted: HashMap<&str, f64> = HashMap::new();
    for (label, w) in &labeled {
        *weighted.entry(label).or_insert(0.0) += w;
    }

    let total: f64 = weighted.values().sum();
    if total == 0.0 {
        return (None, None, None, None);
    }

    let mut sorted: Vec<(&str, f64)> = weighted.into_iter().collect();
    sorted.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(b.0))
    });

    let wml = sorted[0].0.to_string();
    let wlc = sorted[0].1 / total;
    let wld: IndexMap<String, f64> = sorted
        .iter()
        .map(|(l, w)| (l.to_string(), w / total))
        .collect();
    let conflict = majority_label.map(|ml| ml != wml.as_str());

    (Some(wml), Some(wlc), Some(wld), conflict)
}

/// Compute per-evaluator reliability: fraction of evaluations matching the sample's majority label.
///
/// **Experimental**: reliability is computed against the majority label from the initial scoring,
/// not a gold standard. Results are informational only.
///
/// If an observation has `gold_label` set, it takes priority over the majority label.
pub fn compute_evaluator_reliability(
    observations: &[Observation],
    reports: &[StabilityReport],
) -> HashMap<String, f64> {
    // gold_label takes priority over majority_label for ground truth
    let gold_map: HashMap<&str, &str> = observations
        .iter()
        .filter_map(|o| o.gold_label.as_deref().map(|g| (o.sample_id.as_str(), g)))
        .collect();
    let majority_map: HashMap<&str, &str> = reports
        .iter()
        .filter_map(|r| {
            r.majority_label
                .as_deref()
                .map(|m| (r.sample_id.as_str(), m))
        })
        .collect();

    let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
    for obs in observations {
        if let (Some(eval_id), Some(label)) = (obs.evaluator_id.as_deref(), obs.label.as_deref()) {
            let truth = gold_map
                .get(obs.sample_id.as_str())
                .copied()
                .or_else(|| majority_map.get(obs.sample_id.as_str()).copied());
            if let Some(truth) = truth {
                let entry = counts.entry(eval_id.to_string()).or_insert((0, 0));
                entry.1 += 1;
                if label == truth {
                    entry.0 += 1;
                }
            }
        }
    }

    counts
        .into_iter()
        .map(|(id, (matches, total))| {
            (
                id,
                if total > 0 {
                    matches as f64 / total as f64
                } else {
                    0.0
                },
            )
        })
        .collect()
}
