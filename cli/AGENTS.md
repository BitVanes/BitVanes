# AGENTS.md

Build commands for the `bitvanes-cli` crate.

## Quick verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## Smoke tests

```bash
# scrub a single file (placeholder policy)
printf 'mail alice@example.com or 123-45-6789\n' > /tmp/t.txt
./target/release/bitvanes scrub /tmp/t.txt --out - --rules email,ssn

# filter a stream (mask policy)
cat /tmp/t.txt | ./target/release/bitvanes filter --rules email,ssn --mask-char '#'

# daemon health + filter
./target/release/bitvanes daemon --port 8080 --rules email &
curl -s http://127.0.0.1:8080/health
printf 'reach bob@evil.io' | curl -s --data-binary @- http://127.0.0.1:8080/filter
kill %1
```

## Architecture

Subcommand-based CLI (`scrub` / `filter` / `daemon` / `tui`) in `src/main.rs`.

- `src/scrub.rs`   — batch sanitize (parse → detect → redact → write + stats).
- `src/filter.rs`  — async stdin→stdout via core's `StreamSanitizer` (tokio).
- `src/daemon.rs`  — axum HTTP daemon, **127.0.0.1 only**, `/health` + `/filter`.
- `src/shared.rs`  — `Bitvanes.toml` loading, rule parsing, format inference.
- `src/tui/`       — interactive ratatui UI (retained from the original product).

The PII detection/redaction runs inside `bitvanes-core`:
`pii::Scrubber` detects, `sanitizer` applies the output policy + `StreamSanitizer`
streams, `config::BitvanesConfig` parses `Bitvanes.toml`.

## Dependency on core

```toml
bitvanes-core = { path = "../core/crates/core", features = ["ipc","csv","cli-pdf","parallel","office","mmap","stream","config","pdf-redact"] }
```

The CLI links core via a path dependency so the three repos move in lockstep.
Release binaries are built from this monorepo layout (core/cli/web under one
parent directory).

## Toolchain

- Rust stable (edition 2024), MSRV 1.85.
- Native binary only (no wasm target).
