# Research map: candidate truth models

This is a note, not a roadmap commitment. It exists so a future contributor (or Claude) doesn't
re-derive "should quietset add model X" from scratch — the candidates below were considered and
explicitly **not** added yet, for stated reasons. None of this is scheduled; each is a candidate
to reconsider only if concretely requested (the same "deferred is not a permanent rejection"
pattern already used for held-out calibration and EV-based active-review in
`tasks/lessons.md` — a deferred item's status changes when someone asks for it by name, not on
its own timeline).

## Already implemented, for context

- **Dawid-Skene EM** (`--decision-score latent-truth`, `crates/quietset/src/latent_truth.rs`) —
  per-evaluator confusion-matrix reliability + label posterior, no `gold_label` needed. See
  [metrics.md](metrics.md#latent_truth_-dawid-skene-em).

## Candidates, not implemented

- **MACE** (Multi-Annotator Competence Estimation) — estimates each annotator's competence and
  the latent true label together, explicitly modeling that some annotators may be
  spammers/adversarial rather than just noisy.
- **GLAD** (Generative model of Labels, Abilities, and Difficulties) — extends Dawid-Skene-style
  estimation with a per-item difficulty parameter, not just per-annotator reliability.
- **Snorkel label model** — a weak-supervision approach that denoises multiple noisy "labeling
  functions" without ground truth, explicitly modeling unknown accuracies and *correlations*
  between sources.
- **Bayesian annotator models** (broader family) — full posterior/uncertainty treatment of
  annotator reliability rather than point estimates.

### Why not now

No concrete case has come up where Dawid-Skene's independence assumption is known to fail
*and* one of these specifically fixes it for a quietset user. Adding any of them speculatively
would be exactly the kind of premature abstraction `AGENTS.md`'s non-goals already warn against
(a config surface / dependency for a use case nobody has hit yet) — see `tasks/lessons.md`'s
repeated theme that speculative generality gets rejected until there's a specific reason.

### Conditions for reconsidering any of these

All of the following, not just one:

- No heavy dependency required — implementable in a small amount of pure Rust, matching how
  `latent_truth.rs` is hand-rolled with no new crate.
- Failure modes can be stated as plainly as Dawid-Skene's correlated-evaluator blind spot
  already is.
- A synthetic test can show a concrete case where it beats majority vote / current
  `latent-truth` — not just "more sophisticated," an actual win on a constructible fixture.
- It doesn't expand the CLI surface into a config-DSL — one new `DecisionScore` variant plus
  flags, the same shape `GoldWeighted` and `LatentTruth` already used, not a new abstraction
  layer (a `--truth-model` selector layered over `--decision-score` was considered and not
  pursued this round for exactly this reason — the existing enum already extends cleanly, as
  proven twice by `GoldWeighted` and `LatentTruth`).

## Explicitly not on this list

Changing quietset's *default* `--decision-score` away from `raw`, or an EM-confidence
calibration layer (Platt scaling / isotonic regression / temperature scaling) for
`latent_truth_confidence`/`weighted_label_confidence` — both were raised in discussion but
dropped before becoming a concrete ask. If either comes up again, treat it as a fresh,
default-changing decision requiring explicit confirmation (see `tasks/lessons.md`'s
"score_sign defaulting to 0 instead of 1" and "game-ai profile default change" entries for why
this project treats default changes carefully), not something this document pre-approves.
