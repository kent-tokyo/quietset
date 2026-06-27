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

It is computed as the mean of available sub-scores:

- `label_agreement` — fraction of observations with the majority label
- `1 - normalized_score_std` — score consistency
- `1 - budget_sensitivity` — robustness to compute budget changes
- `model_agreement` — label agreement across models
- `evaluator_agreement` — label agreement across evaluators

Missing dimensions (e.g. no labels, no budgets) are excluded from the mean.
Single observations receive `stability_score = 0.5` (review by default).

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

- **Cleanlab** — Python, task-specific, detects label errors via trained classifiers. quietset is model-agnostic and needs no training.
- **Label Studio** — annotation UI. quietset is a CLI/library primitive.
- **pandas** — general data tool. quietset specializes in stability metrics.

## License

MIT OR Apache-2.0
