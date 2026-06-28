# Changelog

## Unreleased

### Added

- `Observation::validate(line)` — shared validation method; called by `parse_jsonl`, `parse_csv`, and the CLI `--skip-invalid` JSONL path
- `Error::InvalidWeight` — returned when a `ScoreWeights` field is negative, NaN, or infinite
- `Error::AllWeightsZero` — returned when all weights sum to zero
- CSV output now includes `component_label`, `component_score_consistency`, `component_budget_robustness`, `component_seed_robustness`, `component_model_agreement`, `component_evaluator_agreement` columns

### Fixed

- `--skip-invalid` JSONL path now calls `Observation::validate()` — empty `sample_id`, non-finite `score`, and non-finite `budget` are skipped with a warning instead of silently accepted
- `ScoreConfig::validate()` now checks each `ScoreWeights` field (must be finite and >= 0.0) and rejects all-zero weight configs
- `StabilityComponents::weakest()` tie policy documented: fixed declaration order (`label` → `score_consistency` → ... → `evaluator_agreement`)



- `StabilityComponents` struct — per-dimension sub-scores (`label`, `score_consistency`, `budget_robustness`, `seed_robustness`, `model_agreement`, `evaluator_agreement`) now appear in every `StabilityReport` under the `components` key
- `StabilityComponents::weakest()` — returns the lowest-scoring component for instability diagnosis
- `quietset summary` CLI command — prints aggregate stats (sample counts, stability percentiles, top instability drivers) for a scored JSONL file
- `cargo audit` (via `rustsec/audit-check`) added to GitHub Actions CI
- `Error::InvalidBudget` — returned when `budget` is NaN or infinite
- `Error::InvalidThreshold` — returned when thresholds are out of `[0.0, 1.0]` or `drop > keep`

### Fixed

- `score` and `budget` fields with NaN or infinite values now return an explicit error instead of propagating silently
- `ScoreConfig::validate()` now also checks that `keep_threshold` and `drop_threshold` are in `[0.0, 1.0]` and that `drop_threshold <= keep_threshold`

### Changed

- `seed_sensitivity` is now included in `stability_score` computation (was computed but silently excluded)
- `sample_id` missing or empty now returns `Error::MissingField("sample_id")` instead of silently using `""`
- `majority_label` tie-breaking is now deterministic: alphabetically first label wins on equal counts
- `ScoreConfig::validate()` — returns `Error::InvalidScoreScale` if `score_scale` is not positive and finite
- `ScoreWeights` struct for per-dimension weighting of `stability_score`
- `--weight-labels/scores/budget/seed/models/evaluators` flags on `quietset score` CLI

## 0.1.0

- Initial release
- `quietset` library crate with `Observation`, `StabilityReport`, `Decision`, `ScoreConfig`, `ScoreWeights`
- `quietset-cli` crate with `score`, `filter`, `summary` commands
- JSONL and CSV input; JSONL and CSV output
- GitHub Actions CI (`fmt`, `clippy`, `test`, `doc`, `audit`)
