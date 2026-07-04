# quietset — lessons learned

## Stability ≠ correctness — say it first

The most important thing to document upfront is that quietset measures *agreement across runs*,
not ground truth. A sample where all evaluators consistently give the wrong answer scores 1.0.
"stable ≠ correct" must appear before any feature description, not buried in an advanced section.

## Wilson LCB requires ~22 observations to clear 0.85 at 95% confidence

For a sample with 100% label agreement (all observations match), the Wilson LCB at 95%
confidence exceeds the default keep threshold of 0.85 only at n ≥ 22. This means:
- 2/2 → LCB ≈ 0.34
- 5/5 → LCB ≈ 0.57
- 10/10 → LCB ≈ 0.72
- 22/22 → LCB ≈ 0.85

Tests that assumed "5 matching observations → LCB above threshold" will fail. Design test
fixtures around this math, not intuition.

## lcb_keep_demotions semantics: raw-keep AND lcb-reject, not just lcb-reject

The useful metric is "samples that would be demoted from keep if you switched to LCB mode",
not "all samples with low LCB". A sample already in review by raw score is not a demotion —
it was never going to be kept. The correct filter is:

    stability_score >= keep_threshold AND label_agreement_lcb < keep_threshold

## Boolean flag pairs have implicit priority — enum is cleaner

`--use-adjusted-score` and `--use-lcb-score` as independent booleans require an `if/else`
chain to resolve conflicts. When a third mode is added, the chain grows silently. An enum
flag (`--decision-score raw|adjusted|lcb`) makes priority explicit and help text
self-documenting. Boolean aliases can stay for backwards compatibility.

## quietset-py excluded from workspace needs its own Dependabot entry

Cargo Dependabot scans only workspace members by default. `quietset-py` is excluded from the
workspace (because maturin requires its own `Cargo.toml` structure), so it needs a separate
`package-ecosystem: cargo` entry in `dependabot.yml` pointing to `/crates/quietset-py`.

## GITHUB_TOKEN permissions should be explicit in every workflow

GitHub Actions defaults to over-permissive `GITHUB_TOKEN` unless `permissions:` is set.
A read-only workflow (checkout + cargo + audit) needs only `contents: read`. Set it at the
workflow level so all jobs inherit it; override per-job only if write access is genuinely needed.

## Kappa/alpha "chance level" ≠ kappa=0 for systematic disagreement

Fleiss' kappa = 0 means "observed agreement equals expected agreement by chance". But two
raters who *always* disagree on every subject produce kappa = -1.0, not 0. Similarly,
Krippendorff's alpha for perfectly antisymmetric data gives alpha = -0.5 (not -1.0 and not 0).
Test fixtures must be derived from the math, not from intuition about what "disagree" means.

## Profile presets need weight fields to be Option<f64>, not f64 with defaults

When CLI weight flags have `default_value_t = 1.0`, there is no way to distinguish "user
explicitly passed 1.0" from "clap used the default". Profiles can only be overridden by
explicit flags if the fields are `Option<f64>` with no default — then `None` means "use
profile default or fall back to 1.0", and `Some(v)` means "user explicitly set this".

## calibrate: stable-but-wrong samples defeat precision targets unless threshold is very high

When structurally stable samples have the wrong label (e.g., all 5 evaluators say "loss" but
gold is "win"), their stability_score equals that of correct samples. No threshold discriminates
between them — precision is capped at the fraction of stable samples that happen to be correct.
The `calibrate` command will find the highest-threshold bucket that meets the target; if that
bucket includes wrong-but-stable samples, precision will be lower than the target and the
search may return None. This is expected and correct: calibration cannot fix stable-wrong data.

## audit/select borderline band should track --keep-threshold, not be hardcoded

The borderline band was initially hardcoded as [0.75, 0.95]. This was correct for the default
keep_threshold=0.85 (±0.10) but misleading for any other threshold. Fixed to use
`[keep_threshold - 0.10, keep_threshold + 0.10]` — always derive dynamic thresholds from
the configurable base, not from magic numbers that happen to match one default.

## README sections for new commands should come before Rust API

As the CLI grows (audit, calibrate, reliability, compare, summary, explain…), the order of
sections in README matters. Keep CLI command sections grouped together and place the Rust API,
Streaming API, and comparison table at the bottom. Users scanning for a CLI command should not
have to scroll past library documentation to find it.

## compare --policy-after lcb is an approximation, not a re-score

The `--policy-after lcb` flag uses `label_agreement_lcb` directly as the policy score proxy.
This differs from what `score --decision-score lcb` produces: in real LCB mode, other
components (score_consistency, budget_robustness, etc.) still contribute to the stability_score,
so the full LCB stability_score is higher than `label_agreement_lcb` alone. As a consequence,
`--policy-after lcb` can classify samples as "drop" when actual LCB mode would classify them
as "review". Use it for directional signal ("how many keeps would be demoted"), not for precise
decision prediction.

## select pass-through design enables pipeline composition

`select` outputs the original JSONL lines unchanged (not re-serialised), so downstream commands
receive exactly the same bytes as the original scored file. This means:
- `quietset select scored.jsonl --class borderline | quietset explain - --sample-id x` works
- CSV round-trips, numeric precision, and extra unknown fields are preserved
- The command is "filter by class" not "transform", keeping the primitive composable

## audit --observations requires separate observation file for agreement stats

`audit` takes scored JSONL as primary input (already has decisions, stability scores, etc.).
Fleiss kappa and Krippendorff alpha need per-evaluator raw labels, which live in the original
observation JSONL, not in scored JSONL. So `--observations` is a separate flag rather than
merging the two inputs. If you want agreement stats in CI, keep the original observation file
alongside the scored output.

## README structure: user journey before reference docs

Restructuring order matters. The README should answer "what can I do with this?" before
"how does the algorithm work?". Order: badges → tagline → use cases → install → CLI examples
→ command reference → detailed sections → Rust API. The original MVP structure put Installation
and CLI examples first, which is fine for a library primitive but undersells the full CLI surface.
When a project grows beyond 5 commands, a command reference table near the top saves readers
significant scrolling. Profile presets buried in a sub-section of the stability score explanation
are invisible; they need their own `## Profiles` heading.

## Embedding dataset-level metadata in per-sample JSONL streams

Dataset-level stats (kappa, alpha) don't belong on every StabilityReport row, but they
need to be discoverable by downstream commands that read scored JSONL. The solution: append
a single sentinel line `{"_quietset_stats":true,...}` at the end of the scored output (opt-in
via `--embed-stats`). Downstream readers do a cheap `contains("_quietset_stats")` check and
skip or parse accordingly. This is backwards-compatible: existing tools that parse the stream
as StabilityReport will fail on this line but can use `--skip-invalid` to ignore it. Don't
make this the default — not all users want the extra line, and it would break strict parsers.

## GitHub releases need explicit tags on version-bump commits

`cargo publish` doesn't create git tags or GitHub releases. After publishing to crates.io, also
run `git tag vX.Y.Z <sha>` and `gh release create`. With multiple version bumps in history,
find the right SHA with `git log --oneline | grep "bump version"`. Creating tags retroactively
is fine — GitHub releases can be created on any existing tag.

## filter --min-* and --max-* have asymmetric absent-field behaviour

For `--min-label-lcb` and `--min-confidence`: a record missing the field (None) is *excluded*
(treated as failing the minimum). For `--max-score-mad` and `--max-score-iqr`: a record missing
the field is *included* (treated as not exceeding the maximum). This asymmetry matches the
intuition that "no LCB" is insufficient evidence (exclude), but "no MAD" means no dispersion
info available (pass through). Document this clearly and keep consistent if adding new filters.

## GoldWeighted needed a private compute_report_inner, not a config knob

`DecisionScore::GoldWeighted` needs a per-sample confidence computed from cross-sample data
(evaluator reliability), which `compute_report` can't derive internally without breaking its
public signature. Fix: split it into a thin public wrapper and a private `compute_report_inner(...,
gold_weighted_confidence: Option<f64>)` that both `compute_report` and the new
`score_all_gold_weighted` orchestrator call. Also: `score_all_gold_weighted`'s truth map uses
`gold_label` only — unlike `score_all_weighted`'s gold-or-majority blend, blending here would make
"no gold anywhere" produce a self-consistency signal instead of the identical-to-`raw` no-op the
type's doc comment promises. A related trap: `calibrate`/`policy` grade precision against
`r.majority_label`, which is exactly the label `GoldWeighted` is designed to override — both had
to switch to `weighted_majority_label.or(majority_label)` or gold-weighted precision numbers would
silently be computed against the stale raw vote. This trap had a third sibling: `stable-wrong-risk`
graded against `r.majority_label` alone and had no `--decision-score` flag at all — missed in the
prior pass because it wasn't grepped alongside `calibrate`/`policy`. Grep all callers of
`majority_label` when fixing this class of bug, not just the ones already flagged.

`stable-wrong-risk --decision-score gold-weighted` is measured in-sample: the per-evaluator
weights are fit against the same gold labels the resulting vote is then graded against, so the
reported stable-wrong rate is optimistic (fitting to gold makes "kept but ≠ gold" shrink by
construction, not just by genuinely better evaluator-reliability signal). This isn't unique to
`stable-wrong-risk` — `calibrate`/`policy` have the identical circularity and it's an accepted
tradeoff there because they're threshold-*finding* tools searching against a gold target. But
`stable-wrong-risk`'s whole purpose is to be an honest safety check, so gold-weighting it can make
"0.0 risk" mean "fit its own training data perfectly" rather than "genuinely safe." Don't silently
ship this without a caveat in the flag's help text — a held-out-gold-split fix is real future work,
not something to add speculatively now.

## Per-label reliability weighting needed no backoff — smoothing already does the job

Extending `GoldWeighted` from a scalar per-evaluator reliability to per-(evaluator, predicted
label) reliability (an evaluator can be reliable on one label and unreliable on another) initially
seemed to need a "back off to the evaluator's overall scalar weight when a specific label has too
few observations" safety net. It doesn't: `compute_evaluator_weights`'s Laplace-smoothed formula
`(matches + 0.5) / (total + 1)` already pulls sparse estimates toward neutral 0.5 on its own — a
scalar-mean backoff would just re-blend across labels, which is exactly what per-label weighting
exists to avoid. Don't add a second shrinkage layer speculatively; add one later only if empirical
sparse-pair noise actually shows up.

## calibrate's Wilson CI on achieved_precision is the same in-sample circularity, one layer up

`precision_ci_low`/`precision_ci_high` (Wilson interval around `achieved_precision`) correctly
captures binomial sampling noise *at the winning threshold* — but `compute_calibration` picks that
threshold by searching 50 candidates and keeping the loosest one whose precision clears the target
on this same gold-labeled data. The threshold wasn't fixed in advance; it was selected against the
same labels the CI is then computed from. So the interval is narrower than the true out-of-sample
uncertainty — it hides selection noise, not just the sampling noise it's built to show. Same root
cause as the `stable-wrong-risk`/`calibrate`/`policy` circularity above, one abstraction layer up
(there it's the reliability weights fit-and-graded on the same gold; here it's the threshold
picked-and-graded on the same gold). Fix applied: one caveat sentence in the README next to the CI
docs, not a held-out split — that's the same deferred future work noted above, still not worth
building speculatively.

## "Large-scale changes are OK" authorizes scale, not direction

A large external review proposed upgrading quietset into a full "risk-controlled
noisy-evaluation engine" (14 items: `DecisionPolicy` trait, policy-file TOML, schema
versioning, nested output schema, an adapters crate, an EV-based active-review optimizer,
conformal risk control, Dawid-Skene latent-truth, and 5 "domain-aware" score signals). The user
said big changes were fine this time — but that permission is about scope, not about changing
what kind of tool this is. `AGENTS.md`'s non-goals (no full ML framework, no config-DSL
platform, no domain-specific game engine tool, no Python-first project) were the actual filter:
they cut the list to 2 items that deepen the existing thesis (noisy repeated evaluation →
explainable keep/review/drop) without adding a new *kind* of surface. Concretely rejected:
`DecisionPolicy` trait (the plain enum already extends fine — proved twice now, by
`GoldWeighted` and then `LatentTruth`), policy-file TOML (a config language for what CLI flags
already do), schema versioning/nested output (a compat shim for a v2 that doesn't exist),
adapters crate (squarely the game-engine-tool non-goal). Deferred to their own future cycle:
EV-based active-review optimizer, full conformal risk control (existing `calibrate`/`policy` +
Wilson CI already cover most of the value). Also re-confirmed the by-now-familiar failure mode:
the review claimed README_ja.md was "実質空" (effectively empty) — it was 791 lines, already at
full parity with README.md from an earlier phase. Don't trust an external review's claims about
current repo state without diffing against HEAD first, even when its *proposals* are worth
taking seriously.

## Score-signal metrics: redundancy-check each one before adding any

Of 5 proposed "domain-aware score signal" metrics, only `score_sign_agreement` survived a
redundancy pass: `score_flip_rate` either restates `1 - score_sign_agreement` or needs an
input-order concept `Observation` doesn't have; `score_bucket_agreement` needs a "near-zero"
threshold with no universal default across domains/scales (the same shape of speculative config
surface as the earlier-rejected bootstrap dependency and backoff mechanism); `budget_convergence_score`
is fully redundant with the already-shipped `budget_sensitivity` + `budget_slope` pair;
`rank_stability` needs cross-sample visibility, which is architecturally incompatible with
`compute_report`'s per-sample-independent computation shape (and squarely the ranking-tool
non-goal). Lesson: when an external source proposes N similarly-named metrics, check each one
against existing fields individually — "5 related metrics" from one proposal usually decomposes
into 1 genuinely new signal plus N-1 restatements or scope creep, not 5 independent asks.

## Dawid-Skene EM: extract the M+E step as a pure function for exact-value testing

Hand-deriving exact EM posteriors after many iterations to convergence is impractical (the
recursion has no closed form). Instead, `latent_truth.rs` exposes a private `em_step(k, j,
samples, pi) -> Vec<Vec<f64>>` performing exactly one M-step + E-step, callable in isolation
from a unit test with a hand-picked starting `pi` — this makes a *single* iteration's arithmetic
exactly checkable (a 2-evaluator/2-label symmetric case gives exact `0.9`/`0.1` posteriors,
verified by hand and asserted to `1e-9`), while `compute_latent_truth`'s full convergence loop is
verified behaviorally instead (via the integration tests: EM actually flips a
biased-evaluator-corrupted decision, fallback conditions trigger correctly, gold_label is never
consulted). Don't try to make one test do both jobs — exact-value and behavioral verification
want different levels of the implementation exposed.

## The `_` / `Some(...)` wildcard trap strikes a third time — grep, don't rely on the compiler

Adding `DecisionScore::LatentTruth` as a 5th enum variant reproduced the exact "GoldWeighted
needed compute_report_inner split" class of bug from two phases ago: several `main.rs` and
`calibration.rs` match sites use a `_` or `Some(X) | None` wildcard arm (the `label_for_score`
match, the `run_score`/`run_stable_wrong_risk`/`run_calibrate`/`run_policy` decision-score
resolution, the `matches!(..., DecisionScore::GoldWeighted)` if/else dispatches, and the
`weighted_majority_label.or(majority_label)` fallback chains) — none of these fail to compile
when a new variant is silently absorbed by the wildcard. `grep -n "GoldWeighted"` across both
crates before considering the CLI wiring done, and confirm every hit has a matching new-variant
line, was what actually caught all of them — the compiler only caught the 5 sites that use
genuine exhaustive matching.
