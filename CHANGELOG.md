# Changelog

## Unreleased

## 0.9.0 — 2026-07-01

### Changed
- **BREAKING**: the internal `quietset::metrics` module has been split into `config`, `scoring`,
  `agreement`, `calibration`, and `weighting` modules for maintainability (`metrics.rs` had grown
  to ~940 of the crate's ~1400 lines). The `quietset::metrics` path no longer exists. All items
  previously re-exported from it (`ScoreConfig`, `score_all`, `compute_report`, `DecisionScore`,
  `MinRequirements`, `ScoreDispersion`, `ScoreWeights`, `compute_fleiss_kappa`,
  `compute_krippendorff_alpha`, `compute_calibration`, `CalibrationResult`,
  `compute_evaluator_weights`, `compute_weighted_majority`, `compute_evaluator_reliability`)
  remain available unchanged at the crate root (e.g. `quietset::score_all`). Only code importing
  via the internal `quietset::metrics::*` path directly needs to update.
- CLI `--help` text clarified for `score --output-format` and `policy --json` to document their
  output-format conventions (see the new README "Output formats" section for the full picture
  across all 13 subcommands).

### Fixed
- `filter` no longer silently exits 0 with zero output when every input row is invalid and
  dropped via `--skip-invalid`; it now errors with "no records found", matching `summary`,
  `audit`, `select`, and `recommend`.

## 0.8.0 — 2026-06-28

### Added
- `label_distribution` field on `StabilityReport` — fraction of observations per label, sorted by frequency descending; `None` when no labels present
- `weighted_majority_label`, `weighted_label_confidence`, `weighted_label_distribution`, `majority_weighted_conflict` fields on `StabilityReport` — set when `score --vote weighted` is used
- `score_all_weighted()` — 2-pass function: standard scoring → per-evaluator reliability weights → weighted majority vote; exported from `quietset` crate
- `compute_evaluator_weights()`, `compute_weighted_majority()` — reliability weighting primitives; exported from `quietset` crate
- `score --vote raw|weighted` flag — `weighted` triggers `score_all_weighted`
- `policy` command — sweeps `keep_threshold` 0.99→0.50 and outputs precision / coverage / stable_wrong_rate table; `--target-precision`, `--target-coverage`, `--json`, `--decision-score` flags
- `active-review` command — ranks scored JSONL samples by re-evaluation urgency (low LCB, high entropy, score MAD, budget/seed sensitivity); `--top`, `--unstable-only`, per-signal `--weight-*` flags
- `--score-dispersion std|mad|iqr` on `score` command — selects the dispersion metric used for the `score_consistency` stability component; `mad` and `iqr` are more robust when occasional outlier scores are present; default `std` is backward-compatible
- `ScoreDispersion` enum (`Std`, `Mad`, `Iqr`) exported from `quietset` crate; `ScoreConfig::score_dispersion` field

### Changed
- `game-ai` profile: `seed_stability` weight 1.5× → 2×, `min_observations_keep` 3 → 4, `min_budgets_keep` 0 → 2, `min_seeds_keep` 0 → 2, default `decision_score` adjusted → LCB

## 0.7.0 — 2026-06-28

### Added
- `score --embed-stats` — appends a trailing sentinel stats line (`{"_quietset_stats":true,...}`) with `fleiss_kappa` and `krippendorff_alpha` to scored JSONL output; opt-in; backward-compatible via `--skip-invalid`
- `audit --json` — auto-reads embedded kappa/alpha from `--embed-stats` output; `--observations` still takes priority
- `recommend --text` — human-readable column output (sample_id | reason | action | detail); JSONL default preserved

## 0.6.0 — 2026-06-28

### Added
- `select` command — extracts samples by diagnostic class (borderline, high-disagreement, budget-sensitive, seed-sensitive, high-raw-low-lcb, high-score-mad); outputs original JSONL lines unchanged (pipeable); `--top N`; borderline band is `keep_threshold ± 0.10` (was hardcoded [0.75, 0.95])
- `recommend` command — emits one JSONL line per sample with a re-evaluation suggestion; priority-ordered rules: high_raw_low_lcb → add_observations, low_evaluator_agreement → add_evaluators, high_seed_sensitivity → add_seeds, high_budget_sensitivity → increase_budget, low_model_agreement → add_models; `--unstable-only` to skip clean keeps
- `stable-wrong-risk` command — scores observation JSONL internally; reports `stable_wrong_rate_among_keep` (kept samples where majority_label ≠ gold_label); requires `gold_label`; JSON output with per-sample list
- `compare --policy-after raw|adjusted|lcb` — second transition matrix showing how after-file decisions would change under an alternative policy; `--policy-keep-threshold`, `--policy-drop-threshold`
- `audit --observations <file>` — optional observation JSONL input; adds `fleiss_kappa` and `krippendorff_alpha` to `--json` output and `dataset agreement:` section to text output

### Fixed
- `audit` borderline band now uses `keep_threshold ± 0.10` (respecting `--keep-threshold` flag) instead of hardcoded [0.75, 0.95]

## 0.5.0 — 2026-06-28

### Added
- `audit` command — deep diagnostic report for scored JSONL; surfaces borderline, high_raw_low_lcb, high_score_mad, budget_sensitive, seed_sensitive samples; `--json` and `--top N`
- `calibrate` command — grid-search `keep_threshold` (0.99→0.50, step 0.01) to meet `--target-precision` or `--target-coverage` using `gold_label` observations; outputs recommended threshold and achieved metrics
- `CalibrationResult` struct and `compute_calibration()` function exported from `quietset` crate
- `filter`: `--min-label-lcb`, `--min-confidence`, `--max-score-mad`, `--max-score-iqr` flags
- `compare --components`: per-dimension mean deltas with regression markers; `component_deltas` in JSON output
- `score --profile llm-judge|simulation|game-ai|benchmark`: use-case weight presets; explicit `--weight-*` and `--decision-score` flags override preset
- `reliability`: confusion matrix per evaluator (`predicted → gold → count`) when `gold_label` present in observations

### Changed
- `score` `--weight-*` flags changed from `f64` (default 1.0) to `Option<f64>` (no default) to allow profile presets to supply defaults without clobbering explicit user values

## 0.4.0 — 2026-06-28

### Changed
- Synced README/CHANGELOG documentation for the Fleiss' kappa / Krippendorff's alpha additions from 0.3.0
- `quietset-cli`'s dependency on `quietset` bumped to 0.4.0

## 0.3.0 — 2026-06-28

### Added
- `compute_fleiss_kappa()` — inter-rater agreement corrected for chance; nominal labels, variable raters per subject; exported from `quietset` crate
- `compute_krippendorff_alpha()` — coincidence-matrix formulation for nominal labels, variable raters; exported from `quietset` crate
- `reliability` command now appends a trailing JSONL line `{"fleiss_kappa": ..., "krippendorff_alpha": ...}` after per-evaluator lines; omitted when fewer than 2 subjects have ≥ 2 ratings

## 0.2.0 — 2026-06-28

### Added
- `label_agreement_lcb` on `StabilityReport` — Wilson confidence interval lower bound of `label_agreement`; guards against over-confidence on low-n samples
- `score_mad`, `score_iqr` on `StabilityReport` — median absolute deviation and interquartile range; more robust to outliers than `score_std`
- `gold_label` on `Observation` — known-correct label; `compute_evaluator_reliability` uses it as ground truth over majority vote when present
- `DecisionScore::LowerConfidenceBound` — new decision mode using `label_agreement_lcb` (most conservative)
- `--decision-score raw|adjusted|lcb` enum flag on `score` command (preferred over boolean aliases for scripting)
- `--confidence-level` flag for Wilson LCB confidence level (default 0.95)
- `explain`: `label_agreement_lcb` line and `score stats` block (mean / std / mad / iqr)
- `summary`: `lcb_keep_demotions` count (samples raw scoring keeps but LCB mode would demote), `score_mad_mean`, `score_iqr_mean`; `--keep-threshold` flag
- `summary --json`: `lcb_keep_demotions`, `score_mad_mean`, `score_iqr_mean` fields
- `.github/dependabot.yml` — weekly Cargo scans for workspace root and `crates/quietset-py`
- CI: `permissions: contents: read` on `GITHUB_TOKEN`
- Decisions section in README updated to 3-mode table with alias column
- "stable ≠ correct" note added to README and README_ja

### Fixed
- `lcb_keep_demotions` now correctly counts only samples where `stability_score >= keep_threshold AND label_agreement_lcb < keep_threshold`; previously included samples already below the raw threshold
- pyo3 bumped 0.21 → 0.29 in `crates/quietset-py` (fixes CVE: missing `Sync` bound on `PyCFunction`, out-of-bounds read in `nth`/`nth_back`, buffer overflow in `PyString::from_object`)

### Changed
- `--use-adjusted-score` and `--use-lcb-score` are now documented as aliases for `--decision-score adjusted` and `--decision-score lcb`; `--decision-score` takes precedence when both are specified; a warning is emitted to stderr on conflict

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
