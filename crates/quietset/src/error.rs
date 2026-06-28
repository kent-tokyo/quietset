use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid score value at line {line}: must be finite")]
    InvalidScore { line: usize },
    #[error("invalid budget value at line {line}: must be finite")]
    InvalidBudget { line: usize },
    #[error("score_scale must be positive and finite, got {0}")]
    InvalidScoreScale(f64),
    #[error("invalid threshold: {0}")]
    InvalidThreshold(String),
    #[error("invalid weight for '{name}': must be finite and >= 0.0, got {value}")]
    InvalidWeight { name: &'static str, value: f64 },
    #[error("all stability_score weights are zero; at least one must be > 0.0")]
    AllWeightsZero,
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("could not parse JSONL at line {line}: {source}")]
    ParseError {
        line: usize,
        source: serde_json::Error,
    },
    #[error("could not parse CSV: {0}")]
    CsvError(#[from] csv::Error),
    #[error("no observations found")]
    NoObservations,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
