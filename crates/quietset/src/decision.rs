use crate::schema::Decision;

/// Stability-score thresholds that determine the [`Decision`] for each sample.
pub struct Thresholds {
    /// Samples with `stability_score >= keep` are labelled [`Decision::Keep`]. Default `0.85`.
    pub keep: f64,
    /// Samples with `stability_score <= drop` are labelled [`Decision::Drop`]. Default `0.40`.
    pub drop: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            keep: 0.85,
            drop: 0.40,
        }
    }
}

/// Map a `stability_score` to a [`Decision`] using the given thresholds.
pub fn decide(stability_score: f64, thresholds: &Thresholds) -> Decision {
    if stability_score >= thresholds.keep {
        Decision::Keep
    } else if stability_score <= thresholds.drop {
        Decision::Drop
    } else {
        Decision::Review
    }
}
