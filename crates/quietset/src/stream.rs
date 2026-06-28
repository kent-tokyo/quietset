use crate::metrics::{ScoreConfig, compute_report};
use crate::observation::Observation;
use crate::schema::StabilityReport;

/// Scores observations in a single pass when they are pre-sorted by `sample_id`.
///
/// Call [`push`](StreamingScorer::push) for each observation in order.
/// It returns `Some(report)` whenever the `sample_id` changes (completing the previous group).
/// After the last observation, call [`flush`](StreamingScorer::flush) to get the final report.
///
/// # Example
/// ```
/// use quietset::{Observation, ScoreConfig, StreamingScorer};
///
/// let mut scorer = StreamingScorer::new(ScoreConfig::default());
/// let obs = vec![
///     Observation { sample_id: "a".into(), score: Some(0.9), ..Default::default() },
///     Observation { sample_id: "a".into(), score: Some(0.8), ..Default::default() },
///     Observation { sample_id: "b".into(), score: Some(0.5), ..Default::default() },
/// ];
/// let mut reports = Vec::new();
/// for o in obs {
///     if let Some(r) = scorer.push(o) { reports.push(r); }
/// }
/// if let Some(r) = scorer.flush() { reports.push(r); }
/// assert_eq!(reports.len(), 2);
/// ```
pub struct StreamingScorer {
    config: ScoreConfig,
    current_id: Option<String>,
    buffer: Vec<Observation>,
}

impl StreamingScorer {
    /// Create a new `StreamingScorer` with the given configuration.
    pub fn new(config: ScoreConfig) -> Self {
        Self {
            config,
            current_id: None,
            buffer: Vec::new(),
        }
    }

    /// Feed one observation. Returns a completed report when the sample group changes.
    ///
    /// Observations **must** be sorted by `sample_id` for correct results.
    pub fn push(&mut self, obs: Observation) -> Option<StabilityReport> {
        if self.current_id.as_deref() == Some(obs.sample_id.as_str()) {
            self.buffer.push(obs);
            None
        } else {
            let result = self.flush_inner();
            self.current_id = Some(obs.sample_id.clone());
            self.buffer.push(obs);
            result
        }
    }

    /// Flush the current buffer and return the last group's report (if any observations remain).
    pub fn flush(&mut self) -> Option<StabilityReport> {
        self.flush_inner()
    }

    fn flush_inner(&mut self) -> Option<StabilityReport> {
        if self.buffer.is_empty() {
            return None;
        }
        let id = self.current_id.take().unwrap();
        let report = compute_report(&id, &self.buffer, &self.config);
        self.buffer.clear();
        Some(report)
    }
}
