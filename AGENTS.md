# AGENTS.md

## Project: quietset

`quietset` is a Rust library and CLI for filtering noisy datasets by label stability.

The core idea is simple:

> Keep samples whose labels or scores remain stable across evaluators, budgets, random seeds, model checkpoints, or repeated runs.

This project is **not** a general machine learning framework, annotation tool, or dataset platform. It should be a small, composable, model-agnostic filtering primitive.

Good positioning:

```text
quietset is a model-agnostic Rust library for selecting stable samples
from noisy evaluations across budgets, seeds, models, and labelers.
```

## Product direction

`quietset` should help users answer questions such as:

* Which samples receive consistent labels across multiple evaluators?
* Which examples are sensitive to compute budget, search depth, random seed, or model checkpoint?
* Which samples should be kept, reviewed, or dropped from a training set?
* Which synthetic labels are stable enough to trust?
* Which evaluation cases are ambiguous or unstable?

Primary use cases:

* noisy label filtering
* synthetic data filtering
* reinforcement learning sample selection
* search-based labeling
* simulation result filtering
* LLM judge agreement analysis
* game AI training data selection
* benchmark curation

## Non-goals

Do **not** turn this into:

* a full ML training framework
* a replacement for Cleanlab
* an annotation UI
* an image-quality auditor
* an LLM data processing platform
* a domain-specific game engine tool
* a Python-first project with Rust as an afterthought

Avoid implementing model training, neural networks, GPU logic, dataset hosting, or visualization dashboards in the core library.

## Core concept

Input data consists of repeated observations for the same sample.

Each observation may include:

```text
sample_id
label
score
evaluator_id
budget
seed
run_id
model_id
metadata
```

The library groups observations by `sample_id` and computes stability metrics.

Possible output fields:

```text
sample_id
n_observations
majority_label
label_agreement
score_mean
score_std
score_range
budget_sensitivity
seed_sensitivity
model_agreement
disagreement_score
stability_score
decision
```

Where `decision` is one of:

```text
keep
review
drop
```

## MVP scope

The first useful version should include:

1. Rust core library
2. JSONL input/output support
3. CSV input/output support if simple
4. CLI command: `quietset score`
5. CLI command: `quietset filter`
6. Label agreement scoring
7. Numeric score stability scoring
8. Budget sensitivity scoring
9. Seed/model/evaluator agreement scoring
10. Basic tests and golden fixtures
11. README with examples

Do not overbuild the first version.

## Suggested crate layout

Use a Cargo workspace.

```text
quietset/
  Cargo.toml
  README.md
  AGENTS.md
  LICENSE-MIT
  LICENSE-APACHE
  crates/
    quietset/
      Cargo.toml
      src/
        lib.rs
        observation.rs
        schema.rs
        group.rs
        metrics.rs
        decision.rs
        error.rs
    quietset-cli/
      Cargo.toml
      src/
        main.rs
  tests/
    fixtures/
      simple.jsonl
      noisy.jsonl
      budget_sensitive.jsonl
      stable_scores.jsonl
```

Optional later:

```text
crates/
  quietset-py/
```

Only add Python bindings after the Rust API and CLI are stable.

## License

Use dual license:

```text
MIT OR Apache-2.0
```

Add this to each crate:

```toml
license = "MIT OR Apache-2.0"
```

## Rust guidelines

Use stable Rust.

Prefer simple, explicit code over clever abstractions.

Recommended dependencies:

```toml
serde
serde_json
csv
clap
thiserror
anyhow
indexmap
ordered-float
```

Avoid heavy dependencies unless strongly justified.

Do not add ML frameworks.

Do not add async unless there is a clear need.

Do not use unsafe code.

If unsafe is ever proposed, reject it unless there is a benchmark-backed reason and a clear safety explanation.

## Public API design

The library should expose simple types.

Example sketch:

```rust
pub struct Observation {
    pub sample_id: String,
    pub label: Option<String>,
    pub score: Option<f64>,
    pub evaluator_id: Option<String>,
    pub budget: Option<f64>,
    pub seed: Option<u64>,
    pub model_id: Option<String>,
}

pub struct StabilityReport {
    pub sample_id: String,
    pub n_observations: usize,
    pub majority_label: Option<String>,
    pub label_agreement: Option<f64>,
    pub score_mean: Option<f64>,
    pub score_std: Option<f64>,
    pub score_range: Option<f64>,
    pub budget_sensitivity: Option<f64>,
    pub seed_sensitivity: Option<f64>,
    pub model_agreement: Option<f64>,
    pub disagreement_score: f64,
    pub stability_score: f64,
    pub decision: Decision,
}

pub enum Decision {
    Keep,
    Review,
    Drop,
}
```

Prefer accepting iterators where practical.

Avoid locking the API to JSONL.

The core library should not depend on CLI-specific types.

## CLI design

The CLI should feel like a Unix data tool.

Main commands:

```bash
quietset score input.jsonl > scored.jsonl

quietset filter scored.jsonl \
  --min-stability 0.85 \
  --max-disagreement 0.15 \
  > quiet.jsonl
```

Support reading from stdin:

```bash
cat runs/*.jsonl | quietset score --input - > scored.jsonl
```

Useful options:

```text
--input
--output
--format jsonl|csv
--group-by sample_id
--label-col label
--score-col score
--budget-col budget
--seed-col seed
--model-col model_id
--evaluator-col evaluator_id
--min-stability
--max-disagreement
--keep-threshold
--drop-threshold
--decision keep|review|drop
```

Do not require users to use Rust structs directly.

The CLI should work with plain JSONL files from other tools.

## Input JSONL example

Example input:

```json
{"sample_id":"a","label":"win","score":0.91,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"a","label":"win","score":0.88,"evaluator_id":"m1","budget":8,"seed":1}
{"sample_id":"a","label":"win","score":0.90,"evaluator_id":"m2","budget":8,"seed":2}
{"sample_id":"b","label":"win","score":0.52,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"b","label":"loss","score":-0.10,"evaluator_id":"m2","budget":8,"seed":2}
```

Example output:

```json
{"sample_id":"a","n_observations":3,"majority_label":"win","label_agreement":1.0,"score_std":0.015,"stability_score":0.97,"decision":"keep"}
{"sample_id":"b","n_observations":2,"majority_label":"win","label_agreement":0.5,"score_std":0.31,"stability_score":0.42,"decision":"review"}
```

## Stability scoring

Keep the scoring simple and explainable.

Initial formula can be:

```text
stability_score =
  weighted_mean(
    label_agreement,
    1 - normalized_score_std,
    1 - budget_sensitivity,
    model_agreement,
    evaluator_agreement
  )
```

All sub-scores should be normalized to `[0.0, 1.0]`.

Rules:

* `1.0` means highly stable
* `0.0` means highly unstable
* missing dimensions should not automatically produce zero
* insufficient observations should lower confidence
* report `n_observations`

Avoid pretending that a single observation is stable.

If there is only one observation, mark it as low-confidence or `review` unless the user explicitly changes thresholds.

## Metrics to implement

Start with these metrics:

### Label agreement

For categorical labels:

```text
label_agreement = count(majority_label) / total_labels
```

### Score standard deviation

For numeric scores:

```text
score_std = standard_deviation(scores)
```

Normalize with a configurable scale:

```text
normalized_score_std = min(score_std / score_scale, 1.0)
```

### Score range

```text
score_range = max(score) - min(score)
```

### Budget sensitivity

If observations include budgets, compare scores across budget levels.

Simple initial implementation:

```text
budget_sensitivity = normalized range of mean score per budget
```

### Model agreement

If observations include model IDs, compute majority-label agreement across models.

### Evaluator agreement

If observations include evaluator IDs, compute majority-label agreement across evaluators.

## Decision logic

Default decision thresholds:

```text
keep   if stability_score >= 0.85
drop   if stability_score <= 0.40
review otherwise
```

These thresholds should be configurable.

Do not silently discard records.

Always make the decision visible in output.

## Error handling

Use typed errors in the library.

Use human-readable errors in the CLI.

Examples:

```text
missing required field: sample_id
invalid score value
unsupported format
could not parse JSONL at line 42
no observations found
```

The CLI should exit non-zero on malformed input unless a future `--skip-invalid` option is added.

## Testing requirements

Every new feature should include tests.

Minimum test categories:

* parsing valid JSONL
* rejecting invalid JSONL
* grouping by sample_id
* label agreement
* score mean/std/range
* budget sensitivity
* keep/review/drop decision
* missing optional fields
* single-observation behavior
* deterministic output order

Use small golden fixture files.

Prefer deterministic tests.

Do not rely on random behavior unless the seed is fixed.

## Performance expectations

The library should handle large JSONL streams reasonably.

For MVP, grouping all observations in memory is acceptable.

However, design with future streaming support in mind.

Avoid unnecessary clones in hot paths.

Do not optimize prematurely.

If performance changes are made, add a benchmark.

Possible future benchmark:

```bash
cargo bench
```

## Documentation requirements

README should include:

* what quietset is
* what quietset is not
* installation
* JSONL example
* CLI examples
* Rust API example
* explanation of stability score
* comparison with adjacent tools
* license

Add docs.rs comments for public types.

Keep terminology consistent:

Use:

```text
sample
observation
label
score
evaluator
budget
seed
stability
disagreement
decision
```

Avoid vague terms like:

```text
quality
clean
good
bad
truth
```

unless clearly defined.

## README positioning

Suggested README opening:

```md
# quietset

quietset filters datasets by label stability, not by task-specific assumptions.

It helps you keep samples whose labels or scores remain stable across evaluators,
budgets, random seeds, model checkpoints, or repeated runs.

It is useful for noisy supervision, synthetic data filtering, reinforcement
learning, search-based labeling, simulation, and benchmark curation.
```

Also include:

```md
quietset is not a model trainer, annotation platform, or image-quality auditor.
It is a small stability-filtering primitive designed to compose with other tools.
```

## Development commands

Expected commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

Before finishing any task, run:

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

If a command cannot be run, explain why.

Do not claim tests pass unless they were actually run.

## CI expectations

Add GitHub Actions for:

```text
cargo fmt
cargo clippy
cargo test
cargo doc
```

Optional later:

```text
cargo audit
cargo deny
coverage
release build
```

## Backward compatibility

Before version `0.1.0`, API changes are acceptable.

After the first public release, avoid breaking changes without updating the changelog.

Use semantic versioning.

## Changelog

Maintain `CHANGELOG.md`.

Use this format:

```md
# Changelog

## Unreleased

### Added

### Changed

### Fixed
```

## Commit style

Prefer clear conventional commits:

```text
feat: add JSONL scoring
fix: handle missing optional score field
docs: add CLI examples
test: add budget sensitivity fixture
refactor: split metric calculation
```

## Agent behavior

When working on this repository:

1. Inspect existing files first.
2. Do not rewrite large parts unnecessarily.
3. Keep changes small and reviewable.
4. Add tests for behavior changes.
5. Update README when user-facing behavior changes.
6. Preserve public API simplicity.
7. Avoid heavy dependencies.
8. Do not introduce domain-specific assumptions.
9. Keep the project model-agnostic.
10. Prefer CLI composability.

## Recommended first implementation plan

Phase 1:

```text
- create workspace
- create quietset library crate
- create quietset-cli crate
- define Observation
- parse JSONL
- group observations by sample_id
- compute label_agreement
- compute score mean/std/range
- compute basic stability_score
- output StabilityReport as JSONL
```

Phase 2:

```text
- add filter command
- add keep/review/drop thresholds
- add CSV support
- add budget sensitivity
- add evaluator/model agreement
- add fixture tests
```

Phase 3:

```text
- add README examples
- add docs.rs comments
- add CI
- add changelog
- prepare 0.1.0 release
```

Phase 4, optional:

```text
- Python bindings via pyo3
- streaming grouped aggregation
- benchmark suite
- additional output formats
- integration examples for LLM judge results
- integration examples for game/search labels
```

## Naming notes

The project name is `quietset`.

Meaning:

```text
quiet = stable, low-volatility, low-disagreement
set   = selected dataset subset
```

Do not rename the project unless explicitly requested.

## Final design principle

The best version of quietset is boring, composable, and trustworthy.

It should do one thing well:

> turn repeated noisy evaluations into explainable stability scores and filtered datasets.
