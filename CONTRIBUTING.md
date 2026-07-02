# Contributing to quietset

quietset is a small, model-agnostic stability-filtering primitive. See [AGENTS.md](AGENTS.md) for
the full project scope, non-goals, and design principles — read it before proposing a feature.

## Before submitting a change

1. Keep changes small and focused; avoid unrelated refactors in the same PR.
2. Add tests for behavior changes (library tests in `crates/quietset/tests/`, CLI process-level
   tests in `crates/quietset-cli/tests/`).
3. Update the README (and `README_ja.md`, if the change affects CLI-facing behavior) and
   `CHANGELOG.md` for user-facing changes.
4. Avoid adding heavy dependencies. `unsafe` code is not accepted without a benchmark-backed
   justification.

## Output-format conventions for new commands

quietset's 13 subcommands deliberately don't share one output format (see README's
["Output formats"](README.md#output-formats) section) — some are built for human inspection,
some for pipeline composition — but new commands should still fit one of the existing groups
rather than introduce a 7th convention:

- If the command emits a single result object, add `--json` for a single pretty-printed JSON
  object, matching `summary`/`explain`/`compare`/`audit`.
- If the command emits a stream of per-record results, default to JSONL (one object per line),
  matching `filter`/`select`/`reliability`/`active-review`/`recommend`.
- Either way, say so explicitly in the flag's `--help` text — don't assume the format is
  self-evident from the flag name (`policy --json` is JSONL, not a single object, and that
  surprised even the maintainer; the help text now says so).

## Development commands

Run these before opening a PR — they match what CI checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

## Commit style

Prefer conventional commits, e.g. `feat: add JSONL scoring`, `fix: handle missing optional score
field`, `docs: add CLI examples`, `test: add budget sensitivity fixture`.

## License

By contributing, you agree that your contributions will be licensed under the same dual
MIT OR Apache-2.0 license as the project.
