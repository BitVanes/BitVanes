# AGENTS.md

Build commands for the `bitvanes-core` workspace.

## Quick verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## Wasm build

```bash
wasm-pack build crates/wasm --target web --out-dir pkg
```

Check bundle size (target: reasonable for a SaaS dashboard, ~4-5 MB gzipped):

```bash
gzip -c crates/wasm/pkg/bitvanes_wasm_bg.wasm | wc -c
```

## Feature flags

- `ipc`: Arrow IPC stream writer. Native-only (CLI).
- `csv`: Arrow CSV writer. Native-only.
- `parallel`: Rayon parallel batch (`run_pipeline_batch`) + parallel regex sweep. Native-only.
- `cli-pdf`: Native PDF text extraction via pdf-extract. Native-only (browser uses PDF.js).
- `office`: DOCX/PPTX/XLSX/EPUB/RTF parsing. Native-only.
- `mmap`: Memory-mapped file I/O for large files. Native-only.
- `pii-model`: Tier-2 NER (stub only — trait + ModelDetector placeholder).

> Rebrand in progress: the `embeddings`/RAG surface has been removed. The
> wasm/browser crate is slated for removal (see `REBRAND.md`, Phase 7).

BPE vocab is **unconditional** (tiktoken-rs embeds it with no
feature toggle), so every build is offline.

Run tests with all features:

```bash
cargo test --workspace --features cli-pdf,parallel,ipc,csv,office,mmap
```

## Architecture

Pipeline: parse -> scrub (detect + redact PII) -> optional chunk -> Arrow assembly.

Entry point: `bitvanes_core::pipeline::run_pipeline(bytes, &cfg)`.

Wasm export: `bitvanes_wasm::process(config_js, bytes) -> slot_id` (pending
removal in Phase 7).

The wasm module exports Arrow data via zero-copy FFI pointers (Arrow C Data
Interface). JS reads them using `arrow-js-ffi`'s `parseRecordBatch`.

## Toolchain

- Rust stable (edition 2024), MSRV 1.85, pinned via `rust-toolchain.toml`.
- `wasm-pack` 0.13+ for wasm builds.
- `wasm32-unknown-unknown` target (auto-installed by rust-toolchain.toml).
