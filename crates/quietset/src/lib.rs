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
pub use metrics::{compute_report, score_all, ScoreConfig, ScoreWeights};
pub use observation::{parse_csv, parse_jsonl, Observation};
pub use schema::{Decision, StabilityReport};
pub use stream::StreamingScorer;
