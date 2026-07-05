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

**Update**: the held-out split is now implemented (`compute_calibration`'s `heldout` param,
CLI `--heldout <path>`) — named as deferred future work three times in this file, then
explicitly requested by the user, which is exactly the point a "deferred" label should expire
and become a candidate to build. The circularity described above still fully applies whenever
`--heldout` is omitted, which remains the default — this fix doesn't retroactively make the
in-sample path honest, it just gives users a way to opt into the honest measurement.

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

## A repeat external review that's ~80% re-litigation is a signal, not just noise to filter

A second large review re-proposed nearly the same 14-item roadmap as the first (same rejected
`DecisionPolicy` trait, policy-file TOML, adapters crate, redundant score signals), and repeated
the exact same false "README_ja.md is empty" claim for a *third* time (it's 793 lines). Rather
than re-deriving the AGENTS.md-non-goals argument from scratch again, citing the prior rejection
reasoning directly (this file, the CHANGELOG) was both faster and more consistent — the
reasoning didn't get weaker for being repeated back. The one thing that *did* change: a
deferred item (held-out calibration split, named 3x here) became a build candidate the moment
the user asked for it explicitly by name — "deferred" is a label with an expiration condition
(explicit request), not a permanent rejection, and it's worth checking on every re-proposal
whether that condition has now been met for any specific item, rather than reflexively holding
the line on everything just because most of the list is repeat noise.

## Diagnostic helper functions taking already-converged state are directly unit-testable

`evaluator_effective_n_diagnostics(j, samples, pi)` takes the EM's converged posterior as a
plain parameter rather than computing it internally — this meant the "hand-derive an exact
DEFF/effective_n value" test could construct `samples`/`pi` directly and call the function in
isolation, without needing to actually run EM to convergence (which has no closed form) or
solve a combinatorial joint-contingency-table system to hit a specific target correlation
through realistic data. Same principle as `em_step` in the lesson above, one level up: keep
diagnostic/post-processing passes as pure functions over already-computed state, not fused into
the loop that produces that state, and exact-value tests become tractable instead of requiring
either a closed-form derivation or elaborate synthetic-data engineering.

## One weight defaulting to 0 instead of 1 needs a deliberate, documented asymmetry

`ScoreWeights::score_sign` had to default to `0.0` while every other field in that struct
defaults to `1.0`, because `score_sign_agreement` shipped informational-only last cycle —
defaulting it to `1.0` like its siblings would have silently changed `stability_score` for
every existing user with numeric scores, the exact kind of default-changes-existing-behavior
break this project avoids elsewhere. The asymmetry needed its own doc-comment sentence
explaining *why* it's the one exception, since without that, a future reader (or Claude) fixing
apparent "inconsistency" would be tempted to make it match the other five and reintroduce the
break. `config.validate()` also needed the new field added to its explicit finite/non-negative
and all-zero checks — it enumerates weight fields by name rather than iterating the struct, so
adding a field to `ScoreWeights` doesn't automatically extend validation; grep for the struct's
existing field names in `config.rs` when adding a new one, the same class of trap as the
`DecisionScore` wildcard-match issue above but for struct fields instead of enum variants.

## A subagent's premise should be verified, not trusted, even mid-design

When asked to design EV-based active-review, the design subagent was told (by me) that
`active-review`'s output shape was pinned by a CLI test — a plausible-sounding claim I hadn't
actually checked. The subagent grepped `crates/quietset-cli/tests/cli.rs` before proceeding and
found zero references to `active_review`/`urgency`; the only two matching tests live in
`integration_test.rs` and never touch the CLI or JSON field names at all. This flipped the
backward-compat decision (a plain in-place rename became viable, since nothing pins the old
strings) and is exactly the kind of claim-about-repo-state that needs verifying against HEAD
rather than accepted from the person asking — the same discipline this file already established
for external reviews' README_ja.md-emptiness claims, just applied to a claim I made myself.

## Ranking scores that multiply an honest-but-uncalibrated ratio into "expected value" need the multiplication kept at the ranking stage, not baked into the benefit fields

`active-review`'s `expected_coverage_gain`/`expected_risk_reduction` are literal, unconditional
quantities (`1/n_total`, `1/n_keep`) — `label_agreement_flip_ratio` (the distance-to-threshold
proxy, explicitly not a probability since only `label_agreement` has a modeled uncertainty in
this codebase) is multiplied in only at the final `utility` score, not folded into the benefit
fields themselves. Baking an uncalibrated ratio into a field named "expected_X" would have
laundered it into false precision; keeping it as a separate ranking multiplier preserves the
literal, checkable meaning of the benefit fields while still using the ratio for what it's
actually good for — ordering, not point estimation.

## Hand-derived test fixtures for multi-component `stability_score` need to check which components actually apply, not just the ones you're thinking about

Building `active_review.rs`'s exact-value tests, a fixture with `evaluator_id` set on every
observation pulled in an `evaluator_agreement` component that wasn't part of the hand
calculation (label + score_consistency only), silently shifting `stability_score` from the
predicted 0.875 to the actual 0.833 and flipping `decision` from the expected `Keep` to
`Review`. Any field present on an `Observation` can activate a `StabilityComponents` dimension
whether or not it's the one under test — when hand-deriving an exact expected value, either
omit every field not load-bearing for the components you intend to activate, or print the
actual `StabilityReport` once to confirm which components fired before asserting.

## Hand-built example JSON in docs needs to satisfy the formula it's documenting, not just look plausible

The `active-review` README example was hand-typed with `utility: 0.033` next to `flip_ratio:
0.42`/`coverage_gain: 0.0`/`risk_reduction: 0.033`/`cost: 1.0` — but the documented formula
(`flip_ratio * (coverage_gain + risk_reduction) / cost`) computes to `0.0139`, not `0.033`; the
`flip_ratio` multiply had silently been dropped from the mental arithmetic while assembling the
number. A reviewer (or a careful reader) checking the example by hand would catch this
immediately, which defeats the entire point of a cycle about honest, checkable numbers. Advisor
review caught it; a self-review wouldn't necessarily, since eyeballing "does this look like a
plausible small number" doesn't verify the actual multiplication. Fix used: paste real output
from an actual CLI run (already captured for a smoke test) instead of hand-assembling a
plausible-looking JSON object — the same standard already applied to hand-derived unit test
values, extended to docs examples.

## A test asserting a hardcoded literal set "maps 1:1" tests the literal, not the mapping

`test_all_five_signals_map_to_distinct_suggested_actions` originally built a `HashSet` from the
five expected action strings and asserted its length was 5 — this passes unconditionally
regardless of what `rank_active_review`'s actual `signals` array produces, since the literal
never touches the function under test. It would stay green even if the real mapping
regressed (e.g. two signals accidentally sharing an action string, or a signal's action
silently reverting to its old name). Fixed by building one single-signal-dominant fixture per
signal (weight 1.0 on it, 0.0 on the rest) and asserting the actual `primary_reason`/
`suggested_action` pair the function returns. General pattern: a test with no call into the
code under test is not a regression guard, no matter how reasonable its assertion looks in
isolation.

## "Hand-built example JSON must satisfy its own formula" bit twice — the lesson needed a mechanical fix, not just a restated warning

The Phase 17 lesson above (active-review's `utility: 0.033` vs. the correct `0.0139`) restated
as prose apparently didn't stick: Phase 18's `calibrate --heldout` README example shipped with
`train_precision: 0.97`, `achieved_precision: 0.71`, `note: null` — but the documented rule
(`gap = train_precision - achieved_precision > 0.05` ⇒ `note` is the overfit-warning string)
means `gap = 0.26 > 0.05`, so `note` could never legitimately be `null` there. Caught by an
advisor pass, not self-review, same as last time. Two bugs of the identical shape in one project
means "remember to double-check" isn't a sufficient fix. Going forward: any hand-typed doc
example with a value derived from a documented if/else or formula gets that branch actually
evaluated by hand (or, better, the example is pasted from real CLI output) before it's treated
as done — do not rely on "looks plausible" as the checkpoint.

## Reusing a function's return type across a new module is a compile-time check, not a design risk

`stable_wrong_breakdown()` was designed to consume `group_by_sample_id`'s output directly, but
its first-draft signature declared `&HashMap<String, Vec<Observation>>` — the actual return type
is `IndexMap<String, Vec<Observation>>` (`crate::group::group_by_sample_id`). The compiler caught
every call site immediately (the function signature plus 5 test fixtures using
`HashMap::new()`); fixed by changing the signature to `&IndexMap<...>` and updating the fixtures
via `replace_all`. Lesson: when a design doc says "reuse function X's output," grep X's actual
return type before writing the signature — Rust's type checker makes the mismatch cheap to catch
but only after code is written; checking first avoids the churn.

## "Probe-then-finalize" is the reliable way to build EM/statistically-derived test fixtures aimed at a target decision

Building a fixture meant to sit at baseline `Keep` before testing a demotion flag, the first
attempt (a single target sample plus 3 brand-new evaluators with no other supporting data) came
back `Review` — EM confidence was only 0.764 against the default 0.85 keep threshold, since 3
evaluators appearing once each carry too little fitted-reliability evidence. Rather than
guessing at parameters, a throwaway `eprintln!`-based probe test was run first to observe the
actual EM output, then the fixture was adjusted (5 additional supporting samples reusing the
same 3 evaluators, raising confidence to 0.9795) and only then were real assertions locked in.
This is the second time this exact probe-then-finalize sequence was needed for an EM-based
fixture in this project (the first was Phase 17's latent-truth convergence iteration counts) —
worth treating as the default approach for any new fixture whose target is a statistically-fit
outcome rather than a hand-computed one.

## A fifth re-proposal of the same rejected roadmap is still a signal, not a reason to relax the filter

An 11-item proposal (largely in Japanese, structured like the four prior "large external
review" cycles) re-proposed policy-file TOML (rejected 2x) and `schema_version` (rejected once)
again, plus new performance work (string interning, streaming CLI mode, spill-to-disk,
`--memory-limit`/`--partition-by`) framed as urgent given "large-scale JSONL." The performance
premise didn't survive a direct check: `calibrate`/`policy` already score once and sweep a
precomputed field (no rescoring-per-threshold exists), `StreamingScorer` already exists and is
already benchmarked, and the largest data anything in the repo is ever exercised against —
including `benches/scoring.rs` — is 5,000 synthetic observations, not "millions of rows." Citing
this evidence plus the prior-rejection reasoning directly to the user via `AskUserQuestion`
(rather than either silently rejecting or silently building) got a clean decision to hold all
three lines again. The one item that survived scrutiny — active-review per-action target
fields — did so because it was concretely specified, small, and hit a real architectural gap
(`StabilityReport` has no raw budget/seed/evaluator values to size a target from) with an
existing, proven solution pattern to copy (`audit --observations`). Lesson restated for a fifth
time because it keeps mattering: the discriminator between "build it" and "decline it" is
concrete evidence and an existing precedent to extend, not the size or confidence of the
proposal asking for it.

## A previously-deliberate "don't touch this default" decision is not permanent once the user asks for it by name

Phase 16 deliberately left `--profile game-ai`'s weights untouched when `score_sign_agreement`
shipped, reasoning that changing an existing profile's default `stability_score` would silently
break existing users. Phase 18's external review re-proposed the same change; rather than
re-rejecting it on the old reasoning, it was scoped via AskUserQuestion and the user explicitly
chose "enable it" for the game-ai profile specifically. Same pattern already logged for held-out
calibration (deferred in one cycle, built when asked by name in a later one) and EV-active-review
— a deliberate non-change made for backward-compat reasons is a default, not a permanent ban;
what makes it permanent is the absence of a direct ask, not the original reasoning.

## `cargo publish` packages the working tree, not a git ref — checkout the tag before publishing if anything landed after the release commit

Releasing 0.12.0, the version bump (`cd26345`) and a follow-up feature commit
(`f8fca26`, active-review target fields) were deliberately kept as two separate commits so the
tag could point at the bump alone. That's necessary but not sufficient: `git tag v0.12.0
cd26345 && git push` doesn't change what `main`'s checked-out working tree contains, and `cargo
publish` has no concept of tags — it packages whatever is on disk. Publishing with `main` still
at `f8fca26` would have shipped the follow-up commit's code to crates.io under the `0.12.0`
label, even though the tag and CHANGELOG section describe `cd26345`. Fix: `git checkout
v0.12.0`, publish from there, then `git checkout main` back. Caught by the user's review before
it happened, not by a self-check — worth treating "what does `cargo publish` actually read" as
a standing question whenever a release commit isn't also `HEAD` at publish time.

## A well-researched proposal's final numbered instruction list is the ask — exploratory prose above it may already be self-hedged or dropped

A metrics-provenance request included several paragraphs of "Sprint A–E" exploratory reasoning
(changing the default `--decision-score` to `lcb`, a new `--truth-model` flag) followed by a
final numbered list that didn't include either. The user had explicitly hedged the default-change
idea in the prose itself before dropping it from the final ask. Scoping the plan around the
prose instead of the final list would have manufactured two confirmation questions (default
change, new flag) the user had already resolved — advisor caught this before it turned into
wasted `AskUserQuestion` rounds. Lesson: when a proposal has both discursive reasoning and a
terminal "here's what to actually do" list, the list is the contract; treat prose-only ideas as
context, not scope, unless they also appear in the list.

## Verify cross-file markdown anchors against the live-rendered page, not the slugger algorithm from memory

Linking `docs/metrics_ja.md` and `README_ja.md` sections required anchors for headings like
"安定性スコア（stability_score）" and "reliability コマンド（experimental）" — full-width
parentheses, mixed Japanese/ASCII, underscores. Recalling GitHub's slug algorithm from memory
(which punctuation ranges get stripped, whether underscores survive) was unreliable enough to
be worth not trusting; fetching the already-live `README_ja.md` page and asking for the actual
`id` attributes on those headings gave exact answers in one call each (e.g. underscores are
kept, full-width parens are stripped, hyphens only appear where a space existed in the source
text). For a new heading not yet pushed, the fallback was picking punctuation-free heading text
instead of guessing the slug — cheaper than being wrong in a shipped doc link.
