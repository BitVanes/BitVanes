# Contributing

Thanks for considering a contribution to BitVanes. This guide covers the
expectations and the fastest path to a merged PR.

## Quick verify

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --features cli-pdf,parallel,ipc,csv -- -D warnings
cargo test --workspace --features cli-pdf,parallel,ipc,csv
```

We avoid linking the `embeddings` feature in CI (its prebuilt `ort` static
lib needs glibc ≥ 2.38); a compile-only `cargo check --all-features` in CI
still catches regressions there. Locally, `cargo test --all-features` works on
a recent Linux.

Wasm: `wasm-pack build crates/wasm --target web --out-dir pkg` (target ≤ 5 MB
gzipped).

## Engineering expectations

- **Tests**: every new code path needs coverage. We hold the line at the
  current ~95% module coverage.
- **No new panics on data paths**: propagate errors via `BitVanesError`. The
  only `expect`/`panic` calls should be on provably-impossible structural
  invariants, each explained in a comment.
- **Zero-telemetry is inviolable**: do not introduce network calls in the
  engine. If a feature needs a model or asset, fetch it from the *consuming*
  binary (CLI/web), never from `bitvanes-core`.
- **Output schema is frozen**: adding/removing/reordering a column in
  `output_schema` is a **semver-major** change. Add columns only at the end
  and mark them nullable.
- **Comments**: document *why*, not *what*. Public items need rustdoc.

## Commit / PR style

- Use the present tense ("Add overlap carry", not "Added").
- Reference issues and the changelog. Add an `Unreleased` entry to
  `CHANGELOG.md` for user-visible changes.
- Keep PRs focused; split unrelated changes into separate PRs.

## Workspace layout

`crates/core` is the pure library (no wasm-bindgen). `crates/wasm` is the
thin `wasm-bindgen` wrapper. Prefer extending the library over the wrapper —
the wrapper should stay a pass-through.
