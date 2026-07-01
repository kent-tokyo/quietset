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
