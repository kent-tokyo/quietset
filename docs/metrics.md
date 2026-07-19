# Metric reference

Deep reference for every number quietset outputs: formula, provenance, interpretation,
assumptions, failure modes, and how quietset uses it. For a compact classification table (which
metrics are established statistics vs. quietset-specific heuristics), see
[README.md#where-these-numbers-come-from](../README.md#where-these-numbers-come-from). This
file is the detailed version that table points to — update both together if a metric changes,
but keep the formula/assumptions/failure-mode depth here, not duplicated there.

## label_agreement_lcb

- **Formula**: Wilson score interval lower bound of `label_agreement` (`crates/quietset/src/scoring.rs:23-43`).
- **Provenance**: Wilson score interval (Wilson, 1927) for a binomial proportion.
- **Interpretation**: a conservative lower bound on the true label-agreement rate, given `n`
  observations. Closer to raw `label_agreement` as `n` grows.
- **Assumptions**: treats repeated observations as independent Bernoulli trials in support of
  the majority label.
- **Failure modes**: the interval's `z` quantile comes from a rational approximation to the
  inverse normal CDF, not an exact quantile function (negligible in practice). More importantly:
  Wilson corrects for small-sample *noise*, not for *correlated* evaluators — if evaluators
  share a systematic bias, the input proportion itself is inflated before Wilson ever sees it,
  so a high LCB is not protection against correlated bias.
- **quietset usage**: `--decision-score lcb`; `label_agreement_lcb` is one of the "established
  statistics" candidates preferred over raw `stability_score` for real accept/reject decisions.

## fleiss_kappa

- **Formula**: observed per-subject agreement `P_i` averaged to `P̄`, chance agreement `P_e`
  from marginal category proportions, `κ = (P̄ - P_e) / (1 - P_e)` (`crates/quietset/src/agreement.rs:8-55`).
- **Provenance**: Fleiss' kappa (Fleiss, 1971).
- **Interpretation**: `0` = agreement at chance level, `1` = perfect agreement, negative = worse
  than chance.
- **Assumptions**: nominal (unordered) categories. This implementation generalizes the classic
  fixed-raters-per-subject formula to a variable number of raters per subject (subjects with
  fewer than 2 ratings are excluded) — a standard, reasonable extension, but numbers won't
  bit-match tools that require equal raters per subject.
- **Failure modes**: corrects only for chance agreement, not for differences in evaluator
  competence — a kappa of 1.0 among evaluators who are all wrong in the same way still reads as
  perfect agreement.
- **quietset usage**: `reliability` command, dataset-level diagnostic (trailing JSONL line).

## krippendorff_alpha

- **Formula**: coincidence-matrix derivation — `Do = 1 - e_diag/n`, `De = (n² - Σn_c²) /
  (n(n-1))`, `α = 1 - Do/De` (`crates/quietset/src/agreement.rs:62-109`).
- **Provenance**: Krippendorff's alpha.
- **Interpretation**: a more general reliability coefficient than kappa, same scale, more
  tolerant of missing data and variable raters per subject.
- **Assumptions**: only the **nominal** distance function is implemented (any label mismatch
  counts as full disagreement) — no ordinal/interval/ratio variant exists in this codebase.
- **Failure modes**: describing this as "Krippendorff's alpha" without the "nominal" qualifier
  overstates what's implemented; there's no support for ordered or numeric label distance.
- **quietset usage**: `reliability` command, alongside `fleiss_kappa`.

## label_entropy

- **Formula**: Shannon entropy over the label distribution, `-Σp·log2(p)`, normalized by
  `log2(count of distinct labels observed for that sample)` (`crates/quietset/src/scoring.rs:135-145`).
- **Provenance**: Shannon entropy.
- **Interpretation**: `0.0` = unanimous, `1.0` = uniform disagreement across whatever labels
  that sample happened to receive.
- **Assumptions/failure modes**: normalized against the labels *observed for that sample*, not
  a global label-space size. A sample that only ever saw 2 possible labels maxes out at `1.0`
  the same as one that saw 5 — the raw value isn't directly comparable across samples with
  different numbers of distinct labels observed.
- **quietset usage**: diagnostic field; feeds `active-review`'s `high_entropy` signal.

## score_std / score_mad / score_iqr

- **Formula**: `score_std` = population standard deviation (`n` divisor, no Bessel correction);
  `score_mad` = median absolute deviation (median of `|s - median|`); `score_iqr` = `Q3 - Q1`
  via linear interpolation between order statistics, matching NumPy's/R's default quantile
  method (`crates/quietset/src/scoring.rs:167-190`).
- **Provenance**: standard dispersion statistics.
- **Interpretation**: how spread out numeric scores are for a sample; feeds the
  `score_consistency` stability component.
- **Assumptions/failure modes**: `score_std` has no Bessel correction, so it slightly
  underestimates the true variance at small `n` (matters more below ~10 observations); MAD/IQR
  are more robust to outliers by design, at the cost of being less sensitive to small,
  consistent shifts.
- **quietset usage**: `--score-dispersion std|mad|iqr` selects which one feeds
  `score_consistency`.

## latent_truth_* (Dawid-Skene EM)

- **Formula**: expectation-maximization over per-evaluator confusion matrices and a label
  prior, Laplace-smoothed, log-space E-step, 20-iteration cap
  (`crates/quietset/src/latent_truth.rs:54-381`).
- **Provenance**: Dawid & Skene (1979) — the classic method for estimating latent true labels
  and per-rater error rates without ground truth.
- **Interpretation**: `latent_truth_label`/`latent_truth_confidence`/
  `latent_truth_label_distribution` — a reliability-weighted vote inferred purely from
  disagreement patterns, no `gold_label` required. `latent_truth_converged`/
  `latent_truth_iterations`/`latent_truth_convergence_delta` disclose whether EM actually
  settled or hit its iteration cap.
- **Assumptions**: evaluator errors are independent *given* the true label — the central
  Dawid-Skene assumption.
- **Failure modes**: correlated evaluator errors violate that independence assumption and can
  produce a confidently-wrong posterior — `latent_truth_confidence` is EM's own fitted
  posterior, not an external correctness check. `evaluator_effective_n` (a Kish design effect
  over pairwise phi-correlation) and `correlated_evaluator_warning` are **quietset-specific
  diagnostics computed on top of the converged EM output**, not part of the original
  Dawid-Skene paper — they can only detect a correlated block of evaluators when *other*
  evaluators reveal its shared deviation; if the correlated block *is* the EM consensus, no
  warning fires (a documented blind spot).
- **quietset usage**: `--decision-score latent-truth`. The safety-demotion flags
  (`--demote-on-correlated-warning`, `--min-evaluator-effective-n <f64>`,
  `--demote-on-non-convergence`) downgrade a would-be `Keep` to `Review` — never `Drop` — when
  these diagnostics fire, because they're uncertainty signals about EM's own fit, not evidence
  that the label itself is wrong (see `crates/quietset/src/lib.rs` around the demotion
  application site for the code-level version of this reasoning).

## stable_wrong_rate_among_keep

- **Formula**: fraction of `Keep`-decision samples whose majority/weighted/latent label
  disagrees with `gold_label`.
- **Provenance**: none — a direct empirical measurement, not a modeled estimate.
- **Interpretation**: how often data that looks "confidently stable" is actually wrong, given
  ground truth.
- **Assumptions**: requires `gold_label`; measured in-sample unless the input itself is a
  held-out split you control.
- **Failure modes**: the `gold-weighted` decision-score variant is circular here — per-evaluator
  reliability weights are fit on the same gold labels the resulting vote is then graded against,
  so `stable_wrong_rate` can read artificially low (see `tasks/lessons.md` for the full
  circularity chain shared with `calibrate`/`policy`).
- **quietset usage**: `stable-wrong-risk` command; `--breakdown` attributes the rate to specific
  evaluators/models/budgets.

## stability_score

- **Formula**: weighted mean of whatever sub-scores are available for a sample — `label`,
  `score_consistency`, `score_sign_agreement`, `budget_robustness`, `seed_robustness`,
  `model_agreement`, `evaluator_agreement` — missing dimensions excluded from both numerator and
  denominator (`crates/quietset/src/scoring.rs:225-280`).
- **Provenance**: none — a quietset-specific composite aggregate.
- **Interpretation**: a single `[0,1]` ranking/inspection score, not a probability of anything.
- **Assumptions**: the sub-scores are commensurable enough to average linearly; single
  observations get a fixed `0.5` (review by default) rather than an extrapolated extreme.
- **Failure modes**: **not a calibrated probability and not a correctness guarantee** — a
  sample where every evaluator consistently agrees on a wrong answer scores `1.0`. It has no
  uncertainty model of its own (unlike `label_agreement_lcb`).
- **quietset usage**: the default score `filter`/thresholds act on. For real accept/reject
  decisions, prefer `label_agreement_lcb`, `latent-truth`, `gold-weighted`, or a `calibrate
  --heldout`-selected threshold instead of raw `stability_score` — see
  [README.md#stability-score](../README.md#stability-score).

## confidence / adjusted_stability_score

- **Formula**: `confidence = n / (n + k)` (`k` defaults to `3.0`); `adjusted_stability_score =
  stability_score * confidence + 0.5 * (1 - confidence)` (`crates/quietset/src/scoring.rs:282-283`).
- **Provenance**: none — a Bayesian-shrinkage-*flavored* heuristic, not a named, citable
  estimator.
- **Interpretation**: shrinks a high `stability_score` backed by few observations toward the
  neutral midpoint `0.5`.
- **Assumptions**: `k = 3.0` is a tuned default, not derived from any target false-positive rate.
- **Failure modes**: unlike Wilson's LCB, this doesn't model the actual sampling distribution of
  `stability_score` — two samples with the same `n` get the same shrinkage regardless of how
  extreme or well-behaved their underlying statistic is.
- **quietset usage**: `--decision-score adjusted` / `--use-adjusted-score`.

## score_sign_agreement

- **Formula**: `max(count_negative, count_zero, count_positive) / total`
  (`crates/quietset/src/scoring.rs:192-206`).
- **Provenance**: none — a domain-specific heuristic for signed, zero-centered scores
  (chess/shogi-style evaluations).
- **Interpretation**: complements magnitude-based dispersion (`score_std`/`score_mad`/
  `score_iqr`) for scores where the *sign* carries meaning independent of magnitude —
  `[+0.01, -0.01]` reads as near-perfectly stable by magnitude alone but is a coin-flip on sign.
- **Failure modes**: zero-valued scores form their own third bucket, not merged into either
  side — a batch with many exact zeros can make "zero" the majority sign.
- **quietset usage**: always computed; excluded from `stability_score` by default
  (`--weight-score-sign 0`); `--profile game-ai` weights it `×2` by default.

## Trajectory stability metrics (gradient_sign_agreement, update_direction_agreement, teacher_residual_stability, shuffle_seed_sensitivity, loss_recipe_agreement)

- **Formula**: each reuses an existing generic verbatim, just applied to a new field —
  `gradient_sign_agreement`/`update_direction_agreement` reuse the `sign_agreement` helper
  `score_sign_agreement` already uses; `teacher_residual_stability` is
  `1 - normalized_std(teacher_residual)`, the same shape as `score_consistency`;
  `shuffle_seed_sensitivity` reuses `compute_range_sensitivity` keyed on `shuffle_seed` instead
  of `seed`; `loss_recipe_agreement` reuses `compute_group_label_agreement` keyed on
  `loss_recipe` instead of `model_id`/`evaluator_id` (`crates/quietset/src/scoring.rs`).
- **Provenance**: none — domain-agnostic heuristics for harness-supplied trajectory signals,
  same status as `score_sign_agreement`.
- **Interpretation**: cross-run consistency of a training-trajectory signal the harness
  computed upstream (quietset never touches a gradient or checkpoint itself — see
  `docs/research-map.md` for why this stays outside quietset's non-goals). `None` when the
  underlying `Observation` field is absent.
- **Assumptions/failure modes**: same as the metrics they reuse — `sign_agreement` treats a
  zero value as its own third bucket (see `score_sign_agreement` above); `shuffle_seed_sensitivity`
  needs ≥2 distinct `shuffle_seed` values to be computable, same gate as `seed_sensitivity`.
- **quietset usage**: all five are computed whenever their input field is present, but excluded
  from `stability_score` by default (`ScoreWeights::{gradient_sign,update_cosine,
  teacher_residual,shuffle_seed,loss_recipe}` all default `0.0`, exactly like `score_sign`) —
  opt in via `--weight-gradient-sign`/`--weight-update-cosine`/`--weight-teacher-residual`/
  `--weight-shuffle-seed`/`--weight-loss-recipe`.

## dead_unit_rate / saturated_unit_rate / trajectory_effect_mean / trajectory_effect_sign_agreement

- **Formula**: `dead_unit_rate`/`saturated_unit_rate` = fraction of observations carrying
  `dead_unit_count`/`saturated_unit_count` where the value is `> 0.0`;
  `trajectory_effect_mean`/`trajectory_effect_sign_agreement` = mean and `sign_agreement` of
  `trajectory_effect` (`crates/quietset/src/scoring.rs`).
- **Provenance**: none — diagnostic-only fields, not part of `stability_score`.
- **Interpretation**: how often a sample induces new dead/saturated units, and whether its net
  training effect (e.g. a radial-norm delta) is growing or shrinking. These feed `block-score`'s
  classification (pathological override, growth/shrink sign) rather than per-sample stability.
- **quietset usage**: `block-score`/`trajectory-audit` (see below); no `ScoreWeights` field
  exists for these — they never enter `stability_score`.

## block_stability and BlockClass classification

- **Formula**: `block_stability = seed_effect_consistency × shuffle_direction_consistency ×
  checkpoint_reproducibility` — the product of whichever of those three
  cross-group-sign-agreement factors are computable (each needs ≥2 distinct groups on its
  axis). Classification is a first-match-wins decision tree: a pathological dead/saturated rate
  overrides everything; else `block_stability >= stable_threshold` with a signed
  `trajectory_effect_mean` gives growth/shrink; else the lowest sub-`sensitivity_threshold`
  factor (fixed tie-break order seed → shuffle → checkpoint) names the cause
  (`crates/quietset/src/block.rs`).
- **Provenance**: none — a quietset-specific composite, the block-level analogue of
  `stability_score`. **Not a calibrated probability of anything** — in particular, not a
  calibrated probability of a downstream playing-strength improvement. The three terms are
  multiplied, not averaged, because block stability is treated as requiring *all three* forms
  of reproducibility — a high value on one axis should not be able to compensate for
  instability on another (see the code comment at the multiplication site in
  `compute_block_report`). This is a design choice, not a derivation: it makes the score
  strongly punish a single weak dimension, which is the intended behavior, but it is not a
  statistically fitted or cross-validated model of anything.
- **Interpretation**: whether a training block's effect is consistent across seed/shuffle/
  checkpoint condition axes, and if not, which axis is most responsible.
- **Assumptions/failure modes**: a block can be classified `insufficient` even when every
  individual consistency factor is above `sensitivity_threshold`, if their *product* still
  undercuts `stable_threshold` — this is reported honestly rather than guessing which axis is
  "the" cause (see the doc comment on `block::classify`). `stable_threshold` (`0.85`),
  `sensitivity_threshold` (`0.5`), and `pathological_dead_rate` (`0.5`) are operational
  defaults, not universal constants, same status as `score`'s `0.85`/`0.40` — validate them (and
  the classification as a whole) against your own held-out data and, for a game engine, actual
  match results, before trusting it to gate training data unattended.
- **`StableGrowth`/`StableShrink` are not "good"/"bad" labels.** They describe *reproducibility*
  of `trajectory_effect`'s sign, not whether that effect is desirable — growth is not always an
  improvement, and shrink is not always a regression (a shrink with rising
  `dead_unit_rate`/`saturated_unit_rate` is caught separately by `Pathological`, but a clean
  shrink could be healthy regularization). Recommended handling per class, pending outcome
  validation rather than automatic action on the label alone:

  | Class | Suggested handling |
  |-------|---------------------|
  | `pathological` | Exclude from training |
  | `insufficient` | Collect more observations before deciding |
  | `seed_sensitive` | Add another seed and re-classify |
  | `order_sensitive` | Add another shuffle seed and re-classify |
  | `trajectory_sensitive` | Re-evaluate at another checkpoint |
  | `stable_growth` | Send to downstream validation — do not auto-keep |
  | `stable_shrink` | Send to downstream validation — do not auto-drop |

- **quietset usage**: `block-score` and `trajectory-audit` commands.

## keep / drop thresholds (0.85 / 0.40)

- **Formula**: none — `Thresholds::default()` (`crates/quietset/src/decision.rs:11-18`).
- **Provenance**: none — plain, user-overridable operational defaults.
- **Interpretation**: leaves a deliberate review band between a confident keep and a confident
  drop.
- **Assumptions/failure modes**: not calibrated to any specific target precision or error rate —
  they're reasonable general-purpose defaults, not a derived constant. If you need a threshold
  with a *measured* precision guarantee, use `calibrate` (ideally with `--heldout`) instead of
  the raw defaults.
- **quietset usage**: `--keep-threshold`/`--drop-threshold` on `score` and related commands.

## calibrate and policy as selective classification

`calibrate`/`policy` are conceptually close to *selective classification* / *reject option* /
risk-coverage tradeoffs: rather than forcing every sample into a class, uncertain ones are
routed to `review` (abstain) to control the risk of what's actually accepted. In this framing:
`keep` = accept, `review` = abstain/defer, `drop` = reject for training use.

They are **not** a conformal-prediction-style formal guarantee, though. A threshold is chosen
empirically by grid search against one gold-labeled sample; `achieved_precision` on that same
sample is optimistic because the threshold was selected against it (see the in-sample-optimism
`note` field and the `--heldout` explanation in
[README.md#calibrate-command](../README.md#calibrate-command)). Treat `achieved_precision` as
*measured*, not *proven* — `--heldout` gets you a more honest, but still empirical, estimate.
