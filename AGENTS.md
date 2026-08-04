# AGENTS.md — BitVanes monorepo

One git history, four components. `core/` is a Cargo workspace; `cli/`,
`nerd/`, and `web/` are standalone. Build each from its subdirectory (no root
`Cargo.toml` — Cargo does not support nested workspaces).

## Quick verification (run all four before considering the tree green)

```bash
# core engine (ner-client included so the NER IPC client + socket tests run)
( cd core && cargo fmt --check && \
  cargo clippy --workspace --all-targets \
    --features cli-pdf,parallel,ipc,csv,office,mmap,stream,pdf-redact,config,ner-client -- -D warnings && \
  cargo test --workspace \
    --features cli-pdf,parallel,ipc,csv,office,mmap,stream,pdf-redact,config,ner-client )

# cli (path-dep on ../core — present in this layout)
( cd cli && cargo fmt --check && \
  cargo clippy --all-targets -- -D warnings && \
  cargo test )

# nerd — the Tier-2 NER sidecar (engine stays ML-free; model + ONNX Runtime
# live here). The scaffold fails closed until the `model` feature lands.
( cd nerd && cargo fmt --check && \
  cargo clippy --all-targets -- -D warnings && \
  cargo build )

# web
( cd web && npm ci && npm run build )
```

> Do **not** use `--all-features` on core: `pii-model` enables an
> `unimplemented!()` stub. Always pass the explicit feature list above.

## Component guides

- `core/AGENTS.md`  — engine feature flags, architecture, wasm build.
- `cli/AGENTS.md`   — subcommand smoke tests, daemon endpoints.
- `nerd/README.md`   — Tier-2 NER sidecar: wire contract, model/onnxruntime.
- `web/README.md`   — dashboard ↔ daemon contract.

## Release binary (dashboard embedded)

```bash
( cd web && npm run build )
( cd cli && cargo build --release --features dashboard )
./cli/target/release/bitvanes --version   # → bitvanes 1.0.0
```

`libpdfium` on the host is required for destructive PDF redaction
(`--pdf-mode redact|flatten`); it is a runtime, not build-time, dependency.

## Toolchain

- Rust stable (edition 2024), MSRV 1.85 (rust-toolchain.toml in core/ and cli/).
- Node 20+ for web.
