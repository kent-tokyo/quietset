use serde::{Deserialize, Serialize};

/// A single evaluation run for one sample.
///
/// All fields except `sample_id` are optional — provide only what your
/// pipeline produces. The library uses whichever fields are present to
/// compute the relevant stability sub-scores.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Observation {
    /// Identifies the sample across all its repeated evaluations.
    #[serde(default)]
    pub sample_id: String,
    /// Categorical label assigned in this evaluation run.
    pub label: Option<String>,
    /// Numeric evaluation score for this run.
    pub score: Option<f64>,
    /// Identifies the evaluator (model, human annotator, or tool).
    pub evaluator_id: Option<String>,
    /// Compute budget consumed by this evaluation (e.g. search depth, token count).
    pub budget: Option<f64>,
    /// Random seed used for this run.
    pub seed: Option<u64>,
    /// Model checkpoint or version that produced this evaluation.
    pub model_id: Option<String>,
    /// Unique identifier for this evaluation run.
    pub run_id: Option<String>,
    /// Known correct label for this sample. When provided, `quietset reliability` uses it
    /// instead of majority label to compute evaluator reliability.
    pub gold_label: Option<String>,
    /// Second seed axis (data-order/shuffle seed), distinct from `seed` (init seed).
    pub shuffle_seed: Option<u64>,
    /// Loss-function or training-recipe variant used for this run. Same role as `model_id`/
    /// `evaluator_id`: a categorical condition axis.
    pub loss_recipe: Option<String>,
    /// Groups this sample into a training block (e.g. a 32-position block) for block-level
    /// stability analysis, distinct from `sample_id`.
    pub block_id: Option<String>,
    /// Generic layer identifier for `dead_unit_count`/`saturated_unit_count`. quietset assigns
    /// no meaning to specific values — the caller's training harness decides what "ft", "l2",
    /// or any other layer name means.
    pub layer_id: Option<String>,
    /// Signed per-run scalar whose sign indicates gradient direction for this sample. Only the
    /// sign is used (mirrors `score`'s role in `score_sign_agreement`).
    pub gradient_sign: Option<f64>,
    /// Per-run cosine similarity between this sample's parameter-update contribution and a
    /// reference direction (e.g. the mean update direction across seeds), computed upstream.
    pub update_cosine: Option<f64>,
    /// Per-run signed scalar capturing disagreement with a teacher/reference model (e.g.
    /// model output minus teacher output).
    pub teacher_residual: Option<f64>,
    /// Signed per-run scalar driving growth/shrink classification (e.g. a radial-norm delta).
    /// Positive = growth, negative = shrink.
    pub trajectory_effect: Option<f64>,
    /// Count of newly-dead units in `layer_id` observed for this (sample, run).
    pub dead_unit_count: Option<f64>,
    /// Count of newly-saturated units in `layer_id` observed for this (sample, run).
    pub saturated_unit_count: Option<f64>,
}

impl Observation {
    /// Validate required and numeric fields. `line` is reported in error messages.
    pub fn validate(&self, line: usize) -> crate::error::Result<()> {
        if self.sample_id.trim().is_empty() {
            return Err(crate::error::Error::MissingField("sample_id"));
        }
        if self.score.is_some_and(|s| !s.is_finite()) {
            return Err(crate::error::Error::InvalidScore { line });
        }
        if self.budget.is_some_and(|b| !b.is_finite()) {
            return Err(crate::error::Error::InvalidBudget { line });
        }
        for (field, value) in [
            ("gradient_sign", self.gradient_sign),
            ("update_cosine", self.update_cosine),
            ("teacher_residual", self.teacher_residual),
            ("trajectory_effect", self.trajectory_effect),
            ("dead_unit_count", self.dead_unit_count),
            ("saturated_unit_count", self.saturated_unit_count),
        ] {
            if value.is_some_and(|v| !v.is_finite()) {
                return Err(crate::error::Error::InvalidField { field, line });
            }
        }
        Ok(())
    }
}

/// Parse observations from a JSONL string, returning typed errors with line numbers.
pub fn parse_jsonl(input: &str) -> crate::error::Result<Vec<Observation>> {
    let mut out = Vec::new();
    for (i, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let obs: Observation =
            serde_json::from_str(line).map_err(|source| crate::error::Error::ParseError {
                line: i + 1,
                source,
            })?;
        obs.validate(i + 1)?;
        out.push(obs);
    }
    Ok(out)
}

/// Parse observations from CSV bytes.
pub fn parse_csv(input: &[u8]) -> crate::error::Result<Vec<Observation>> {
    let mut rdr = csv::Reader::from_reader(input);
    let mut out = Vec::new();
    for (i, record) in rdr.deserialize::<Observation>().enumerate() {
        let obs = record?;
        obs.validate(i + 1)?;
        out.push(obs);
    }
    Ok(out)
}
