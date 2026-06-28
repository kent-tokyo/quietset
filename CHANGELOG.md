# Changelog

## Unreleased

### Fixed

- `seed_sensitivity` is now included in `stability_score` computation (was computed but silently excluded)
- `sample_id` missing or empty now returns `Error::MissingField("sample_id")` instead of silently using `""`
- `majority_label` tie-breaking is now deterministic: alphabetically first label wins on equal counts
- Same tiebreak fix applied inside `model_agreement` and `evaluator_agreement` per-group majority selection

### Added

- `ScoreConfig::validate()` — returns `Error::InvalidScoreScale` if `score_scale` is not positive and finite
- `ScoreWeights` struct for per-dimension weighting of `stability_score`; set a weight to `0.0` to exclude a dimension
- `--weight-labels/scores/budget/seed/models/evaluators` flags on `quietset score` CLI
- `debug_assert` guard in `compute_report` to catch bad `score_scale` during development

- Initial workspace with `quietset` library and `quietset-cli` crates
- `Observation` struct with JSONL and CSV parsing
- `StabilityReport` with label_agreement, score_mean/std/range, budget_sensitivity, seed_sensitivity, model_agreement, evaluator_agreement
- `stability_score` and `disagreement_score` computation
- `keep/review/drop` decision logic with configurable thresholds
- `quietset score` CLI command
- `quietset filter` CLI command with `--min-stability`, `--max-disagreement`, `--decision`
- Integration tests with golden fixture files
- GitHub Actions CI
