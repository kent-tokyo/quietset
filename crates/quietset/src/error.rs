use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid score value at line {line}")]
    InvalidScore { line: usize },
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
