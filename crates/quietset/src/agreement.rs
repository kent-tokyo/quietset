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

/// Ranks `values`, assigning the average rank (1-indexed) to tied values — standard tie
/// handling for Spearman's rank correlation.
fn average_ranks(values: &[f64]) -> Vec<f64> {
    let mut indexed: Vec<(usize, f64)> = values.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0; values.len()];
    let mut i = 0;
    while i < indexed.len() {
        let mut j = i;
        while j + 1 < indexed.len() && indexed[j + 1].1 == indexed[i].1 {
            j += 1;
        }
        // Tied group occupies 1-indexed positions i+1..=j+1; all get the average of those.
        let avg_rank = ((i + 1) + (j + 1)) as f64 / 2.0;
        for item in indexed.iter().take(j + 1).skip(i) {
            ranks[item.0] = avg_rank;
        }
        i = j + 1;
    }
    ranks
}

/// Pearson correlation coefficient of two equal-length slices. `None` if fewer than 2 values,
/// mismatched lengths, or either slice has zero variance (undefined correlation).
fn pearson_correlation(xs: &[f64], ys: &[f64]) -> Option<f64> {
    let n = xs.len();
    if n < 2 || n != ys.len() {
        return None;
    }
    let mean_x = xs.iter().sum::<f64>() / n as f64;
    let mean_y = ys.iter().sum::<f64>() / n as f64;
    let (mut cov, mut var_x, mut var_y) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let dx = xs[i] - mean_x;
        let dy = ys[i] - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }
    if var_x <= 0.0 || var_y <= 0.0 {
        return None;
    }
    Some(cov / (var_x.sqrt() * var_y.sqrt()))
}

/// Spearman's rank correlation coefficient between paired values, using the general
/// average-rank tie handling (the simplified `1 - 6·Σd²/(n(n²-1))` shortcut assumes no ties and
/// silently misbehaves when values repeat). `None` if fewer than 2 pairs, or if either side has
/// zero variance (e.g. every value identical) — the correlation is undefined.
///
/// Range: -1.0 to 1.0 (1.0 = identical rank order, -1.0 = perfectly reversed, 0.0 = no
/// association).
pub fn compute_spearman_rank_correlation(pairs: &[(f64, f64)]) -> Option<f64> {
    if pairs.len() < 2 {
        return None;
    }
    let xs: Vec<f64> = pairs.iter().map(|(x, _)| *x).collect();
    let ys: Vec<f64> = pairs.iter().map(|(_, y)| *y).collect();
    pearson_correlation(&average_ranks(&xs), &average_ranks(&ys))
}

#[cfg(test)]
mod tests {
    use super::compute_spearman_rank_correlation;

    #[test]
    fn test_identical_rank_order_is_perfect_positive() {
        let pairs = [(1.0, 10.0), (2.0, 20.0), (3.0, 30.0), (4.0, 40.0)];
        assert!((compute_spearman_rank_correlation(&pairs).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_reversed_rank_order_is_perfect_negative() {
        let pairs = [(1.0, 40.0), (2.0, 30.0), (3.0, 20.0), (4.0, 10.0)];
        assert!((compute_spearman_rank_correlation(&pairs).unwrap() - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn test_ties_use_average_rank_not_arbitrary_order() {
        // x has a tie at positions 2,3 (both value 2.0) -> average rank 2.5 each.
        let pairs = [(1.0, 1.0), (2.0, 2.0), (2.0, 3.0), (4.0, 4.0)];
        let rho = compute_spearman_rank_correlation(&pairs).unwrap();
        assert!(
            rho > 0.9,
            "near-monotonic data with one tie should stay strongly positive, got {rho}"
        );
    }

    #[test]
    fn test_fewer_than_two_pairs_is_none() {
        assert_eq!(compute_spearman_rank_correlation(&[]), None);
        assert_eq!(compute_spearman_rank_correlation(&[(1.0, 1.0)]), None);
    }

    #[test]
    fn test_zero_variance_side_is_none() {
        // every y identical -> undefined correlation
        let pairs = [(1.0, 5.0), (2.0, 5.0), (3.0, 5.0)];
        assert_eq!(compute_spearman_rank_correlation(&pairs), None);
    }

    #[test]
    fn test_result_is_independent_of_pair_order() {
        // Spearman's rho is a symmetric function of the pair set -- shuffling which pair comes
        // first must not change the result (unlike per-sample block-level aggregates, there is
        // no grouping-by-insertion-order step here to complicate this).
        let pairs = [
            (1.0, 10.0),
            (2.0, 2.0),
            (3.0, 30.0),
            (2.0, 25.0),
            (5.0, 5.0),
        ];
        let mut shuffled = pairs;
        shuffled.reverse();
        let a = compute_spearman_rank_correlation(&pairs).unwrap();
        let b = compute_spearman_rank_correlation(&shuffled).unwrap();
        // Epsilon, not exact equality: floating-point summation isn't associative, so a
        // different accumulation order can differ at the ULP level for the same math result.
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }
}
