# quietset

quietset filters datasets by label stability, not by task-specific assumptions.

It helps you keep samples whose labels or scores remain stable across evaluators,
budgets, random seeds, model checkpoints, or repeated runs.

It is useful for noisy supervision, synthetic data filtering, reinforcement
learning, search-based labeling, simulation, and benchmark curation.

quietset is not a model trainer, annotation platform, or image-quality auditor.
It is a small stability-filtering primitive designed to compose with other tools.

> **Note:** quietset measures *stability*, not *correctness*. A sample can score high
> because evaluators consistently agree on a wrong answer. Use `gold_label`-based
> reliability or `--use-lcb-score` to add evidence-based conservatism.

## Installation

```bash
cargo install quietset-cli
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

# Machine-readable summary for CI
quietset summary scored.jsonl --json | jq '.drop_rate < 0.1'

# Explain why a specific sample was scored the way it was
quietset explain scored.jsonl --sample-id a

# Compare two scored files (e.g. before/after a model update)
quietset compare before.jsonl after.jsonl

# Per-evaluator reliability (experimental)
quietset reliability input.jsonl

# CSV output
quietset score input.jsonl --output-format csv > scored.csv

# Weight label agreement 2x, ignore score variance
quietset score input.jsonl --weight-labels 2.0 --weight-scores 0.0 > scored.jsonl

# Penalise low-evidence samples: decisions use confidence-adjusted score
quietset score input.jsonl --use-adjusted-score > scored.jsonl

# Penalise low-evidence samples: Wilson LCB on label agreement (most conservative)
quietset score input.jsonl --use-lcb-score > scored.jsonl

# Tune the confidence level for LCB (default 0.95)
quietset score input.jsonl --use-lcb-score --confidence-level 0.99 > scored.jsonl

# Explicit --decision-score flag (preferred for scripting; --use-* are aliases)
quietset score input.jsonl --decision-score lcb > scored.jsonl
quietset score input.jsonl --decision-score adjusted > scored.jsonl

# Require at least 3 observations and 2 evaluators before Keep
quietset score input.jsonl --min-observations-keep 3 --min-evaluators-keep 2 > scored.jsonl
```

## Input JSONL format

```json
{"sample_id":"a","label":"win","score":0.91,"evaluator_id":"m1","budget":4,"seed":1,"gold_label":"win"}
{"sample_id":"a","label":"win","score":0.88,"evaluator_id":"m1","budget":8,"seed":1,"gold_label":"win"}
{"sample_id":"b","label":"win","score":0.52,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"b","label":"loss","score":-0.10,"evaluator_id":"m2","budget":8,"seed":2}
```

All fields except `sample_id` are optional. `gold_label` provides the known-correct label for
a sample; when present, the `reliability` command uses it as ground truth instead of majority vote.

## Output JSONL format

Key fields in the output (optional fields are omitted when not computable):

```json
{
  "sample_id": "a",
  "n_observations": 2,
  "majority_label": "win",
  "label_agreement": 1.0,
  "label_agreement_lcb": 0.342,
  "label_margin": 1.0,
  "label_entropy": 0.0,
  "score_mean": 0.895,
  "score_std": 0.015,
  "score_mad": 0.015,
  "score_iqr": 0.030,
  "confidence": 0.40,
  "adjusted_stability_score": 0.782,
  "stability_score": 0.97,
  "decision": "keep",
  "components": {
    "label": 1.0,
    "score_consistency": 0.985
  }
}
```

Optional fields are omitted when not computable (e.g. `label_agreement_lcb` only appears when
labels are present; `score_mad` / `score_iqr` require at least two numeric scores).

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

Additional diagnostic fields on `StabilityReport`:

| Field | Meaning |
|-------|---------|
| `label_margin` | `(majority_count - runner_up_count) / total`. 0.0 = perfectly split |
| `label_entropy` | Normalised Shannon entropy [0, 1]. 1.0 = uniform label distribution |
| `label_agreement_lcb` | Wilson confidence interval lower bound of `label_agreement`. More conservative than raw `label_agreement` — guards against low-n coincidences. |
| `score_mad` | Median absolute deviation of numeric scores. More robust to outliers than `score_std`. |
| `score_iqr` | Interquartile range (Q3 − Q1) of numeric scores. |
| `budget_slope` | Score trend as budget increases (positive = converges upward) |
| `confidence` | `n / (n + k)` — how much to trust the score given evidence count |
| `adjusted_stability_score` | `stability * confidence + 0.5 * (1 - confidence)` |

### Components field

Each sub-score is also exposed in `components` so you can see why a sample was scored as it was:

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
quietset score input.jsonl --weight-evaluators 2.0 --weight-models 2.0

# Simulation: weight seed/budget robustness more
quietset score input.jsonl --weight-seed 2.0 --weight-budget 2.0
```

## Confidence and adjusted score

`confidence = n / (n + k)` where `k` defaults to 3.0.

| n_observations | confidence (k=3) |
|---------------|-----------------|
| 1 | 0.25 |
| 2 | 0.40 |
| 5 | 0.63 |
| 10 | 0.77 |
| 20 | 0.87 |

`adjusted_stability_score = stability_score * confidence + 0.5 * (1 - confidence)`

A sample with `stability_score = 0.95` but only 2 observations gets `adjusted_stability_score ≈ 0.68` — unlikely to reach the keep threshold (0.85) without more evidence.

Use `--use-adjusted-score` to make decisions based on the adjusted score.
Use `--confidence-k` to tune the convergence speed.

## Minimum requirements for Keep

High stability does not guarantee sufficient evidence. Use `--min-*-keep` to demote underevidenced samples to Review:

```bash
quietset score input.jsonl \
  --min-observations-keep 3 \
  --min-evaluators-keep 2 \
  --min-seeds-keep 2 \
  > scored.jsonl
```

## Decisions

By default, decisions use `stability_score`. Three decision modes are available:

| Flag | Alias | Score used | Behaviour |
|------|-------|-----------|-----------|
| `--decision-score raw` *(default)* | — | `stability_score` | Raw stability. Fast; can overfit small-n. |
| `--decision-score adjusted` | `--use-adjusted-score` | `adjusted_stability_score` | Penalises low-evidence samples proportionally. |
| `--decision-score lcb` | `--use-lcb-score` | `label_agreement_lcb` (label) | Wilson LCB — most conservative. A 2/2 label match gives LCB ≈ 0.34 at 95% confidence, so it will not be kept without more evidence. |

`MinRequirements` are always applied **after** the threshold comparison.

| Condition | Decision |
|-----------|----------|
| score >= 0.85 | keep |
| score <= 0.40 | drop |
| otherwise | review |

Configurable via `--keep-threshold` and `--drop-threshold`. Use `--confidence-level` to tune the
Wilson LCB confidence level (default 0.95).

The `--use-adjusted-score` and `--use-lcb-score` boolean flags are aliases for
`--decision-score adjusted` and `--decision-score lcb` respectively, kept for backwards
compatibility. When both `--decision-score` and a boolean alias are specified,
`--decision-score` takes precedence.

## explain command

Print a detailed breakdown for one sample:

```bash
quietset explain scored.jsonl --sample-id a
```

```
sample_id:          a
decision:           keep
n_observations:     3
stability_score:    0.9700
confidence:         0.5000
adjusted_score:     0.7350
label_agreement_lcb:0.4380
label_margin:       1.0000
label_entropy:      0.0000

score stats:
  mean:             0.8950
  std:              0.0150
  mad:              0.0150
  iqr:              0.0300

components:
  label                      1.0000  ████████████████████
  score_consistency          0.9850  ███████████████████
  budget_robustness          0.8800  █████████████████
  seed_robustness            0.9200  ██████████████████
```

Add `--json` to get the full `StabilityReport` as JSON.

> **Note**: this example uses the default raw-score decision mode (`stability_score = 0.97 → keep`).
> With `--use-adjusted-score` (confidence ≈ 0.50 at n=3), `adjusted_score = 0.74` falls below the
> keep threshold — the decision would be **review** unless `--keep-threshold` is lowered.
> With `--use-lcb-score`, `label_agreement_lcb ≈ 0.44` also falls below 0.85 — **review**.

## compare command

Compare two scored JSONL files by `sample_id`:

```bash
quietset compare before.jsonl after.jsonl
```

```
matched samples:  10000
mean stability:   0.7412 → 0.7801

decision transitions (before → after):
              →keep   →review    →drop
      keep↓    7210       311       42
    review↓     508      2101      301
      drop↓      19       104      404

top 5 regressions:
  sample_001  0.9100 → 0.4400  (Δ-0.4700)
  sample_382  0.8800 → 0.3900  (Δ-0.4900)
```

Add `--json` for machine-readable output.

## summary command

```bash
quietset summary scored.jsonl
```

```
samples:              1000
  keep:                621  (62.1%)
  review:              291  (29.1%)
  drop:                 88   (8.8%)
  lcb_keep_demotions:  139  (stability_score >= 0.85, label_agreement_lcb < 0.85)

stability_score:
  mean:              0.7412
  median:            0.7810
  p10 / p90:         0.4200 / 0.9600

score dispersion (mean across samples):
  mad:               0.0421
  iqr:               0.0812

top instability drivers (review + drop samples):
  label disagreement        38%
  score variance            24%
  seed sensitivity          21%
  budget sensitivity        17%
```

`lcb_keep_demotions` counts samples where `stability_score >= keep_threshold` (raw mode would
keep them) but `label_agreement_lcb < keep_threshold` (LCB mode would not) — the number of
samples that switching to `--decision-score lcb` would demote from `keep`. Samples already
below the threshold in raw mode are excluded. Pass `--keep-threshold` to match the value used
during scoring.

Use `--json` for CI integration:

```bash
quietset summary scored.jsonl --json | jq '.drop_rate < 0.1'
```

## reliability command (experimental)

Estimate per-evaluator reliability from observation JSONL:

```bash
quietset reliability input.jsonl
```

```json
{"evaluator_id": "m1", "reliability": 0.94}
{"evaluator_id": "m2", "reliability": 0.71}
{"evaluator_id": "m3", "reliability": 0.52}
```

Reliability is the fraction of evaluations where the evaluator's label matches the reference label.
By default, the reference is the majority label across evaluators. If `gold_label` is set on any
observation for a sample, it is used as the reference instead — enabling ground-truth-based
reliability without changing the scoring output.

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

### Streaming API

```rust
use quietset::{Observation, ScoreConfig, StreamingScorer};

let mut scorer = StreamingScorer::new(ScoreConfig::default());
for obs in observations {
    if let Some(report) = scorer.push(obs) {
        println!("{:?}", report.decision);
    }
}
if let Some(report) = scorer.flush() { println!("{:?}", report.decision); }
```

## Compared to adjacent tools

| Tool | What it does | How quietset differs |
|------|-------------|----------------------|
| **Cleanlab** | Python library that detects label errors using trained classifiers and confident learning. | quietset needs no model training and makes no task-specific assumptions. It filters by cross-run stability rather than estimated label quality. |
| **Label Studio** | Web-based annotation platform for labelling images, text, audio, and time series. | quietset is a CLI/library primitive, not an annotation UI. It measures stability of labels already produced by other tools. |
| **pandas / polars** | General-purpose data manipulation libraries. | quietset provides a purpose-built stability schema — decisions, per-dimension sub-scores, confidence, instability diagnostics — that would otherwise require substantial custom code. |
| **Great Expectations / Soda** | Data quality frameworks that validate data against rules (nulls, ranges, schema). | Those tools check whether data *conforms to a schema*. quietset checks whether labels or scores are *consistent across repeated evaluations*. |
| **scipy.stats / sklearn metrics** | Statistical functions such as Cohen's kappa and Fleiss' kappa. | quietset wraps similar ideas into a composable pipeline primitive with JSONL I/O, per-sample reports, confidence adjustment, and configurable thresholds. |
| **LLM evaluation frameworks (RAGAS, DeepEval)** | Frameworks that score LLM outputs against reference answers using model-based judges. | quietset is judge-agnostic. It takes whatever scores or labels your judges produce and measures agreement across runs, budgets, models, or seeds. |

## License

MIT OR Apache-2.0
