# bitvanes-nerd

The Tier-2 NER sidecar. Listens on a local socket and serves named-entity
detection to the `bitvanes-core` engine. **The engine stays ML-free** — the
model + ONNX Runtime live entirely in this crate; the engine's `ner-client`
feature is a pure IPC client (no ML deps).

## Why a separate process

- The thing we iterate on most often (the model) is decoupled from engine
  releases — swap the model without rebuilding the CLI.
- Process-level fault isolation: a model OOM/panic cannot take down the
  scrubbing daemon.
- Defense-in-depth entitlement: the license token is verified **here**, where
  the model actually runs, so a client that bypasses the CLI gate gets nothing.

## Wire contract (mirrors `core/crates/core/src/pii/ner_client.rs`)

Length-prefixed (4-byte big-endian) JSON over a Unix domain socket:

```text
Request  -> { "text": str, "token": str|null }
Response <- { "status": "ok",  "findings": [ {entity, start, end, confidence} ] }
          | { "status": "err", "code": "unentitled"|"unavailable"|"internal", "msg": str }
```

Offsets are **byte** offsets into the request `text`. `token` is the `bv1_…`
license key; free-tier (absent / wrong prefix) is refused with `unentitled`.

## Build & run

```bash
cd nerd
cargo build                       # scaffold (no model) — fails closed: "unavailable"
cargo build --features model      # real ONNX inference (needs libonnxruntime at runtime)

# Fetch the bundled model (one-time; pinned + SHA256-verified; no Python):
./scripts/fetch-model.sh          # → models/model_quantized.onnx + models/tokenizer.json

BITVANES_NERD_SOCKET=/tmp/bitvanes-nerd.sock ./target/debug/bitvanes-nerd
```

The socket path defaults to `$XDG_RUNTIME_DIR/bitvanes-nerd.sock`
(or `/tmp/bitvanes-nerd.sock`), overridable via `BITVANES_NERD_SOCKET`.

## Runtime dependencies (only with `--features model`)

All local, all pinned, no Python anywhere in the repo or at runtime:

- **The model + tokenizer** — fetched by `scripts/fetch-model.sh` (pure shell +
  curl + sha256) from `Xenova/bert-base-NER` at a pinned commit, SHA256-verified
  against `models/MANIFEST.toml`. Loaded by nerd from `models/` (or
  `$BITVANES_NER_MODEL` / `$BITVANES_NER_TOKENIZER`). Ship them in the release
  tarball next to the binary.
- **`libonnxruntime`** — the ONNX Runtime C engine; loaded dynamically via
  `$ORT_DYLIB_PATH`. Absent ⇒ fail-closed `unavailable` (same pattern as
  `libpdfium`). Ship it in the release tarball.

Nothing else. No network calls at runtime, no Python on the host, no telemetry.

## Status

The server, framing, socket I/O, entitlement gate, and the **full NER
post-processing pipeline** (`nerd/src/ner.rs` — BIO aggregation, subword→word
merging, byte-offset validation, entity-slug mapping) are real and unit-tested
(23 tests) without any model.

The **ONNX inference boundary** (`nerd/src/inference.rs`, behind `--features
model`) is wired and **compile-verified** against `ort` 2.0.0-rc.13 +
`tokenizers` 0.21: lazy-loads the model + tokenizer, tokenizes with offset +
word-id tracking, runs the model, argmaxes per-token logits, and feeds the
result through the pure `ner` pipeline. Runtime behavior — that the model's
output head matches `CONLL_LABELS` and the tokenizer's offsets are
byte-accurate — is gated on the model artifact + `libonnxruntime` being
present, and is exercised by a runtime test once those ship.

Until then, `nerd` running without the model artifact (or without
`--features model`) is a deliberate no-op: the engine's
`scrub_with_detectors` gets `Inference` and refuses to emit text the detector
would have scrubbed. **Fail-closed by construction.**

## Runtime dependencies

See "Runtime dependencies" under Build & run above — model fetched via
`scripts/fetch-model.sh`, `libonnxruntime` via `ORT_DYLIB_PATH`. Both ship in
the release tarball; nothing is fetched or run from the network at runtime.
