# quietset

quietset filters datasets by label stability, not by task-specific assumptions.

It helps you keep samples whose labels or scores remain stable across evaluators,
budgets, random seeds, model checkpoints, or repeated runs.

It is useful for noisy supervision, synthetic data filtering, reinforcement
learning, search-based labeling, simulation, and benchmark curation.

quietset is not a model trainer, annotation platform, or image-quality auditor.
It is a small stability-filtering primitive designed to compose with other tools.

## Installation

```bash
cargo install --path crates/quietset-cli
```

## CLI examples

```bash
# Score observations
quietset score input.jsonl > scored.jsonl

# Filter to stable samples
quietset filter scored.jsonl --min-stability 0.85 > quiet.jsonl

# Filter by decision
quietset filter scored.jsonl --decision keep > keep.jsonl

# Pipeline from stdin
cat runs/*.jsonl | quietset score - > scored.jsonl

# Aggregate statistics
quietset summary scored.jsonl

# CSV output
quietset score input.jsonl --output-format csv > scored.csv

# Weight label agreement 2x, ignore score variance
quietset score input.jsonl --weight-labels 2.0 --weight-scores 0.0 > scored.jsonl
```

## Input JSONL format

```json
{"sample_id":"a","label":"win","score":0.91,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"a","label":"win","score":0.88,"evaluator_id":"m1","budget":8,"seed":1}
{"sample_id":"b","label":"win","score":0.52,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"b","label":"loss","score":-0.10,"evaluator_id":"m2","budget":8,"seed":2}
```

## Output JSONL format

```json
{"sample_id":"a","n_observations":2,"majority_label":"win","label_agreement":1.0,"score_mean":0.895,"score_std":0.015,"stability_score":0.97,"decision":"keep"}
{"sample_id":"b","n_observations":2,"majority_label":"win","label_agreement":0.5,"score_std":0.31,"stability_score":0.42,"decision":"review"}
```

## Stability score

The `stability_score` is a value in `[0.0, 1.0]`:

- `1.0` = highly stable
- `0.0` = highly unstable

It is the **weighted mean** of available sub-scores (all in `[0.0, 1.0]`):

| Component | Value |
|-----------|-------|
| `label_agreement` | fraction of observations with the majority label |
| `score_consistency` | `1 - normalized_score_std` |
| `budget_robustness` | `1 - budget_sensitivity` |
| `seed_robustness` | `1 - seed_sensitivity` |
| `model_agreement` | label agreement across models |
| `evaluator_agreement` | label agreement across evaluators |

Missing dimensions (e.g. no labels, no budgets) are excluded from the mean.
Single observations receive `stability_score = 0.5` (review by default).

Each sub-score is also exposed in `StabilityReport.components` so you can see why a sample was scored as it was:

```json
{
  "sample_id": "a",
  "stability_score": 0.91,
  "decision": "keep",
  "components": {
    "label": 1.0,
    "score_consistency": 0.96,
    "budget_robustness": 0.88
  }
}
```

Use `--weight-*` flags to emphasise dimensions relevant to your pipeline:

```bash
# LLM judge: weight evaluator/model agreement more
quietset score input.jsonl --weight-labels 1.0 --weight-evaluators 2.0 --weight-models 2.0

# Simulation: weight seed/budget robustness more
quietset score input.jsonl --weight-seed 2.0 --weight-budget 2.0
```

## Decisions

| threshold | decision |
|-----------|----------|
| `stability_score >= 0.85` | keep |
| `stability_score <= 0.40` | drop |
| otherwise | review |

Configurable via `--keep-threshold` and `--drop-threshold`.

## Rust API

```rust
use quietset::{Observation, ScoreConfig, score_all};

let obs = vec![
    Observation { sample_id: "a".into(), label: Some("win".into()), score: Some(0.9), ..Default::default() },
    Observation { sample_id: "a".into(), label: Some("win".into()), score: Some(0.88), ..Default::default() },
];
let reports = score_all(obs, &ScoreConfig::default());
println!("{:?}", reports[0].decision);
```

## Compared to adjacent tools

| Tool | What it does | How quietset differs |
|------|-------------|----------------------|
| **Cleanlab** | Python library that detects label errors using trained classifiers and confident learning. Works with classification, regression, and NLP tasks. | quietset needs no model training and makes no task-specific assumptions. It filters by cross-run stability rather than by estimated label quality. |
| **Label Studio** | Web-based annotation platform for labelling images, text, audio, and time series. Supports multi-annotator workflows. | quietset is a CLI/library primitive, not an annotation UI. It consumes labels already produced by other tools and measures how stable they are. |
| **pandas / polars** | General-purpose data manipulation libraries. Can compute std, groupby, and aggregations. | quietset provides a purpose-built stability schema — `keep / review / drop` decisions, per-dimension sub-scores, instability diagnostics — that would otherwise require substantial custom code. |
| **Great Expectations / Soda** | Data quality frameworks that validate data against rules (nulls, ranges, schema). | Those tools check whether data *conforms to a schema*. quietset checks whether labels or scores are *consistent across repeated evaluations*. The concerns are orthogonal. |
| **scipy.stats / sklearn metrics** | Statistical functions such as Cohen's kappa, Fleiss' kappa, and inter-rater agreement. | quietset wraps similar ideas into a composable pipeline primitive with JSONL I/O, per-sample reports, and configurable thresholds. You could replicate it with scipy, but you would need to wire up grouping, normalisation, weighting, and output formatting yourself. |
| **LLM evaluation frameworks (RAGAS, DeepEval)** | Frameworks that score LLM outputs against reference answers using model-based judges. | quietset is judge-agnostic. It takes *whatever scores or labels your judges produce* and measures agreement across runs, budgets, models, or seeds. It composes with any LLM judge rather than replacing one. |

## License

MIT OR Apache-2.0
