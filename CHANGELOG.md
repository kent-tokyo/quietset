# Changelog

## Unreleased

## 0.1.0 — 2026-06-28

### Added

- `Observation`, `StabilityReport`, `Decision`, `ScoreConfig`, `ScoreWeights` — core library types
- `quietset score` — score observation JSONL/CSV and output `StabilityReport` JSONL/CSV
- `quietset filter` — filter scored JSONL by stability, disagreement, or decision
- `quietset summary` — aggregate stats with instability drivers; `--json` for CI use
- `quietset explain` — per-sample component breakdown with weakness highlighting; `--json`
- `quietset compare` — decision transition matrix and regressions across two runs; `--json`
- `quietset reliability` — per-evaluator reliability from observation JSONL (experimental)
- `StabilityComponents` — per-dimension sub-scores on every `StabilityReport`
- `confidence` and `adjusted_stability_score` fields; `--confidence-k` / `--use-adjusted-score` CLI flags
- `DecisionScore::Raw|Adjusted` in `ScoreConfig` — decision logic fully inside library; `MinRequirements` always applies after threshold comparison and cannot be overridden
- `MinRequirements` in `ScoreConfig` — demotes Keep when evidence is thin; `--min-*-keep` CLI flags
- `label_margin` and `label_entropy` for precise label disagreement detection
- `budget_slope` — score trend across compute budget levels
- `compute_evaluator_reliability()` — experimental per-evaluator trust score
- `ScoreWeights` — per-dimension weighting; `--weight-*` CLI flags
- `ScoreConfig::validate()` — rejects invalid `score_scale`, thresholds, weights, and `confidence_k`
- `Observation::validate()` — shared validation for `sample_id`, `score`, and `budget`
- `StreamingScorer` — single-pass scoring over pre-sorted observations
- JSONL and CSV input; JSONL and CSV output with full component columns
- GitHub Actions CI (`fmt`, `clippy`, `test`, `doc`, `rustsec/audit-check`)
- Python bindings skeleton (`crates/quietset-py` via pyo3 + maturin, experimental)

### Fixed

- `seed_sensitivity` included in `stability_score` (was computed but excluded)
- `sample_id` missing/empty returns `Error::MissingField` instead of silently using `""`
- `majority_label` tie-breaking deterministic: alphabetically first label wins on equal counts
- `--skip-invalid` JSONL path validates observations; cannot bypass empty `sample_id` or non-finite fields
- `score` and `budget` NaN/infinite values return explicit typed errors
