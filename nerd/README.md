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
cargo build --features model      # real ONNX inference (once wired + libonnxruntime present)
BITVANES_NERD_SOCKET=/tmp/bitvanes-nerd.sock ./target/debug/bitvanes-nerd
```

The socket path defaults to `$XDG_RUNTIME_DIR/bitvanes-nerd.sock`
(or `/tmp/bitvanes-nerd.sock`), overridable via `BITVANES_NERD_SOCKET`.

## Status — scaffold

The server, framing, socket I/O, and entitlement gate are real and exercised
by the engine-side `ner_client` tests. The inference path is a **fail-closed
stub** (`code = "available"` → `"unavailable"`) until the `model` feature
wires:

- the bundled Int8 ONNX BERT-NER (`dslm/bert-base-NER`) via `ort` (dynamic
  `libonnxruntime` load — clean build, fail-closed if the native lib is
  absent, same pattern as pdfium),
- `tokenizers` for WordPiece,
- BIO-tag → byte-offset aggregation (the invariant-critical part),
- PER/ORG/LOC → `person_name`/`organization`/`location` slug mapping.

Until then, `nerd` running without `--features model` is a deliberate
no-op: the engine's `scrub_with_detectors` gets `Inference` and refuses to
emit text the detector would have scrubbed. **Fail-closed by construction.**

## Runtime dependencies

- `libonnxruntime` on the host when built with `--features model` (runtime,
  not build-time — mirrors `libpdfium`). Bundled into the release tarball.
- The model artifact (Int8 ONNX + tokenizer.json) — to be bundled; a Python
  export script (`scripts/export_ner_model.py`) produces it from the HuggingFace
  source. Until then the `model` feature errors at first inference.
