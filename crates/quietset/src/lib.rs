//! quietset — filter datasets by label stability.
//!
//! # Quick start
//!
//! ```rust
//! use quietset::{Observation, ScoreConfig, score_all};
//!
//! let obs = vec![
//!     Observation { sample_id: "a".into(), label: Some("win".into()), score: Some(0.9), ..Default::default() },
//!     Observation { sample_id: "a".into(), label: Some("win".into()), score: Some(0.88), ..Default::default() },
//! ];
//! let reports = score_all(obs, &ScoreConfig::default());
//! assert_eq!(reports[0].decision, quietset::Decision::Keep);
//! ```

pub mod decision;
pub mod error;
pub mod group;
pub mod metrics;
pub mod observation;
pub mod schema;
pub mod stream;

pub use decision::Thresholds;
pub use error::{Error, Result};
pub use metrics::{
    MinRequirements, ScoreConfig, ScoreWeights, compute_evaluator_reliability, compute_report,
    score_all,
};
pub use observation::{Observation, parse_csv, parse_jsonl};
pub use schema::{Decision, StabilityComponents, StabilityReport};
pub use stream::StreamingScorer;
