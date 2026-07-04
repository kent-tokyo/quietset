# quietset — todo

## Phase 1: Core library (done)

- [x] Cargo workspace
- [x] `quietset` library crate
- [x] `quietset-cli` crate
- [x] `Observation` struct
- [x] JSONL parsing with line-number errors
- [x] Group observations by `sample_id` (insertion order preserved)
- [x] `label_agreement` metric
- [x] `score_mean / score_std / score_range` metrics
- [x] `stability_score` computation
- [x] `StabilityReport` serialized as JSONL

## Phase 2: Filter + extended metrics (done)

- [x] `quietset filter` command (`--min-stability`, `--max-disagreement`, `--decision`)
- [x] `keep / review / drop` thresholds (configurable)
- [x] CSV input support
- [x] `budget_sensitivity` metric
- [x] `seed_sensitivity` metric
- [x] `model_agreement` metric
- [x] `evaluator_agreement` metric
- [x] Fixture files (`simple`, `noisy`, `budget_sensitive`, `stable_scores`)
- [x] 11 integration tests (all passing)

## Phase 3: Polish (done)

- [x] README with CLI examples and Rust API example
- [x] CHANGELOG
- [x] GitHub Actions CI (`fmt`, `clippy`, `test`, `doc`)
- [x] Dual license MIT OR Apache-2.0
- [x] docs.rs comments on all public types
- [x] `--score-scale` flag exposed in CLI
- [x] Cargo.toml metadata: `description`, `repository`, `keywords`, `categories`

## Phase 4: Extensions (done)

- [x] Python bindings (`crates/quietset-py` via pyo3 + maturin — excluded from workspace)
- [x] Streaming grouped aggregation (`StreamingScorer` for pre-sorted input)
- [x] Benchmark suite (`cargo bench` — criterion, `simple` and `stable_scores` fixtures)
- [x] CSV output format (`--output-format csv` in `score` command)
- [x] `--skip-invalid` flag for CLI (both `score` and `filter`)
- [x] Integration example: LLM judge agreement (`examples/llm_judge.jsonl`)
- [x] Integration example: game/search AI training labels (`examples/game_search.jsonl`)

## Phase 5: Precision & safety (done)

- [x] `label_agreement_lcb` — Wilson confidence interval lower bound
- [x] `score_mad`, `score_iqr` — robust dispersion metrics
- [x] `gold_label` on `Observation` — ground-truth for reliability
- [x] `DecisionScore::LowerConfidenceBound` + `--decision-score raw|adjusted|lcb` enum flag
- [x] `--use-adjusted-score` / `--use-lcb-score` kept as backwards-compat aliases
- [x] `explain`: `label_agreement_lcb` + score stats block
- [x] `summary`: `lcb_keep_demotions`, `score_mad_mean`, `score_iqr_mean`, `--keep-threshold`
- [x] Fleiss' kappa + Krippendorff's alpha on `reliability` (trailing JSONL line)
- [x] pyo3 CVEs fixed (0.21 → 0.29); Dependabot config; CI permissions
- [x] 46 integration tests (all passing)

## Phase 6: Data quality gate (done)

- [x] `audit` — deep diagnostic report; `--observations` for agreement stats; borderline band = `keep_threshold ± 0.10`
- [x] `calibrate` — grid-search keep_threshold from gold_label observations
- [x] `filter` extended — `--min-label-lcb`, `--min-confidence`, `--max-score-mad`, `--max-score-iqr`
- [x] `compare --components` + `--policy-after raw|adjusted|lcb`
- [x] `score --profile llm-judge|simulation|game-ai|benchmark`
- [x] `reliability` — confusion matrix when `gold_label` present
- [x] `select` — extracts samples by class (pass-through JSONL); borderline = `keep_threshold ± 0.10`
- [x] `recommend` — per-sample re-evaluation suggestions with priority-ordered rules
- [x] `stable-wrong-risk` — fraction of kept samples stably wrong
- [x] `CalibrationResult` + `compute_calibration()` exported from library
- [x] 50 integration tests (all passing)

## Phase 7: Discoverability & ergonomics (done)

- [x] README restructured: badges, tagline, use-cases (game-ai / llm-judge / simulation), command reference table (11 commands), `## Profiles` section promoted
- [x] README_ja.md synced: same structure in Japanese
- [x] GitHub releases v0.2.0–v0.7.0 created with CHANGELOG notes
- [x] `recommend --text` — human-readable column output; JSONL default preserved
- [x] `score --embed-stats` — appends sentinel stats line to scored JSONL; downstream readers skip gracefully
- [x] `audit --json` — auto-reads embedded kappa/alpha from `--embed-stats`; `--observations` still takes priority
- [x] Published v0.7.0; CHANGELOG kept in sync

## Phase 8: Documentation polish (done)

- [x] README Decisions section: added rationale for 0.85/0.40 defaults and guidance for adjusting thresholds
- [x] README: added `## Python bindings` section documenting quietset-py alpha status, install, and Python API snippet
- [x] tasks/todo.md: removed duplicate Phase 7 header

## Phase 9: Precision & weighted voting (done)

- [x] `label_distribution` field added to `StabilityReport` — fraction per label, insertion-order sorted by frequency
- [x] `indexmap` serde feature enabled in `crates/quietset/Cargo.toml`
- [x] `compute_evaluator_weights()` — per-evaluator smoothed accuracy from gold or majority truth
- [x] `compute_weighted_majority()` — reliability-weighted label vote per sample
- [x] `score_all_weighted()` — 2-pass function: standard score → compute weights → fill weighted_* fields
- [x] `weighted_majority_label`, `weighted_label_confidence`, `weighted_label_distribution`, `majority_weighted_conflict` added to `StabilityReport`
- [x] `game-ai` profile tightened: seed_stability ×2 (was ×1.5), min_obs 4 (was 3), min_budgets 2, min_seeds 2, decision-score LCB (was adjusted)
- [x] `score --vote weighted` flag: triggers `score_all_weighted`
- [x] `policy` command: threshold sweep 0.99→0.50, precision/coverage/stable_wrong_rate table, `--target-precision` / `--target-coverage` / `--json`
- [x] `active-review` command: ranks scored JSONL by urgency (low_lcb, high_entropy, score_mad, budget/seed sensitivity), `--top` / `--unstable-only`
- [x] 7 new integration tests (57 total, all passing)
- [x] cargo fmt + clippy -D warnings clean

## Phase 10: Robust score dispersion + CHANGELOG sync (done)

- [x] `ScoreDispersion` enum (`Std`, `Mad`, `Iqr`) added to `metrics.rs`, exported from lib
- [x] `ScoreConfig::score_dispersion` field (default `Std`, backward-compatible)
- [x] `compute_report()` uses selected dispersion for `score_consistency` component
- [x] `score --score-dispersion std|mad|iqr` CLI flag
- [x] 3 new integration tests (60 total, all passing)
- [x] CHANGELOG synced: v0.7.0 entry (embed-stats/audit--json/recommend--text), v0.8.0 entry (Phase 9 + Phase 10 combined)
- [x] Cargo.toml versions bumped to 0.8.0 (both crates + dependency pin)
- [x] git commit + push (main branch, 77d7143)
- [x] Published quietset v0.8.0 and quietset-cli v0.8.0 to crates.io

## Phase 11: Gold-weighted decision score (done)

- [x] `DecisionScore::GoldWeighted` variant + `compute_report`/`compute_report_inner` split
- [x] `score_all_gold_weighted()` — gold-only truth map, 2-pass reliability-weighted rescoring
- [x] `score --decision-score gold-weighted` CLI flag; short-circuits `--vote` with a warning on redundant combos
- [x] `calibrate`/`policy` support `--decision-score gold-weighted`; precision uses `weighted_majority_label`
- [x] `compare --policy-after gold-weighted` proxy via `weighted_label_confidence`
- [x] 2 new integration tests (66 total, all passing)
- [x] README.md / README_ja.md Decisions table + CHANGELOG Unreleased entry
- [x] cargo fmt + clippy -D warnings clean

## Phase 12: Precision improvements for GoldWeighted (done)

- [x] `stable-wrong-risk` gained `--decision-score` (previously missing entirely) and now falls back to `weighted_majority_label`, matching `calibrate`/`policy`
- [x] `compute_evaluator_label_weights()` — per-(evaluator, predicted label) confusion-matrix reliability, Laplace-smoothed, no backoff (smoothing alone handles sparse pairs)
- [x] `compute_weighted_majority_by_label()` — weighted vote keyed by per-label reliability; shared tail logic extracted into `majority_from_weighted_labels`
- [x] `score_all_gold_weighted()` swapped to per-label weighting; `score_all_weighted`/`--vote weighted` (Phase 9) left untouched
- [x] 1 new integration test (67 total, all passing) — proves asymmetric per-evaluator reliability is captured
- [x] CHANGELOG Unreleased Added/Fixed entries
- [x] cargo fmt + clippy -D warnings clean

## Phase 13: Small gaps found by diffing a stale external review (done)

- [x] `game-ai` profile now auto-bumps `--min-evaluators-keep` to 2, matching the existing min-observations/min-budgets/min-seeds pattern
- [x] `recommend` gained a `low_score_consistency` → `reduce_score_variance` rule (lowest priority), reusing the already-normalized `components.score_consistency` field
- [x] README.md/README_ja.md Python bindings section now discloses `quietset-py`'s workspace/CI exclusion, not just alpha status; README_ja.md brought to full parity with README.md
- [x] `test_game_ai_profile_uses_lcb_and_stricter_mins` updated to keep its replicated CLI config accurate
- [x] CHANGELOG Unreleased Added/Changed entries
- [x] cargo fmt + clippy -D warnings clean

## Phase 14: calibrate/policy CSV output + Wilson CI on achieved_precision (done)

- [x] `wilson_lcb` generalized into two-sided `wilson_ci` (`pub(crate)`); `wilson_lcb` now a thin wrapper, no behavior change for its existing caller
- [x] `CalibrationResult` gains `precision_ci_low`/`precision_ci_high` (always `f64`, no arbitrary "few gold labels" cutoff — the interval widens on its own)
- [x] `calibrate --output-format json|csv` — CSV is a single header + data row
- [x] `policy --output-format text|json|csv` — CSV has fixed columns (empty cells when no gold); `--json` kept as a backward-compatible alias, warns if both passed
- [x] 2 new library tests (67 total) — CI contains achieved_precision; CI narrows with more gold samples
- [x] 2 new CLI smoke tests (justified exception to the regression-tests-only convention — new user-facing output format, not a config knob)
- [x] README.md/README_ja.md calibrate/policy sections, Output formats table, CSV dead-end paragraph updated
- [x] CHANGELOG Unreleased Added entries
- [x] cargo fmt + clippy -D warnings clean

## Phase 15: Latent-truth decision score (Dawid-Skene EM) + score_sign_agreement (done)

A large external review proposed a 14-item "risk-controlled noisy-evaluation engine"
upgrade; filtered against AGENTS.md non-goals (scale permission ≠ direction change) down to
the 2 items that deepen the core thesis without changing what kind of tool this is. The other
12 — `DecisionPolicy` trait, policy-file TOML, schema versioning/nested output, adapters
crate, expanded Python API, EV-based active-review optimizer, conformal risk control, and 3 of
5 proposed score signals — were rejected as premature abstraction or deferred to their own
cycle; see `tasks/lessons.md` and the CHANGELOG for the reasoning.

- [x] `DecisionScore::LatentTruth` — new `crates/quietset/src/latent_truth.rs` module,
  hand-rolled Dawid-Skene EM (Laplace-smoothed, 20-iteration cap, log-space E-step), no new
  dependency
- [x] `score_all_latent_truth()` — 2-pass orchestrator mirroring `score_all_gold_weighted`
- [x] `compute_report_inner`'s `gold_weighted_confidence` param generalized to
  `external_label_confidence`, shared by `GoldWeighted` and `LatentTruth` (mutually exclusive
  by construction)
- [x] `--decision-score latent-truth` wired through `score`, `compare --policy-after`,
  `calibrate`, `policy`, `stable-wrong-risk` — all 5 sites `gold-weighted` already touched
- [x] 4 new integration tests (72 total) + 4 inline unit tests in `latent_truth.rs` (exact
  hand-derived EM-step values, adversarial-tie termination, both fallback conditions)
- [x] `score_sign_agreement` — one new always-computed `StabilityReport` field; 4 of 5
  proposed "domain-aware score signal" metrics dropped as redundant with
  `budget_sensitivity`/`budget_slope` or architecturally incompatible (cross-sample ranking);
  no new `--score-mode` flag, no new `ScoreConfig` surface
- [x] Manual smoke tests: CLI live-confirms the EM recovers a biased-evaluator-corrupted
  majority; single-evaluator/single-label fallback is byte-identical to plain `score`;
  `calibrate`/`policy`/`stable-wrong-risk --decision-score latent-truth` all dispatch
  correctly; `score_sign_agreement` matches hand-computed values against
  `examples/game_search.jsonl`'s existing signed scores
- [x] README.md/README_ja.md Decisions table (5 rows) + diagnostic-fields table updated
- [x] CHANGELOG Unreleased Added entries
- [x] cargo fmt + clippy -D warnings clean

## Phase 16: Held-out calibration split, latent-truth correlation diagnostics, score_sign_agreement as optional weight (done)

A second large external review re-proposed the same 14-item roadmap from Phase 15's cycle;
~80% was re-litigation of already-recorded rejections (see `tasks/lessons.md`). Three items
were genuinely new or newly-actionable and confirmed via AskUserQuestion; everything else was
held to the prior rejections, not re-litigated.

- [x] `compute_calibration` gains `heldout: Option<&[Observation]>` — threshold *selection*
  unchanged (grid-searched on the primary set only); once found, precision/coverage/CI are
  measured on `heldout` instead when supplied, fixing the in-sample circularity named 3x in
  `tasks/lessons.md`. `calibrate --heldout <path>` (not stdin-capable); CLI-only
  `validated_on_heldout` field discloses which set was measured
- [x] Latent-truth gains `evaluator_effective_n`/`correlated_evaluator_warning` — Kish design
  effect over a phi-coefficient pairwise evaluator correlation, computed post-hoc from EM's
  converged posterior (no changes to `em_step` or the EM loop itself)
- [x] `score_sign_agreement` can now optionally weight into `stability_score`
  (`ScoreWeights::score_sign`, default `0.0` — the one weight in the struct that doesn't
  default to `1.0`, deliberately, for backward compatibility); added to `StabilityComponents`
  and `weakest()`'s tie-break candidates; `config.validate()` extended to cover it
- [x] 2 new library tests for `--heldout` (anti-circularity proof + no-gold-in-heldout) +
  1 CLI test column-count update; 7 new `latent_truth.rs` tests (hand-derived exact DEFF
  arithmetic, correlated-twins detection, the documented blind-spot case, independent-pair
  computed-not-defaulted phi, single-evaluator trivial case, 3 helper-level unit tests) +
  1 bench call site updated; 3 new `score_sign_agreement` weighting tests (76 lib tests total)
- [x] Manual smoke tests: `--heldout` shows a materially different (lower, honest) precision
  vs. the same file without it; no-gold-in-heldout produces the documented error; correlated
  twins fixture live-confirms `evaluator_effective_n≈1.8`/`correlated_evaluator_warning=true`
  matching the hand-derived test exactly; `--weight-score-sign 2.0` measurably lowers
  `stability_score` on a sign-flipping fixture vs. the same fixture with the default weight
- [x] README.md/README_ja.md `calibrate` section (`--heldout` example + explanation),
  Decisions table (latent-truth row), diagnostic-fields table and Components table
  (`score_sign_agreement`) updated
- [x] CHANGELOG Unreleased Added entries
- [x] cargo build --workspace --all-targets + test + clippy -D warnings + fmt clean

## Phase 17: Latent-truth convergence diagnostics, calibrate optimism note, EV-based active-review (done)

A third large external review re-proposed ~80% of the same rejected 14-item roadmap (a 4th
repeat of the false "README_ja.md is empty" claim — verified 828 lines; a repeat of the
already-twice-rejected policy-file TOML direction) plus a stale claim about GitHub
release/tag/crates.io version mismatch (verified all consistent at 0.11.0). Three items were
genuinely new or newly-actionable, confirmed via AskUserQuestion: latent-truth EM convergence
diagnostics, a machine-readable calibrate optimism note, and EV-based active-review (the latter
explicitly *deferred* — not rejected — in Phase 15's cycle, promoted to build now on explicit
request, same "deferred has an expiration condition" pattern as Phase 16's held-out split).

- [x] `LatentTruthEstimate`/`StabilityReport` gain `latent_truth_converged`,
  `latent_truth_iterations`, `latent_truth_convergence_delta` — previously an EM run that hit
  its 20-iteration cap without settling was indistinguishable from a clean convergence; same
  value applies to every sample in a batch (EM runs once per batch). 2 new tests empirically
  verified before locking in (clean fixture converges in 3 iterations; the existing adversarial
  fixture hits the cap at exactly 20)
- [x] `calibrate` gains `note: Option<String>` — always present when `--heldout` wasn't used
  (the existing in-sample-optimism caveat, now machine-readable); present when `--heldout` was
  used but in-sample precision exceeded held-out precision by >0.05 (likely overfit); `null`
  when the gap is small. 3 new tests (always-present without heldout, present on a 0.70 gap,
  absent on a zero gap)
- [x] EV-based active-review: promoted the existing CLI-only urgency heuristic byte-for-byte
  into a new `quietset::active_review` library module (`rank_active_review`/
  `ActiveReviewEntry`/`ActiveReviewWeights`/`ActiveReviewCosts`/`RankBy`), then added
  `label_agreement_flip_ratio` (a distance-to-threshold ratio, explicitly documented as **not**
  a probability — only `label_agreement`'s Wilson-CI uncertainty is modeled, since no other
  `stability_score` component has an uncertainty model in this codebase),
  `expected_coverage_gain` (`1/n_total` for non-keep samples), `expected_risk_reduction`
  (`1/n_keep` for at-risk keeps, reusing `stable-wrong-risk`'s existing `lcb_keep_demotions`
  condition), `cost` (new `--cost-seed`/`--cost-budget`/`--cost-evaluator`/`--cost-gold-label`
  flags), and `utility` (benefit/cost). New `--keep-threshold`/`--drop-threshold` feed only
  these new fields, never re-deriving `decision`. New `--rank-by urgency|utility`
- [x] **Breaking**: `suggested_action` values renamed (`add_observations`→`request_gold_label`,
  `diversify_evaluators`→`add_evaluator`, `reduce_score_variance`→`add_model`,
  `add_budget`→`increase_budget`, `add_seeds`→`add_seed`) — confirmed via AskUserQuestion after
  verifying no test pinned the old strings; documented in CHANGELOG as a breaking change
- [x] 5 new `active_review.rs` unit tests (2 hand-derived exact utility values, 1 property-style
  urgency-vs-utility reordering test, 1 no-labels edge case, 1 action-distinctness guard) +
  1 new CLI smoke test asserting the full new JSON shape and renamed action set
- [x] Manual smoke tests: latent-truth convergence fields show up correctly in `score` output;
  calibrate `note` empirically matches all three cases (always-present, large-gap, small-gap);
  active-review `--rank-by urgency` vs `--rank-by utility` empirically flips ranking order on a
  cheap-fix-at-threshold vs. expensive-fix-off-threshold fixture, matching hand-derived values
  exactly
- [x] README.md/README_ja.md `active-review` section rewritten (renamed actions, 5 new fields,
  new flags) and latent-truth Decisions-table row extended; CHANGELOG Added + Changed(breaking)
  entries
- [x] cargo build --workspace --all-targets + test (18+78+11+2) + clippy -D warnings + fmt clean
