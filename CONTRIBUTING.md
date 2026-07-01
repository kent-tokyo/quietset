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
