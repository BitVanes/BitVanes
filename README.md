# BitVanes

Zero-trust data purification & stream-filtering engine. BitVanes detects,
redacts, and sanitizes PII across document and byte streams (PDF, JSON, CSV,
text, office docs) **locally** — before they reach downstream APIs, databases,
or AI models. 100% on-premise; the engine makes no network calls.

This is the canonical monorepo. The three components share one git history:

```
BitVanes/
├── core/      Rust workspace: bitvanes-core (library) + bitvanes-wasm (legacy binding)
├── cli/       bitvanes-cli → the `bitvanes` binary (scrub / filter / daemon / tui)
└── web/       @bitvanes/web — Vite + React landing page + local dashboard
```

`cli` links `core` via a path dependency (`../core/crates/core`). `core` is its
own Cargo workspace; `cli` and `web` are standalone. Build each from its
subdirectory (Cargo does not support nested workspaces, so there is no root
`Cargo.toml`).

## Quick start

```bash
# core (library engine)
cd core
cargo test --workspace --features cli-pdf,parallel,ipc,csv,office,mmap,stream,pdf-redact,config

# cli (the bitvanes binary) — requires ../core present (it is, in this layout)
cd ../cli
cargo build --release
./target/release/bitvanes --help

# web (landing page + local dashboard)
cd ../web
npm install && npm run build
```

## Deployable artifact (single self-contained binary)

```bash
(cd web && npm run build)                  # produce web/dist
(cd cli && cargo build --release --features dashboard)   # embeds ../web/dist
./cli/target/release/bitvanes daemon --port 8080         # serves the dashboard
```

## Zero-trust invariants

1. **Destructive redaction** — PII bytes are removed (text) or deleted from the
   page-object tree (PDF), never painted over. Fail-closed.
2. **Bounded memory** — stream path uses a rolling window ≥ longest pattern;
   document path uses mmap.
3. **Loopback-only daemon** — binds `127.0.0.1` only; dashboard same-origin.
4. **No network calls** — tokenization, detection, and redaction are fully
   offline. (The legacy wasm crate is retained but unused by the dashboard.)
5. **`unsafe_code = deny`** — pdfium FFI lives inside the `pdfium-render` dep.

See [`REBRAND.md`](./REBRAND.md) for the full architecture, build/test/run
procedures, the completed-phase list, and the design audit.

## License

Proprietary (see [`LICENSE.md`](./LICENSE.md)). The engine is free to use for
internal data purification; the source code is not open-source. Contact the
author for licensing inquiries.
