# AGENTS.md

Build commands for the `bitvanes-core` workspace.

## Quick verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets \
  --features cli-pdf,parallel,ipc,csv,office,mmap,stream,pdf-redact,config,ner-client -- -D warnings
cargo test --workspace \
  --features cli-pdf,parallel,ipc,csv,office,mmap,stream,pdf-redact,config,ner-client
```

> Do **not** use `--all-features`: `pii-model` enables an `unimplemented!()`
> stub. Always pass the explicit feature list above.

## Wasm build (legacy binding)

```bash
wasm-pack build crates/wasm --target web --out-dir pkg
gzip -c crates/wasm/pkg/bitvanes_wasm_bg.wasm | wc -c
```

The web dashboard no longer uses this — it talks to the native daemon. The
wasm crate is retained as a legacy FFI binding.

## Feature flags

- `ipc`: Arrow IPC stream writer. Native-only (CLI).
- `csv`: Arrow CSV writer. Native-only.
- `parallel`: Rayon parallel batch (`run_pipeline_batch`) + parallel regex sweep. Native-only.
- `cli-pdf`: Native PDF text extraction via pdf-extract. Native-only.
- `office`: DOCX/PPTX/XLSX/EPUB/RTF parsing. Native-only.
- `mmap`: Memory-mapped file I/O for large files. Native-only.
- `stream`: Async rolling-window `StreamSanitizer` over `tokio::io`. Native-only.
- `pdf-redact`: PDF sanitization — text-layer redaction + destructive
  coordinate-aware blackout/flatten via runtime `pdfium-render` (implies
  `cli-pdf`). Fails closed (`FeatureNotEnabled`) if `libpdfium` is absent.
- `config`: `Bitvanes.toml` user-facing config loader.
- `ner-client`: Tier-2 NER IPC client + wire contract for the `bitvanes-nerd`
  sidecar (local Unix-domain socket). Adds NO ML deps — the engine stays
  ML-free; inference runs in `nerd/`. The sidecar enforces the license token
  offline so free-tier use is refused.
- `pii-model`: Tier-2 NER trait + `ModelDetector` **stub** (`unimplemented!()`).

Token counting uses a pure-arithmetic **chars-per-token heuristic** (no BPE
dependency, no embedded vocab), so every build is fully offline.

## Architecture

Pipeline: parse -> scrub (detect + redact PII) -> optional chunk -> Arrow assembly.

Entry point: `bitvanes_core::pipeline::run_pipeline(bytes, &cfg)`.

PII/sanitizer surface:
- `pii::Scrubber` detects PII (regex + Luhn/ABA + anchor windows).
- `sanitizer::policy::{RedactionPolicy, sanitize_text}` applies mask/placeholder/hash.
- `sanitizer::stream::StreamSanitizer` does bounded-memory rolling-window redaction.
- `sanitizer::pdfium::redact_pdf` does destructive PDF redaction (Redact/Flatten).

The wasm module exports Arrow data via zero-copy FFI pointers (Arrow C Data
Interface). JS reads them using `arrow-js-ffi`'s `parseRecordBatch`.

## Toolchain

- Rust stable (edition 2024), MSRV 1.85, pinned via `rust-toolchain.toml`.
- `wasm-pack` 0.13+ for wasm builds.
- `wasm32-unknown-unknown` target (auto-installed by rust-toolchain.toml).
- PDF destructive redaction needs a runtime `libpdfium` on the host (not a
  build-time dependency).
