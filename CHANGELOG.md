# Changelog

## Unreleased

## 0.1.0 — 2026-06-28

### Added

**CLI commands**
- `quietset score` — stability scoring with JSONL/CSV input, JSONL/CSV output
- `quietset filter` — filter by stability score, disagreement, or decision
- `quietset summary` — aggregate stats (counts, percentiles, instability drivers); `--json` for CI
- `quietset explain` — per-sample component breakdown with weakness highlighting; `--json`
- `quietset compare` — decision transition matrix and regressions between two runs; `--json`
- `quietset reliability` — per-evaluator reliability from observation JSONL (experimental)

**Library types**
- `Observation`, `StabilityReport`, `Decision`, `StabilityComponents`
- `ScoreConfig` with `ScoreWeights`, `MinRequirements`, `DecisionScore`, `confidence_k`
- `DecisionScore::Raw|Adjusted` — decision logic fully in library; `MinRequirements` always applied after threshold comparison and cannot be overridden
- `confidence` and `adjusted_stability_score` on every `StabilityReport`
- `label_margin` and `label_entropy` for precise label disagreement detection
- `budget_slope` — score trend across compute budget levels
- `compute_evaluator_reliability()` — experimental per-evaluator trust score
- `StreamingScorer` — single-pass scoring over pre-sorted observations
- `ScoreConfig::validate()` — rejects invalid `score_scale`, thresholds, weights, `confidence_k`
- `Observation::validate()` — shared validation for `sample_id`, `score`, `budget`

**CLI flags on `score`**
- `--confidence-k`, `--use-adjusted-score`
- `--min-observations-keep`, `--min-evaluators-keep`, `--min-seeds-keep`, `--min-budgets-keep`, `--min-models-keep`
- `--weight-labels/scores/budget/seed/models/evaluators`
- `--output-format csv`, `--skip-invalid`, `--estimate-evaluator-reliability`

**Infrastructure**
- GitHub Actions CI (`fmt`, `clippy`, `test`, `doc`, `rustsec/audit-check`)
- Python bindings skeleton (`crates/quietset-py` via pyo3 + maturin, experimental)

### Fixed

- `seed_sensitivity` is now included in `stability_score` (was computed but silently excluded)
- `sample_id` missing or empty returns `Error::MissingField` instead of silently using `""`
- `majority_label` tie-breaking is deterministic: alphabetically first label wins on equal counts
- `--skip-invalid` validates observations; cannot bypass empty `sample_id` or non-finite fields
- `score` and `budget` NaN/infinite values return explicit typed errors
- `--use-adjusted-score` can no longer override `MinRequirements` demotion (decision unified in library)
