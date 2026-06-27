# Changelog

## Unreleased

### Added

- Initial workspace with `quietset` library and `quietset-cli` crates
- `Observation` struct with JSONL and CSV parsing
- `StabilityReport` with label_agreement, score_mean/std/range, budget_sensitivity, seed_sensitivity, model_agreement, evaluator_agreement
- `stability_score` and `disagreement_score` computation
- `keep/review/drop` decision logic with configurable thresholds
- `quietset score` CLI command
- `quietset filter` CLI command with `--min-stability`, `--max-disagreement`, `--decision`
- Integration tests with golden fixture files
- GitHub Actions CI
