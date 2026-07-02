use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use ::quietset::{parse_jsonl, score_all, ScoreConfig};

/// Score a JSONL string of observations and return a JSONL string of StabilityReports.
///
/// Each input line must be a JSON object with at least a ``sample_id`` field.
/// Returns one output line per unique ``sample_id``.
///
/// Example::
///
///     import quietset
///     result = quietset.score_jsonl(
///         '{"sample_id":"a","label":"win","score":0.9}\n'
///         '{"sample_id":"a","label":"win","score":0.8}\n'
///     )
///     print(result)
#[pyfunction]
fn score_jsonl(input: &str) -> PyResult<String> {
    let obs = parse_jsonl(input).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let reports = score_all(obs, &ScoreConfig::default());
    let mut out = String::new();
    for r in &reports {
        out.push_str(
            &serde_json::to_string(r)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        );
        out.push('\n');
    }
    Ok(out)
}

#[pymodule]
fn quietset(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(score_jsonl, m)?)?;
    Ok(())
}
