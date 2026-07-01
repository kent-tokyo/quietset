use crate::observation::Observation;
use std::collections::HashMap;

/// Fleiss' kappa: inter-rater agreement corrected for chance (nominal labels, variable raters).
///
/// Returns `None` when fewer than 2 subjects have at least 2 ratings each (undefined).
/// Range: −1.0 to 1.0 (1.0 = perfect agreement, 0.0 = chance level).
pub fn compute_fleiss_kappa(observations: &[Observation]) -> Option<f64> {
    // Build per-subject label counts
    let mut subjects: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for obs in observations {
        if let Some(label) = obs.label.as_deref() {
            *subjects
                .entry(obs.sample_id.as_str())
                .or_default()
                .entry(label)
                .or_insert(0) += 1;
        }
    }

    let mut total_per_cat: HashMap<&str, usize> = HashMap::new();
    let mut total_ratings = 0usize;
    let mut p_bar = 0.0f64;
    let mut valid = 0usize;

    for counts in subjects.values() {
        let n_i: usize = counts.values().sum();
        if n_i < 2 {
            continue;
        }
        let observed: f64 =
            counts.values().map(|&c| (c * (c - 1)) as f64).sum::<f64>() / (n_i * (n_i - 1)) as f64;
        p_bar += observed;
        valid += 1;
        for (&cat, &cnt) in counts {
            *total_per_cat.entry(cat).or_insert(0) += cnt;
            total_ratings += cnt;
        }
    }

    if valid < 2 || total_ratings == 0 {
        return None;
    }
    p_bar /= valid as f64;

    let p_e: f64 = total_per_cat
        .values()
        .map(|&c| (c as f64 / total_ratings as f64).powi(2))
        .sum();

    if (1.0 - p_e).abs() < 1e-10 {
        return Some(1.0);
    }
    Some((p_bar - p_e) / (1.0 - p_e))
}

/// Krippendorff's alpha: reliability coefficient for nominal labels, variable raters.
///
/// Uses the coincidence-matrix formulation. Returns `None` when there are fewer than
/// 2 units with at least 2 ratings (undefined).
/// Range: −1.0 to 1.0 (1.0 = perfect agreement, 0.0 = chance level).
pub fn compute_krippendorff_alpha(observations: &[Observation]) -> Option<f64> {
    let mut subjects: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for obs in observations {
        if let Some(label) = obs.label.as_deref() {
            *subjects
                .entry(obs.sample_id.as_str())
                .or_default()
                .entry(label)
                .or_insert(0) += 1;
        }
    }

    // Marginals: n_c[cat] = total count across subjects with n_k >= 2
    let mut n_c: HashMap<&str, usize> = HashMap::new();
    let mut e_diag = 0.0f64; // sum of coincidence-matrix diagonal
    let mut n_total = 0usize;
    let mut valid = 0usize;

    for counts in subjects.values() {
        let n_k: usize = counts.values().sum();
        if n_k < 2 {
            continue;
        }
        valid += 1;
        let denom = (n_k - 1) as f64;
        for (&cat, &cnt) in counts {
            e_diag += (cnt * (cnt - 1)) as f64 / denom;
            *n_c.entry(cat).or_insert(0) += cnt;
            n_total += cnt;
        }
    }

    if valid < 2 || n_total < 2 {
        return None;
    }
    let n = n_total as f64;

    // Observed disagreement (nominal: d=1 for c≠c')
    let do_ = 1.0 - e_diag / n;
    // Expected disagreement
    let sum_nc_sq: f64 = n_c.values().map(|&c| (c * c) as f64).sum();
    let de = (n * n - sum_nc_sq) / (n * (n - 1.0));

    if de.abs() < 1e-10 {
        return Some(1.0);
    }
    Some(1.0 - do_ / de)
}
