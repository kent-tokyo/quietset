# quietset

**English** | [日本語](README_ja.md)

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
`--profile game-ai` already weights signed-score agreement and defaults `--decision-score`
to `lcb`, so a small centipawn-like magnitude near zero can't masquerade as "stable" when
its sign is actually flipping under search noise (see `stability_score` below for why raw
stability alone isn't a keep/drop guarantee). Evaluating with several distinct engines?
Use `--profile game-ai`. Evaluating with one engine at multiple search depths instead? Use
`--profile game-ai-single-engine` — same weights, but without a min-evaluators floor that a
single-engine setup could never satisfy.

```bash
quietset score positions.jsonl --profile game-ai > scored.jsonl
quietset stable-wrong-risk positions.jsonl  # flag positions stable evaluators consistently mis-label

# Spend additional search budget only on positions worth re-evaluating, not the whole set.
quietset active-review scored.jsonl --observations positions.jsonl \
  --plan next_eval_plan.jsonl --budget 2000

# Gate a training run on held-out precision before trusting the kept set.
quietset calibrate train_gold.jsonl --heldout heldout_gold.jsonl \
  --target-precision 0.98 --fail-below-target
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
| `block-score` | observation JSONL | Group by `block_id` and classify block-level trajectory stability |
| `trajectory-audit` | 2 observation JSONL | Diff block-level trajectory stability between a before/after checkpoint |

## Output formats

Output conventions differ by command — deliberately, since some commands are built for human
inspection and some for pipeline composition (see [`select`](#select-command) and
[`--embed-stats`](#audit-command)). There is no single unified format; use this table to know
what to expect from each command before scripting against it:

| Command | Default | Flag(s) | Notes |
|---------|---------|---------|-------|
| `score` | JSONL | `--output-format jsonl\|csv` | `csv` is a terminal/export format — see below |
| `filter` | JSONL (pass-through) | none | Always echoes original input lines unchanged |
| `select` | JSONL (pass-through) | none | Always echoes original input lines unchanged |
| `reliability` | JSONL | none | One object per evaluator, plus an optional trailing kappa/alpha line |
| `active-review` | JSONL | none | One object per ranked sample |
| `block-score` | JSONL | none | One object per block |
| `trajectory-audit` | text | `--json` (JSONL, one object per block) | Sorted most-destructive block first |
| `recommend` | JSONL | `--text` | Only command where JSONL is the default and text is the opt-in |
| `stable-wrong-risk` | single pretty JSON object | none | |
| `calibrate` | single pretty JSON object | `--output-format json\|csv` | `csv` is a single header row + one data row |
| `summary` | text | `--json` (single pretty object) | |
| `explain` | text | `--json` (single pretty object) | |
| `compare` | text | `--json` (single pretty object) | |
| `audit` | text | `--json` (single pretty object) | |
| `policy` | text table | `--output-format text\|json\|csv` (legacy `--json` still works as an alias for `json`) | `json` is JSONL, one line per swept threshold; `csv` has fixed columns (empty cells when no `gold_label`) |

**CSV is a dead end for piping.** `score --output-format csv`, `calibrate --output-format csv`,
and `policy --output-format csv` exist purely for spreadsheets/BI tools. No other quietset
command can parse CSV back in (`filter`/`summary`/`explain`/`compare`/`audit`/`select`/
`recommend`/`active-review` all expect JSONL `StabilityReport`s; `stable-wrong-risk`/`calibrate`/
`reliability`/`policy` expect JSONL `Observation`s). If you plan to pipe `score`'s output into
another quietset command, use the default `jsonl`, not `csv`.

## Input JSONL format

```json
{"sample_id":"a","label":"win","score":0.91,"evaluator_id":"m1","budget":4,"seed":1,"gold_label":"win"}
{"sample_id":"a","label":"win","score":0.88,"evaluator_id":"m1","budget":8,"seed":1,"gold_label":"win"}
{"sample_id":"b","label":"win","score":0.52,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"b","label":"loss","score":-0.10,"evaluator_id":"m2","budget":8,"seed":2}
```

All fields except `sample_id` are optional. `gold_label` provides the known-correct label for
a sample; when present, the `reliability` command uses it as ground truth instead of majority vote.

### Trajectory stability fields (optional)

10 additional optional fields let a training harness feed precomputed per-run trajectory
signals into quietset for `block-score`/`trajectory-audit` and the `score` components below —
quietset only ever aggregates these numbers; it never reads a checkpoint or computes a gradient
itself:

```json
{"sample_id":"p1","block_id":"B5","seed":1,"shuffle_seed":1,"model_id":"ckpt143","loss_recipe":"recipe_a","layer_id":"ft","gradient_sign":0.3,"update_cosine":0.8,"teacher_residual":0.02,"trajectory_effect":-0.4,"dead_unit_count":0,"saturated_unit_count":0}
```

| Field | Meaning |
|-------|---------|
| `shuffle_seed` | Second seed axis (data-order/shuffle), distinct from `seed` (init seed) |
| `loss_recipe` | Loss-function/training-recipe variant — same role as `model_id`/`evaluator_id` |
| `block_id` | Groups this sample into a training block (e.g. a 32-position block), distinct from `sample_id` |
| `layer_id` | Generic layer identifier for `dead_unit_count`/`saturated_unit_count` — quietset assigns no meaning to specific values (e.g. `"ft"`, `"l2"` are just strings you choose) |
| `gradient_sign` | Signed per-run scalar; only its sign is used |
| `update_cosine` | Per-run cosine similarity of this update to a reference direction |
| `teacher_residual` | Per-run signed scalar (e.g. model output minus teacher output) |
| `trajectory_effect` | Signed per-run scalar driving block growth/shrink classification (e.g. a radial-norm delta); positive = growth, negative = shrink |
| `dead_unit_count` | Count of newly-dead units in `layer_id` for this (sample, run) |
| `saturated_unit_count` | Count of newly-saturated units in `layer_id` for this (sample, run) |

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

`stability_score` is a **convenience aggregate for ranking and inspection — not a calibrated
probability and not a correctness guarantee**. It has no uncertainty model of its own; a sample
where every evaluator consistently agrees on a wrong answer still scores `1.0`. For real
accept/reject decisions, prefer a component with an actual statistical or model-based basis —
`label_agreement_lcb` (Wilson interval), `--decision-score latent-truth` (Dawid-Skene EM),
`gold-weighted`, or a threshold picked by `calibrate --heldout` — over the raw
`stability_score`/`--keep-threshold` default. See
[Where these numbers come from](#where-these-numbers-come-from) and
[docs/metrics.md](docs/metrics.md) for the full provenance of every field below.

It is the **weighted mean** of available sub-scores (all in `[0.0, 1.0]`):

| Component | Value |
|-----------|-------|
| `label_agreement` | fraction of observations with the majority label |
| `score_consistency` | `1 - normalized_score_std` |
| `budget_robustness` | `1 - budget_sensitivity` |
| `seed_robustness` | `1 - seed_sensitivity` |
| `model_agreement` | label agreement across models |
| `evaluator_agreement` | label agreement across evaluators |
| `score_sign_agreement` | fraction of numeric scores sharing the majority sign — weight defaults to `0` (excluded); opt in with `--weight-score-sign` (`--profile game-ai` enables it at `×2` by default) |
| `gradient_sign_agreement` | fraction of `gradient_sign` observations sharing the majority sign — weight defaults to `0`; opt in with `--weight-gradient-sign` |
| `update_direction_agreement` | fraction of `update_cosine` observations sharing the majority sign — weight defaults to `0`; opt in with `--weight-update-cosine` |
| `teacher_residual_stability` | `1 - normalized_std(teacher_residual)` — weight defaults to `0`; opt in with `--weight-teacher-residual` |
| shuffle-seed robustness | `1 - shuffle_seed_sensitivity` — weight defaults to `0`; opt in with `--weight-shuffle-seed` |
| `loss_recipe_agreement` | label agreement across `loss_recipe` values — weight defaults to `0`; opt in with `--weight-loss-recipe` |

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
| `score_sign_agreement` | Fraction of numeric scores sharing the majority sign (negative / zero / positive). Domain-aware complement to `score_std`/`score_mad`/`score_iqr` for signed, zero-centered scores (chess/shogi-style eval): `[+0.01, -0.01]` reads as near-perfectly stable by magnitude alone but is a coin-flip on sign. Always computed but excluded from `stability_score` by default (`--weight-score-sign 0`); pass `--weight-score-sign <f>` to make it contribute. `--profile game-ai` enables it at `×2` by default — for every other profile, this stays a purely informational field unless explicitly weighted. |
| `budget_slope` | Score trend as budget increases (positive = converges upward) |
| `confidence` | `n / (n + k)` — how much to trust the score given evidence count |
| `adjusted_stability_score` | `stability * confidence + 0.5 * (1 - confidence)` |
| `update_cosine_mean` | Mean of `update_cosine` across observations. Diagnostic only, not a `stability_score` component. |
| `dead_unit_rate` / `saturated_unit_rate` | Fraction of observations where `dead_unit_count`/`saturated_unit_count` is `> 0.0` — how often this sample induces new dead/saturated units. Diagnostic only; feeds `block-score`'s pathological classification, not `stability_score`. |
| `trajectory_effect_mean` / `trajectory_effect_sign_agreement` | Mean and sign-agreement of `trajectory_effect`. Diagnostic only; feeds `block-score`'s growth/shrink classification, not `stability_score`. |

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
| `game-ai` | budget ×2, seed ×2, score_sign ×2; min-observations 4, min-budgets 2, min-seeds 2, min-evaluators 2 | `lcb` |
| `game-ai-single-engine` | same as `game-ai`, but no min-evaluators floor — for one engine evaluated at multiple depths | `lcb` |
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

By default, decisions use `stability_score`. Five decision modes are available:

| Flag | Alias | Score used | Behaviour |
|------|-------|-----------|-----------|
| `--decision-score raw` *(default)* | — | `stability_score` | Raw stability. Fast; can overfit small-n. |
| `--decision-score adjusted` | `--use-adjusted-score` | `adjusted_stability_score` | Penalises low-evidence samples proportionally. |
| `--decision-score lcb` | `--use-lcb-score` | `label_agreement_lcb` (label) | Wilson LCB — most conservative. A 2/2 label match gives LCB ≈ 0.34 at 95% confidence, so it will not be kept without more evidence. |
| `--decision-score gold-weighted` | — | `stability_score` (label component reliability-weighted) | Replaces the raw majority label vote with a `gold_label`-reliability-weighted vote. Falls back to identical behaviour as `raw` when no `gold_label` is present anywhere in the input. |
| `--decision-score latent-truth` | — | `stability_score` (label component EM-weighted) | Replaces the raw majority label vote with a Dawid-Skene EM-estimated vote — infers per-evaluator reliability from disagreement patterns alone, no `gold_label` required. Falls back to identical behaviour as `raw` when the input has fewer than 2 distinct evaluators or labels. Adds `latent_truth_label`, `latent_truth_confidence`, `latent_truth_label_distribution`, `majority_latent_conflict`, `evaluator_effective_n`, `correlated_evaluator_warning`, `latent_truth_converged`, `latent_truth_iterations`, `latent_truth_convergence_delta`, `latent_truth_demotion_reason` to the output. `latent_truth_confidence` is EM's fitted posterior, which saturates toward 0/1 with only a few evaluators and can be confidently wrong when evaluators are correlated in their bias — it is not a correctness guarantee. `evaluator_effective_n`/`correlated_evaluator_warning` diagnose that same risk (Kish design effect on pairwise-correlated evaluator errors) but can only detect it when *other* evaluators reveal a correlated block's shared deviation — if the block captures the EM consensus outright, no warning fires; it's a self-consistency diagnostic, not an independent correctness check. `latent_truth_converged` is false when the EM loop exhausted its iteration cap without settling (`latent_truth_iterations` hits the cap, `latent_truth_convergence_delta` stays above the convergence epsilon) — the posterior is still valid but less certain; the same value applies to every sample scored in that batch, since EM runs once per batch. |

`MinRequirements` are always applied **after** the threshold comparison. For
`--decision-score latent-truth`, three additional opt-in, default-off safety demotions apply
after that: `--demote-on-correlated-warning` (demotes when `correlated_evaluator_warning`
fires), `--min-evaluator-effective-n <f64>` (demotes when `evaluator_effective_n` falls below
this absolute value, independent of the relative 70%-of-nominal warning above), and
`--demote-on-non-convergence` (demotes when `latent_truth_converged` is false). Each only ever
demotes `Keep` → `Review` — never further to `Drop`, since these are uncertainty signals, not
certainty-of-wrongness signals — and never touches `stability_score` itself. When a demotion
fires, `latent_truth_demotion_reason` names which condition (`correlated_evaluator_warning`,
`evaluator_effective_n_below_minimum`, or `latent_truth_not_converged`, in that priority order
if more than one fires at once). Available on `score` and `stable-wrong-risk`; no-op on
`calibrate`/`policy`/`compare --policy-after`, which compare raw scores directly rather than
reading `decision`.

| Condition | Decision |
|-----------|----------|
| score >= 0.85 | keep |
| score <= 0.40 | drop |
| otherwise | review |

This maps onto *selective classification* / *reject option* / risk-coverage tradeoffs: rather
than forcing every sample into a class, uncertain ones are routed to review to control the risk
of what's actually accepted. `keep` = accept, `review` = abstain/defer, `drop` = reject for
training use. `calibrate`/`policy` operationalize this — see
[calibrate](#calibrate-command) — but they pick a threshold empirically against one gold-labeled
sample, not a conformal-style formal guarantee; use `--heldout` for a more honest (still
empirical) precision estimate. Full discussion: [docs/metrics.md](docs/metrics.md#calibrate-and-policy-as-selective-classification).

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
rank stability:   0.8912  (Spearman's rho of stability_score ranks)

decision transitions (before → after):
              →keep   →review    →drop
      keep↓    7210       311       42
    review↓     508      2101      301
      drop↓      19       104      404

top 5 regressions:
  sample_001  0.9100 → 0.4400  (Δ-0.4700)
  sample_382  0.8800 → 0.3900  (Δ-0.4900)
```

`rank stability` is Spearman's rank correlation (average-rank tie handling) between the
before/after `stability_score` orderings among matched samples — `1.0` means the relative
ranking didn't change at all even if absolute scores drifted, `-1.0` means the ranking fully
reversed. Omitted (from both text and `--json` output) when fewer than 2 matched samples or
either side has zero variance (undefined). `--json` adds it as `rank_stability`.

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
  "precision_ci_low": 0.943,
  "precision_ci_high": 0.997,
  "coverage": 0.61,
  "n_keep": 610,
  "n_total": 1000,
  "validated_on_heldout": false,
  "note": "achieved_precision and its confidence interval were selected and measured on the same gold-labeled data; the threshold search makes this optimistic. Pass --heldout <path> for an independent estimate."
}
```

`calibrate` grid-searches `keep_threshold` from 0.99 down to 0.50 (step 0.01) and returns the
loosest threshold that meets the target. Requires `gold_label` on at least one observation per
sample. Returns an error if no threshold meets the target (try a lower `--target-precision`).

`precision_ci_low`/`precision_ci_high` is a Wilson score interval around `achieved_precision` at
`--confidence-level` (the same knob used for the `lcb` decision score) — it widens automatically
when few gold labels back the estimate, so don't treat `achieved_precision` alone as exact when
`n_keep` is small. The interval covers sampling noise at the chosen threshold only, not the fact
that the threshold itself was picked to hit the target on these same gold labels — it is still
optimistic. Treat it as a lower bar, not the full uncertainty; a held-out gold set is the more
conservative check. `note` makes this caveat machine-readable: it's always present when
`--heldout` was not used (as above), and present when `--heldout` was used but the training-set
precision at the selected threshold exceeded the held-out precision by more than 0.05 (a likely
overfit); `null` when `--heldout` was used and the gap was small enough to be ordinary sampling
noise.

Use `--heldout <path>` to get that more conservative check: `keep_threshold` is still selected
by grid search on the primary input, but `achieved_precision`/`precision_ci_low`/
`precision_ci_high`/`coverage`/`n_keep`/`n_total` are instead measured on the held-out file's own
`gold_label`s at that fixed threshold. For `raw`/`adjusted`/`lcb`/`latent-truth` this fully
removes the circularity — the threshold-selection circularity is gone, and latent-truth's EM
never consumes gold labels at all, so grading its inferred labels against held-out gold is a
genuine independent check even on the same samples. For `gold-weighted`, one layer of
circularity remains: the per-evaluator reliability weights are fit from the held-out file's own
`gold_label`s and then graded against those same labels, so treat its held-out precision as
better than the in-sample number but not fully independent. Not stdin-capable (`-`); the primary
input already claims stdin. Requires `gold_label` on the held-out file, or the command errors.
The reported precision can legitimately land *below* `--target-precision` when the
primary-input-selected threshold overfits — that's the correct, honest signal, not a bug.
`validated_on_heldout` in the output discloses whether `--heldout` was used, so a saved result
still says what it measured. `train_precision` exposes the in-sample precision at the same
threshold alongside `achieved_precision` (which becomes the held-out precision once `--heldout`
is used) — `null` unless `--heldout` was given, since otherwise it would just repeat
`achieved_precision`:

```bash
quietset calibrate input.jsonl --target-precision 0.95 --heldout heldout_gold.jsonl
```
```json
{
  "decision_score": "raw",
  "keep_threshold": 0.99,
  "drop_threshold": 0.40,
  "achieved_precision": 0.71,
  "precision_ci_low": 0.44,
  "precision_ci_high": 0.89,
  "coverage": 1.0,
  "n_keep": 24,
  "n_total": 24,
  "train_precision": 0.97,
  "validated_on_heldout": true,
  "note": "in-sample precision at this threshold was 0.970, held-out precision is 0.710 (gap 0.260) — the training-selected threshold likely overfit the training gold labels"
}
```

Use `--output-format csv` for a single header row + one data row instead of a JSON object:

```bash
quietset calibrate input.jsonl --target-precision 0.95 --output-format csv
```
```csv
decision_score,keep_threshold,drop_threshold,achieved_precision,precision_ci_low,precision_ci_high,coverage,n_keep,n_total,train_precision,validated_on_heldout,note
lcb,0.910000,0.400000,0.982000,0.943000,0.997000,0.610000,610,1000,,false,"achieved_precision and its confidence interval were selected and measured on the same gold-labeled data; the threshold search makes this optimistic. Pass --heldout <path> for an independent estimate."
```

Use `--fail-below-target` to make `calibrate` a CI gate: it exits non-zero (after still printing
the result) if `achieved_precision` falls short of `--target-precision`. Without `--heldout`
this is effectively a no-op — the grid search already guarantees `achieved_precision >=
--target-precision` on the primary input, or fails outright with an error. With `--heldout`,
`achieved_precision` is the held-out measurement, which can legitimately fall short — this flag
turns that honest signal into something a CI pipeline can act on:

```bash
quietset calibrate train.jsonl --heldout heldout.jsonl --target-precision 0.98 --fail-below-target
echo "exit code: $?"
```

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
| `low_score_consistency` | score_consistency < 0.7 → `reduce_score_variance` |

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

Use `--breakdown` to also report `stable_wrong_rate` per `evaluator_id`/`model_id`/`budget`
value among `Keep`-decision samples — spotting a specific evaluator/model/budget that shows up
disproportionately often in stably-wrong-but-kept samples:

```bash
quietset stable-wrong-risk input.jsonl --breakdown
```
```json
{
  "n_total": 1000,
  "n_keep": 621,
  "n_stable_wrong": 12,
  "stable_wrong_rate_among_keep": 0.019,
  "samples": [ ... ],
  "by_evaluator": [
    {"evaluator_id": "m1", "n_keep": 340, "n_stable_wrong": 9, "stable_wrong_rate": 0.026},
    {"evaluator_id": "m2", "n_keep": 281, "n_stable_wrong": 3, "stable_wrong_rate": 0.011}
  ],
  "by_model": [ ... ],
  "by_budget": [ ... ]
}
```

Each breakdown is sample-level, like the top-level rate: `by_evaluator[i].n_keep` counts `Keep`
samples that touch that specific `evaluator_id`, not observations. A sample can touch multiple
evaluators/models/budgets at once, so rows within a breakdown do **not** sum to the top-level
`n_keep` — that's intentional, not a partition; it answers "which evaluators/models/budgets show
up in wrong-but-confident samples," not "whose fault is it." Off by default, since it clones the
input and does an extra grouping pass.

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
threshold meeting that target with `←`. `--output-format json` emits one JSONL object per
threshold row instead of the formatted table (`--json` still works as an alias; passing both
warns and `--output-format` wins). `--output-format csv` emits one row per swept threshold with
fixed columns `threshold,n_keep,coverage,precision,stable_wrong_rate,best` — `precision`/
`stable_wrong_rate` are empty when no `gold_label` is present, and `best` is `true` only on the
row matching `--target-precision`/`--target-coverage`:

```bash
quietset policy input.jsonl --target-precision 0.95 --output-format csv
```
```csv
threshold,n_keep,coverage,precision,stable_wrong_rate,best
0.99,1,0.500000,1.000000,0.000000,
0.98,1,0.500000,1.000000,0.000000,true
```

## active-review command

Ranks scored JSONL samples by re-evaluation urgency — a weighted combination of low
`label_agreement_lcb`, high `label_entropy`, high `score_mad`, and high budget/seed sensitivity:

```bash
quietset score input.jsonl | quietset active-review -
quietset active-review scored.jsonl --unstable-only --top 20
```

```json
{"budget_sensitivity":0.625,"cost":1.0,"expected_coverage_gain":1.0,"expected_risk_reduction":0.0,"label_agreement_flip_ratio":0.818,"label_agreement_lcb":0.301,"label_entropy":0.811,"primary_reason":"high_entropy","sample_id":"b","seed_sensitivity":0.075,"suggested_action":"add_evaluator","urgency_score":0.502,"utility":0.818}
```

`--unstable-only` skips samples already decided `keep` with no instability signals. Per-signal
`--weight-*` flags (`--weight-lcb`, `--weight-entropy`, `--weight-score-mad`,
`--weight-budget-sensitivity`, `--weight-seed-sensitivity`, `--weight-order-sensitivity`,
`--weight-teacher-conflict`, `--weight-gradient-instability`) let you emphasize the signal most
relevant to your review budget. `suggested_action` is one of `request_gold_label`,
`add_evaluator`, `add_model`, `increase_budget`, `add_seed`, `add_shuffle_seed`,
`flag_teacher_conflict`, `add_init_seed` (the first five renamed from the earlier
`add_observations`/`diversify_evaluators`/`reduce_score_variance`/`add_budget`/`add_seeds` — a
breaking change, see CHANGELOG; the last three added for trajectory stability — see
[Trajectory stability fields](#trajectory-stability-fields-optional)). The 3 trajectory signals
only fire when the corresponding `Observation` fields (`shuffle_seed`, `teacher_residual`,
`gradient_sign`) were present when `score` ran:

| Signal | `primary_reason` | `suggested_action` | Cost flag |
|--------|-------------------|---------------------|-----------|
| Order-dependency (`shuffle_seed_sensitivity`) | `high_order_sensitivity` | `add_shuffle_seed` | `--cost-shuffle-seed` |
| Teacher conflict (`1 - teacher_residual_stability`) | `high_teacher_conflict` | `flag_teacher_conflict` | `--cost-gold-label` |
| Gradient instability (`1 - gradient_sign_agreement`) | `high_gradient_instability` | `add_init_seed` | `--cost-seed` |

Five additional fields turn the urgency heuristic into an expected-value ranking:

- `label_agreement_flip_ratio` — a distance-to-threshold ratio in `[0.0, 1.0]`: `1.0` exactly
  at `--keep-threshold`/`--drop-threshold`, shrinking toward `0.0` further away, relative to
  `label_agreement`'s own Wilson-CI margin of error. **Not a probability** — only
  `label_agreement`'s sampling uncertainty is modeled; `stability_score`'s other components
  (score dispersion, budget/seed sensitivity, model/evaluator agreement) have no uncertainty
  model in this codebase, so this ratio is a loose proxy whenever those other components
  actually drive `stability_score`. `None` when the sample has no labels.
- `expected_coverage_gain` — `1 / n_total` if the sample's `decision` (read verbatim from the
  input, not re-derived) is not `keep`, else `0.0`: the exact coverage change if this sample's
  decision flipped to `keep`.
- `expected_risk_reduction` — `1 / n_keep` if the sample is a `keep` whose
  `label_agreement_lcb` falls below `--keep-threshold` (the same "at-risk keep" condition
  `stable-wrong-risk`'s `lcb_keep_demotions` counts), else `0.0`.
- `cost` — the cost of `suggested_action`, from `--cost-seed`/`--cost-budget`/
  `--cost-evaluator`/`--cost-gold-label` (all default `1.0`; `--cost-evaluator` also backs
  `add_model`).
- `utility` — `label_agreement_flip_ratio * (expected_coverage_gain + expected_risk_reduction)
  / cost`. A ranking score, not a literal expected value — ordering by it is sound (closer to
  threshold, higher payoff, cheaper actions rank higher), but don't read the absolute number as
  a calibrated expectation.

`--keep-threshold`/`--drop-threshold` (defaults `0.85`/`0.40`, matching `score`'s own defaults)
feed only `label_agreement_flip_ratio` and the at-risk-keep gate — they do not re-derive
`decision`. `--rank-by urgency` (default) sorts by `urgency_score`; `--rank-by utility` sorts by
`utility` instead, surfacing samples where additional review effort pays off most rather than
samples that merely look unstable.

Use `--plan <path>` to write a budget-constrained evaluation plan — a JSONL file, always sorted
by `utility` descending regardless of `--rank-by`, in addition to the normal stdout output.
`--budget <f64>` caps the total `cost` of entries it includes: entries are taken greedily by
utility, skipping (not stopping at) any that don't fit so a cheaper lower-utility entry later in
the list can still be included. Without `--budget`, `--plan` writes every entry (still
utility-sorted):

```bash
quietset active-review scored.jsonl --plan next_eval_plan.jsonl --budget 20.0
```

This is a standard greedy value-density approximation for a knapsack-style budget allocation
problem — not guaranteed globally optimal, but sound for "what should the next batch of review
effort go toward."

Pass `--observations <path>` (the same raw observation JSONL `score` was run on) to also size a
concrete target for `suggested_action` — `StabilityReport` only carries derived stats
(sensitivity/robustness scores), not the raw budget/seed/evaluator/model values needed to size
one, the same gap `audit --observations` fills for agreement stats:

```bash
quietset active-review scored.jsonl --observations input.jsonl
```

```json
{"cost":1.0,"expected_coverage_gain":1.0,"expected_risk_reduction":0.0,"label_agreement_flip_ratio":0.766,"label_agreement_lcb":0.510,"label_entropy":0.0,"primary_reason":"high_seed_sensitivity","sample_id":"pricey","seed_sensitivity":0.8,"suggested_action":"add_seed","target_seed":2,"urgency_score":0.423,"utility":0.766}
```

Only the field matching `suggested_action` is populated, from that sample's own observations:

- `increase_budget` → `target_budget` = the sample's max observed `budget` × 2
- `add_seed` / `add_init_seed` → `target_seed` = the sample's max observed `seed` + 1 (both
  actions size the same `seed` field — "run one more seed")
- `add_shuffle_seed` → `target_shuffle_seed` = the sample's max observed `shuffle_seed` + 1
- `add_evaluator` → `target_evaluator_slot` = the sample's distinct `evaluator_id` count + 1
- `add_model` → `target_model_slot` = the sample's distinct `model_id` count + 1
- `request_gold_label` / `flag_teacher_conflict` → no target field (nothing to size for a
  review/label request)

All fields are omitted without `--observations`, and omitted per-sample whenever the
relevant raw value is missing from the observations file — never a fabricated default.

## block-score command

Groups observations by `block_id` (e.g. a 32-position training block) instead of `sample_id`
and classifies each block's trajectory stability:

```bash
quietset block-score observations.jsonl
```

```json
{"block_id":"B5","n_samples":2,"n_observations":4,"seed_effect_consistency":1.0,"shuffle_direction_consistency":1.0,"checkpoint_reproducibility":1.0,"block_stability":1.0,"trajectory_effect_mean":-0.45,"dead_unit_rate":0.5,"classification":"stable_shrink"}
```

`block_stability = seed_effect_consistency × shuffle_direction_consistency ×
checkpoint_reproducibility` — the product of whichever of those three factors are computable
(each requires ≥2 distinct groups on its axis: `seed`, `shuffle_seed`, `model_id`
respectively). `classification` is one of:

| Class | Meaning |
|-------|---------|
| `stable_growth` | `block_stability >= --stable-threshold` and net `trajectory_effect_mean > 0` |
| `stable_shrink` | `block_stability >= --stable-threshold` and net `trajectory_effect_mean < 0` |
| `seed_sensitive` | Unstable, and `seed_effect_consistency` is the lowest sub-threshold factor |
| `order_sensitive` | Unstable, and `shuffle_direction_consistency` is the lowest sub-threshold factor |
| `trajectory_sensitive` | Unstable, and `checkpoint_reproducibility` is the lowest sub-threshold factor |
| `pathological` | `dead_unit_rate`/`saturated_unit_rate` exceeds `--pathological-dead-rate` — overrides every other signal |
| `insufficient` | Not enough condition-axis diversity (or no `trajectory_effect` to read a growth/shrink sign from) |

**`stable_growth`/`stable_shrink` are not "good"/"bad" labels** — they describe reproducibility
of `trajectory_effect`'s sign, not whether that effect is desirable. Growth is not always an
improvement; shrink is not always a regression (a shrink with rising dead/saturated-unit rates
is caught separately by `pathological`, but a clean shrink could be healthy regularization).
Recommended handling, pending your own held-out/downstream validation:

| Class | Suggested handling |
|-------|---------------------|
| `pathological` | Exclude from training |
| `insufficient` | Collect more observations before deciding |
| `seed_sensitive` | Add another seed and re-classify |
| `order_sensitive` | Add another shuffle seed and re-classify |
| `trajectory_sensitive` | Re-evaluate at another checkpoint |
| `stable_growth` | Send to downstream validation — do not auto-keep |
| `stable_shrink` | Send to downstream validation — do not auto-drop |

`--stable-threshold` (default `0.85`), `--sensitivity-threshold` (default `0.5`), and
`--pathological-dead-rate` (default `0.5`) tune the classification, same "operational default,
not a derived constant" status as `score`'s `--keep-threshold`/`--drop-threshold` — **not a
calibrated probability of anything** (in particular, not of a downstream playing-strength
improvement); validate against your own held-out data and match results before trusting it to
gate training data unattended. See [docs/metrics.md](docs/metrics.md) for why the three
consistency factors are multiplied rather than averaged. Warns on stderr (doesn't error) if no
observation carries `block_id`.

## trajectory-audit command

Diffs block-level trajectory stability between a before/after observation file (e.g. two
training checkpoints) — the destructive-sample detector:

```bash
quietset trajectory-audit --before checkpoint_143.jsonl --after checkpoint_176.jsonl --group-size 32
```

```
B5  stable_shrink → pathological
  dead_unit_delta[ft]:       +1.0000
  trajectory_effect_delta:      -0.4500
```

Matches blocks by `block_id` (blocks present on only one side are skipped) and reports, per
matched block: `dead_unit_delta`/`saturated_unit_delta` (per `layer_id`, `after_rate -
before_rate`; a layer seen on only one side is treated as `0.0` on the other, so a brand-new
problem layer still shows its full rate as the delta), `trajectory_effect_delta`,
`before_classification`/`after_classification`, and `reproducibility_3seed`
(`seed_effect_consistency` on the after side, only when ≥3 distinct `seed` values are present
there). Sorted by the largest dead/saturated-unit delta magnitude descending — most-destructive
blocks first. `--json` for machine-readable JSONL output (one object per block).

`--group-size N` warns on stderr for any block whose distinct `sample_id` count doesn't match
`N` on either side — a malformed block grouping (e.g. a truncated 32-position block), not a
destructive-training signal. Same `--stable-threshold`/`--sensitivity-threshold`/
`--pathological-dead-rate` flags as `block-score`.

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

## Where these numbers come from

quietset is not an implementation of one paper. Most of its per-sample building blocks are
standard, named statistics; `stability_score` and the keep/review/drop policy built on top of
them are quietset's own design — useful, but not a theoretical guarantee. This table is a
compact classification; for the full formula, assumptions, and failure modes behind each
metric, see [docs/metrics.md](docs/metrics.md).

**Established statistics** (faithful implementations, full formulas in the sections linked):

| Field | Basis |
|-------|-------|
| `label_agreement_lcb` | Wilson score interval lower bound — see [Stability score](#stability-score) |
| `fleiss_kappa` | Fleiss' kappa, generalized to variable raters per subject — see [reliability](#reliability-command-experimental) |
| `krippendorff_alpha` | Krippendorff's alpha, nominal-labels variant — see [reliability](#reliability-command-experimental) |
| `label_entropy` | Normalized Shannon entropy over the labels observed for that sample — see [Stability score](#stability-score) |
| `score_std` / `score_mad` / `score_iqr` | Population standard deviation / median-based MAD / linear-interpolation IQR — see [Stability score](#stability-score) |

**Model-based estimator:**

| Field | Basis |
|-------|-------|
| `latent_truth_*` | Dawid-Skene-style EM — per-evaluator confusion matrix + label posterior, fit with no `gold_label` needed. `evaluator_effective_n` and `correlated_evaluator_warning` are quietset's own diagnostics computed on top of the converged EM output, not part of the original Dawid-Skene method — see [Decisions](#decisions) |

**Empirical diagnostics** (measured directly against `gold_label`, not modeled):

| Field | Basis |
|-------|-------|
| `stable_wrong_rate_among_keep` | Direct empirical rate — see [stable-wrong-risk](#stable-wrong-risk-command) |
| `stable-wrong-risk --breakdown` | Same rate, grouped by evaluator/model/budget — see [stable-wrong-risk](#stable-wrong-risk-command) |

**quietset-specific heuristics** (compose the above into an operational decision — not citable
statistical methods on their own):

| Field | Basis |
|-------|-------|
| `stability_score` | Weighted mean of the sub-scores above — see [Stability score](#stability-score) |
| `confidence` / `adjusted_stability_score` | Evidence-count shrinkage toward neutral 0.5 — see [Confidence and adjusted score](#confidence-and-adjusted-score) |
| `score_sign_agreement` | Domain-aware complement metric for signed, zero-centered scores — see [Stability score](#stability-score) |
| `keep` / `drop` thresholds (0.85 / 0.40) | Operational defaults, not derived constants — see [Decisions](#decisions) |

`calibrate`/`policy` are conceptually close to selective-prediction / risk-coverage ideas
(reject uncertain samples to review rather than forcing a keep-or-drop call on everything) —
but they are **not** a conformal-style formal guarantee. A threshold is chosen empirically
against one gold-labeled sample, so treat `achieved_precision` as *measured*, not *proven*; see
the in-sample-optimism `note` and `--heldout` explanation in [calibrate](#calibrate-command).

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
It is excluded from the Cargo workspace and from CI (`cargo build/test --workspace` never touches
it); build and test it manually from within `crates/quietset-py`.

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
