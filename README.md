# quietset

[![CI](https://github.com/kent-tokyo/quietset/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/quietset/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/quietset.svg)](https://crates.io/crates/quietset)
[![docs.rs](https://docs.rs/quietset/badge.svg)](https://docs.rs/quietset)

A model-agnostic stability filter — keeps samples whose labels or scores remain
consistent across evaluators, budgets, seeds, and model checkpoints.

quietset is not a model trainer, annotation platform, or image-quality auditor.
It is a small stability-filtering primitive designed to compose with other tools.

> **Note:** quietset measures *stability*, not *correctness*. A sample can score high
> because evaluators consistently agree on a wrong answer. Use `gold_label`-based
> reliability or `--decision-score lcb` to add evidence-based conservatism.

## Use cases

### Game AI / search training data

Multiple engines, depths, or seeds evaluate the same position. Keep only positions
where the evaluation is stable — consistent labels and scores regardless of search parameters.

```bash
quietset score positions.jsonl --profile game-ai > stable_positions.jsonl
quietset stable-wrong-risk positions.jsonl  # flag positions stable evaluators consistently mis-label
```

### LLM judge pipelines

Multiple judge models or prompts evaluate the same response. Keep only responses
where judges consistently agree, using Wilson LCB to guard against low-n flukes.

```bash
quietset score judge_evals.jsonl --profile llm-judge > reliable_evals.jsonl
quietset calibrate judge_evals.jsonl --target-precision 0.95 --decision-score lcb
```

### Synthetic / simulation data

Scores or rewards vary across seeds, budgets, or model checkpoints. Keep samples
whose quality signal is robust to these variations.

```bash
quietset score runs.jsonl --profile simulation > robust_samples.jsonl
quietset audit robust_samples.jsonl --json | jq '.seed_sensitive[:5]'
```

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

# Explicit --decision-score flag (preferred for scripting; --use-* are aliases)
quietset score input.jsonl --decision-score lcb > scored.jsonl
quietset score input.jsonl --decision-score adjusted > scored.jsonl

# Apply a use-case preset (sets weight and decision-score defaults)
quietset score input.jsonl --profile llm-judge > scored.jsonl
quietset score input.jsonl --profile simulation > scored.jsonl

# Require at least 3 observations and 2 evaluators before Keep
quietset score input.jsonl --min-observations-keep 3 --min-evaluators-keep 2 > scored.jsonl

# Filter by LCB, confidence, and dispersion
quietset filter scored.jsonl --min-label-lcb 0.70 > filtered.jsonl
quietset filter scored.jsonl --min-confidence 0.60 --max-score-mad 0.05 > filtered.jsonl

# Compare with per-component deltas (spot regressions)
quietset compare before.jsonl after.jsonl --components

# Deep diagnostic audit report
quietset audit scored.jsonl
quietset audit scored.jsonl --json | jq '.high_raw_low_lcb'
quietset audit scored.jsonl --json --observations input.jsonl | jq '{fleiss_kappa,krippendorff_alpha}'

# Extract samples by diagnostic class for human review
quietset select scored.jsonl --class borderline --top 50
quietset select scored.jsonl --class high-raw-low-lcb > uncertain_keeps.jsonl

# Get re-evaluation recommendations
quietset recommend scored.jsonl

# Compute risk of stably-wrong kept samples
quietset stable-wrong-risk input.jsonl

# Compare with hypothetical policy applied to after file
quietset compare before.jsonl after.jsonl --policy-after lcb

# Calibrate keep_threshold from gold labels to meet a precision target
quietset calibrate input.jsonl --target-precision 0.95
quietset calibrate input.jsonl --target-precision 0.98 --decision-score lcb
```

## Command reference

| Command | Input | What it does |
|---------|-------|-------------|
| `score` | observation JSONL/CSV | Compute per-sample stability scores and decisions |
| `filter` | scored JSONL | Keep samples by stability, decision, LCB, confidence, or dispersion |
| `summary` | scored JSONL | Aggregate statistics; `lcb_keep_demotions`; `--json` for CI |
| `explain` | scored JSONL | Per-sample component breakdown with visual bars |
| `compare` | 2 scored JSONL | Before/after transition matrix, regressions, component deltas, policy comparison |
| `reliability` | observation JSONL | Per-evaluator reliability, confusion matrix, Fleiss kappa, Krippendorff alpha |
| `audit` | scored JSONL | Deep diagnostic report: borderline, LCB risk, sensitivity lists |
| `select` | scored JSONL | Extract samples by class for human review queues (pipeable) |
| `recommend` | scored JSONL | Per-sample re-evaluation suggestions with reasons |
| `stable-wrong-risk` | observation JSONL | Rate of stably-wrong kept samples (requires `gold_label`) |
| `calibrate` | observation JSONL | Find keep_threshold meeting a precision/coverage target |
| `policy` | observation JSONL | Sweep keep_threshold and show the precision/coverage trade-off table |
| `active-review` | scored JSONL | Rank samples by re-evaluation urgency (low LCB, high entropy, dispersion, sensitivity) |

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

Use `--weight-*` flags to tune individual dimensions, or use `--profile` (see [Profiles](#profiles)).

## Profiles

Apply a use-case preset with `--profile` instead of tuning weights manually. Explicit
`--weight-*` and `--decision-score` flags always override the preset.

| Profile | Weight changes | Default decision-score |
|---------|---------------|----------------------|
| `llm-judge` | evaluator ×2, model ×2 | `lcb` |
| `simulation` | budget ×2, seed ×2 | `adjusted` |
| `game-ai` | budget ×2, seed ×2; min-observations 4, min-budgets 2, min-seeds 2 | `lcb` |
| `benchmark` | label ×2, evaluator ×1.5 | `raw` |

```bash
# LLM judge preset (equivalent to --weight-evaluators 2 --weight-models 2 --decision-score lcb)
quietset score input.jsonl --profile llm-judge > scored.jsonl

# Override one weight from the preset
quietset score input.jsonl --profile simulation --weight-budget 3.0 > scored.jsonl
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

The defaults — 0.85 keep, 0.40 drop — were chosen to leave a deliberate review band.
0.85 requires strong agreement across most observations before a sample is trusted;
0.40 only rejects samples with clear, consistent disagreement. Everything between
is uncertain enough to warrant human review rather than an automatic decision.
For high-stakes training data, raise `--keep-threshold` to 0.90–0.95. For noisy
synthetic data where volume matters more than purity, lower it to 0.75–0.80.

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

Add `--components` to show per-dimension mean deltas:

```bash
quietset compare before.jsonl after.jsonl --components
```

```
matched samples:  10000
mean stability:   0.7412 → 0.7801

decision transitions (before → after):
...

component deltas (mean before → after):
  label                      0.88 → 0.90  (+0.02)
  score_consistency          0.79 → 0.86  (+0.07)
  budget_robustness          0.91 → 0.72  (-0.19)  ← regression
  seed_robustness            0.88 → 0.89  (+0.01)
```

`--json` adds a `component_deltas` object with signed delta values.

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

## filter command

In addition to `--min-stability`, `--max-disagreement`, and `--decision`, `filter` supports
diagnostic field filters:

| Flag | Keeps records where |
|------|---------------------|
| `--min-label-lcb <f>` | `label_agreement_lcb >= f` (drop low-evidence keeps) |
| `--min-confidence <f>` | `confidence >= f` (drop low-observation-count samples) |
| `--max-score-mad <f>` | `score_mad <= f` (drop high-dispersion samples) |
| `--max-score-iqr <f>` | `score_iqr <= f` (drop high-spread samples) |

```bash
# Keep only samples with high evidence and low score dispersion
quietset filter scored.jsonl --min-label-lcb 0.70 --min-confidence 0.60 --max-score-mad 0.05 > clean.jsonl
```

Records that lack the filtered field (e.g. `label_agreement_lcb` is absent when no labels were
provided) are excluded by `--min-*` filters and included by `--max-*` filters.

## reliability command (experimental)

> Stability measures agreement, not correctness. Use `stable-wrong-risk` to quantify how many
> of your kept samples are consistently wrong — the most dangerous failure mode in stability-filtered datasets.

Estimate per-evaluator reliability from observation JSONL:

```bash
quietset reliability input.jsonl
```

```json
{"evaluator_id": "m1", "reliability": 0.94}
{"evaluator_id": "m2", "reliability": 0.71}
{"evaluator_id": "m3", "reliability": 0.52}
{"fleiss_kappa": 0.81, "krippendorff_alpha": 0.83}
```

Reliability is the fraction of evaluations where the evaluator's label matches the reference label.
By default, the reference is the majority label across evaluators. If `gold_label` is set on any
observation for a sample, it is used as the reference instead — enabling ground-truth-based
reliability without changing the scoring output.

The trailing line reports two dataset-level agreement statistics:

| Field | Meaning |
|-------|---------|
| `fleiss_kappa` | Inter-rater agreement corrected for chance (nominal labels, variable raters per subject). 0 = chance, 1 = perfect, negative = worse than chance. |
| `krippendorff_alpha` | Agreement coefficient using the coincidence-matrix formulation for nominal labels. More general than kappa; same scale. |

Both are omitted when fewer than 2 subjects have at least 2 ratings each (undefined).
Use `jq 'select(.fleiss_kappa)'` to extract the summary line.

When `gold_label` is present, each evaluator line also includes a `confusion` matrix
(`predicted → gold → count`):

```json
{"evaluator_id": "m1", "reliability": 0.94, "confusion": {"win": {"win": 120, "loss": 8}, "loss": {"win": 11, "loss": 101}}}
{"evaluator_id": "m2", "reliability": 0.71, "confusion": {"win": {"win": 98, "loss": 31}, "loss": {"win": 4, "loss": 107}}}
{"fleiss_kappa": 0.81, "krippendorff_alpha": 0.83}
```

## audit command

Deep diagnostic report for a scored JSONL file:

```bash
quietset audit scored.jsonl
quietset audit scored.jsonl --json           # machine-readable
quietset audit scored.jsonl --top 20         # show top 20 in each list (default 10)
```

```
=== quietset audit ===
total:              1000
  keep:              621  (62.1%)
  review:            291  (29.1%)
  drop:               88   (8.8%)
  lcb_keep_demotions:  139  (stability >= 0.85, lcb < 0.85)

stability_score:
  mean:            0.7412
  median:          0.7810
  p10 / p90:       0.4200 / 0.9600

top instability drivers:
  label disagreement        38%
  score variance            24%

--- borderline (0.75 <= stability <= 0.95, top 10) ---
  sample_042  0.8201  review
  sample_187  0.8490  keep

--- high_raw_low_lcb (stability >= 0.85, lcb < 0.85, top 10) ---
  sample_003  stability=0.9100  lcb=0.3423

--- budget_sensitive (top 10) ---
  sample_091  budget_sensitivity=0.8200
```

`--json` output includes `borderline`, `high_raw_low_lcb`, `high_score_mad`, `budget_sensitive`,
and `seed_sensitive` as arrays of `{sample_id, ...}` objects, suitable for piping to downstream tools.

## calibrate command

Find a `keep_threshold` that meets a precision or coverage target, using `gold_label` observations:

```bash
quietset calibrate input.jsonl --target-precision 0.95
quietset calibrate input.jsonl --target-precision 0.98 --decision-score lcb
quietset calibrate input.jsonl --target-precision 0.90 --target-coverage 0.50
```

```json
{
  "decision_score": "lcb",
  "keep_threshold": 0.91,
  "drop_threshold": 0.40,
  "achieved_precision": 0.982,
  "coverage": 0.61,
  "n_keep": 610,
  "n_total": 1000
}
```

`calibrate` grid-searches `keep_threshold` from 0.99 down to 0.50 (step 0.01) and returns the
loosest threshold that meets the target. Requires `gold_label` on at least one observation per
sample. Returns an error if no threshold meets the target (try a lower `--target-precision`).

> **Note:** calibrate cannot separate stable-correct from stable-wrong samples — if a sample
> consistently gets the wrong label, its `stability_score` is indistinguishable from a correct
> sample. Use `gold_label`-based `reliability` diagnostics to identify systematically wrong evaluators.

## select command

Extract samples by diagnostic class, outputting the original scored JSONL lines (pass-through,
pipeable to other commands):

```bash
quietset select scored.jsonl --class borderline --top 100
quietset select scored.jsonl --class high-raw-low-lcb > uncertain_keeps.jsonl
quietset select scored.jsonl --class budget-sensitive --top 20 | quietset explain - --sample-id x
```

| Class | Selects |
|-------|---------|
| `borderline` | `keep_threshold ± 0.10` stability band (uncertainty zone) |
| `high-disagreement` | sorted by `disagreement_score` descending |
| `budget-sensitive` | sorted by `budget_sensitivity` descending |
| `seed-sensitive` | sorted by `seed_sensitivity` descending |
| `high-raw-low-lcb` | `stability_score >= keep_threshold` but `label_agreement_lcb < keep_threshold` |
| `high-score-mad` | sorted by `score_mad` descending |

Use `--top N` to limit output. Use `--keep-threshold` to adjust the band for `borderline` and
`high-raw-low-lcb` (default 0.85).

## recommend command

Emit a re-evaluation suggestion for each sample that has a detectable issue, in priority order:

```bash
quietset recommend scored.jsonl
quietset recommend scored.jsonl --unstable-only   # skip clean keeps
```

```json
{"sample_id": "x42", "reason": "high_raw_low_lcb", "recommended_action": "add_observations", "stability_score": 0.91, "label_agreement_lcb": 0.34, "n_observations": 2}
{"sample_id": "y17", "reason": "high_seed_sensitivity", "recommended_action": "add_seeds", "seed_sensitivity": 0.71}
```

| Reason | Action |
|--------|--------|
| `high_raw_low_lcb` | LCB below threshold despite high raw stability → `add_observations` |
| `low_evaluator_agreement` | evaluator_agreement < 0.7 → `add_evaluators` |
| `high_seed_sensitivity` | seed_sensitivity > 0.3 → `add_seeds` |
| `high_budget_sensitivity` | budget_sensitivity > 0.3 → `increase_budget` |
| `low_model_agreement` | model_agreement < 0.7 → `add_models` |

Each sample emits at most one recommendation (highest priority rule wins).

## stable-wrong-risk command

Scores observation JSONL internally and reports kept samples whose `majority_label` differs from
`gold_label`:

```bash
quietset stable-wrong-risk input.jsonl
quietset stable-wrong-risk input.jsonl --keep-threshold 0.90
```

```json
{
  "n_total": 1000,
  "n_keep": 621,
  "n_stable_wrong": 12,
  "stable_wrong_rate_among_keep": 0.019,
  "samples": [
    {"sample_id": "x42", "stability_score": 0.96, "majority_label": "loss", "gold_label": "win"}
  ]
}
```

Requires `gold_label` on observations. Sorted by `stability_score` descending — the most
confidently-kept wrong samples appear first. Use `--top N` to limit the sample list.

## compare --policy-after

After the standard comparison output, show how decisions in the after file would change under
a hypothetical decision-score policy:

```bash
quietset compare before.jsonl after.jsonl --policy-after lcb
quietset compare before.jsonl after.jsonl --policy-after adjusted --policy-keep-threshold 0.80
```

```
policy comparison: current → lcb (keep_threshold=0.85):
              →keep   →review    →drop
    keep↓         0       850        0
  review↓         0      2291      300
    drop↓         0         0      200
  demoted by policy: 850  promoted: 0
```

> **Note:** `--policy-after lcb` uses `label_agreement_lcb` as a proxy for the LCB policy score.
> Other components are not recomputed, so results are approximate. Use for directional signal
> ("how many keeps would be demoted"), not precise prediction.

## policy command

Sweeps `keep_threshold` from 0.99 down to 0.50 and reports the precision/coverage/stable-wrong-rate
trade-off at each step, so you can pick a threshold before running `score`:

```bash
quietset policy input.jsonl
quietset policy input.jsonl --target-precision 0.95
quietset policy input.jsonl --decision-score lcb --json
```

```
threshold  n_keep  coverage
0.99            1     0.500
0.98            1     0.500
0.97            1     0.500
```

With `gold_label` present on observations, the table also gains `precision` and
`stable_wrong_rate` columns. `--target-precision`/`--target-coverage` mark the loosest
threshold meeting that target with `←`. `--json` emits one JSONL object per threshold row instead
of the formatted table.

## active-review command

Ranks scored JSONL samples by re-evaluation urgency — a weighted combination of low
`label_agreement_lcb`, high `label_entropy`, high `score_mad`, and high budget/seed sensitivity:

```bash
quietset score input.jsonl | quietset active-review -
quietset active-review scored.jsonl --unstable-only --top 20
```

```json
{"budget_sensitivity":0.62,"label_agreement_lcb":0.095,"label_entropy":1.0,"primary_reason":"high_entropy","sample_id":"b","seed_sensitivity":0.62,"suggested_action":"diversify_evaluators","urgency_score":0.691}
```

`--unstable-only` skips samples already decided `keep` with no instability signals. Per-signal
`--weight-*` flags (`--weight-lcb`, `--weight-entropy`, `--weight-score-mad`,
`--weight-budget-sensitivity`, `--weight-seed-sensitivity`) let you emphasize the signal most
relevant to your review budget.

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

## Python bindings

`crates/quietset-py` provides Python bindings via [pyo3](https://pyo3.rs/) + [maturin](https://www.maturin.rs/).
Status: **alpha / experimental** — the core scoring API is wrapped but the interface may change.

```bash
cd crates/quietset-py && maturin develop
```

```python
import quietset
result = quietset.score_jsonl(
    '{"sample_id":"a","label":"win","score":0.9}\n'
    '{"sample_id":"a","label":"win","score":0.8}\n'
)
print(result)
```

The bindings currently expose a single function, `score_jsonl`, which scores a JSONL string with
default settings and returns a JSONL string of results. The CLI is the stable interface with the
full set of commands and options; use Python bindings for embedding basic scoring in existing
Python pipelines where spawning a subprocess is impractical.

## License

MIT OR Apache-2.0
