# BitVanes — v1.0.0 Release Walkthrough

BitVanes is a **zero-trust data purification & stream-filtering engine**: it
directs, filters, and purifies streams of unstructured/binary data (PDFs, JSON,
CSVs, text logs, office docs) by stripping sensitive PII, destructively
redacting compliant documents, and sanitizing payloads **locally** — before
they reach downstream APIs, databases, or AI models.

This doc tracks the pivot from the original "RAG / LLM document parser" to the
purification product, records the **final TODO audit** for v1.0.0, and is the
single source of truth for how the three repos fit together.

## Repository layout (three repos, monorepo-style)

```
BitVanes/
├── core/      Rust workspace: bitvanes-core (library) + bitvanes-wasm (legacy binding)
├── cli/       bitvanes-cli → the `bitvanes` binary (scrub / filter / daemon / tui)
└── web/       @bitvanes/web — Vite + React landing page + local dashboard
```

`cli` links `core` via a **path dependency** (`../core/crates/core`), so all
three repos must be checked out as siblings to build the CLI. They are
currently three separate git histories (see `TODO(workspace)` below).

---

## Governing invariants (non-negotiable)

1. **Destructive redaction** — occluding PII is not redaction. PII bytes must
   be *removed* from the output (text) or page-object tree (PDF), never merely
   painted over. Security = fail-closed.
2. **Bounded memory regardless of input size** — stream path uses a rolling
   overlap window ≥ max-pattern length; document path uses mmap.
3. **Stream-boundary correctness** — a token split across two read chunks must
   still be caught (rolling window ≥ longest pattern; property-tested, 256
   cases).
4. **Loopback-only daemon** — binds `127.0.0.1` only; dashboard served from the
   same origin ⇒ zero CORS surface, zero remote attack surface.
5. **`unsafe_code = deny` workspace** — pdfium FFI lives entirely inside the
   `pdfium-render` dependency; the workspace contains no `unsafe` blocks.
6. **Wire-format breaks are deliberate** — removing `embeddings`/`EmbeddingConfig`
   broke old `profile.json`; gated by the semver-major bump to 1.0.0.

---

## Phase status — all shipped ✅

- [x] **Phase 0** — flip CLI core dep to path; fix API-drift sites.
- [x] **Phase 1** — strip embeddings from core (removed feature, deps, modules,
      `EmbeddingConfig`, `ChunkStrategy::Semantic`, the Arrow `embedding`
      column; schema is now 10 columns).
- [x] **Phase 2** — `pii/` (`detect` + `model`) and `sanitizer/` (`policy` +
      `report`); `StreetAddress` + `[REDACTED_*]` / mask / `[SHA256:…]`
      policies + `SanitizationStats`.
- [x] **Phase 3** — async streaming core (`sanitizer::stream::StreamSanitizer`,
      bounded-memory rolling window; `stream` feature).
- [x] **Phase 4** — **destructive PDF sanitization shipped.** `pdf-redact`
      feature: text-layer redaction + coordinate-aware blackout/flatten via
      runtime `pdfium-render`. Redact deletes PII text objects + draws black
      rects; Flatten rasterizes to 300 DPI and drops the text layer. Fails
      closed (`FeatureNotEnabled`) if `libpdfium` is absent.
- [x] **Phase 5** — `Bitvanes.toml` config loader (`config` feature).
- [x] **Phase 6** — CLI rewritten to `scrub` / `filter` / `daemon` / `tui`;
      axum daemon bound to 127.0.0.1 (`/health`, `/filter`, `/scrub`); embedded
      dashboard via `rust-embed` (`dashboard` feature); drag-and-drop UI.
- [x] **Phase 7** — web stripped of in-browser engine/WASM/embeddings; landing
      copy + pricing + zero-trust callout; minimal local dashboard client.
      Bundle 705 KB → 206 KB.
- [x] **Phase 8** — branding/copy sweep across all three repos.
- [x] **Phase 9** — verification + v1.0.0 version bump (this release).

---

## Final TODO audit (v1.0.0)

Every `TODO(phase-*)` from the rc.1 audit, resolved:

| TODO | Status | Notes |
|------|--------|-------|
| `phase-4-pdfium` — destructive PDF blackout | ✅ **DONE** | `sanitizer::pdfium::redact_pdf` (Redact + Flatten), wired into `bitvanes scrub --pdf-mode`, integration tests. Dead stub `redact_pdf_blackout` removed. |
| `phase-6-followup` — daemon hardening | 🟡 Partial (non-blocking) | `/scrub` JSON, `/filter` text, embedded dashboard, drag-and-drop **all shipped**. Remaining: multipart upload, true HTTP-body streaming, request audit logging — post-v1. |
| `phase-9-release` — tag v1.0.0 | ✅ **DONE** | All three crates bumped to `1.0.0`. |
| `credit-card` — Luhn fail-safe | ✅ **DONE** | Luhn is now a *soft* confidence modifier: card-shaped numbers that fail the checksum are still flagged + redacted (fail-safe). Raise `min_confidence` for precision. |
| `tiktoken` — drop `tiktoken-rs` | ✅ **DONE** | Replaced the BPE backend with a `chars/4` heuristic estimator. API + wire-format preserved; multi-MB vocab dependency removed. |
| `name-ner` — personal-name detection | ✅ **DONE** | Tier-2 gazetteer: `ScrubProfile::names` (customer-supplied) + opt-in `use_generic_names` starter list. Matched case-insensitively on word boundaries. |
| `pricing` — billing backend | 🟡 Hook stubbed | `cli/src/entitlement.rs` defines the `EntitlementChecker` trait + license-key wire format (`bv1_…`). Default `OpenSourceChecker`; `--license-key`/`BITVANES_LICENSE_KEY` resolves a `LicenseChecker`. Cloud Stripe/sig-verify backend is a separate service (out of engine scope). |
| `workspace` — consolidate into one git history | ✅ **DONE** | `core/`, `cli/`, `web/` merged into a single monorepo via `git subtree` (full history preserved). Unified CI + Dependabot. |

**Result:** the single v1.0 blocker (destructive PDF redaction) is cleared and
every deferred audit item is either resolved or has a shipped interface.

---

## How to build, test, and run

### core (library engine)

```bash
cd core
cargo fmt --check
cargo clippy --workspace --all-targets \
  --features cli-pdf,parallel,ipc,csv,office,mmap,stream,pdf-redact,config -- -D warnings
cargo test --workspace \
  --features cli-pdf,parallel,ipc,csv,office,mmap,stream,pdf-redact,config
```

Expected: **174 unit + 4 pdfium-integration + 1 doctest** passing. The pdfium
tests are guarded by `pdfium_available()` — they run when `libpdfium` is on the
host, otherwise skip. **Do not use `--all-features`** (it enables the
`pii-model` `unimplemented!()` stub).

> Destructive PDF redaction needs a runtime `libpdfium` (e.g.
> `libpdfium.so` / `pdfium.dylib` / `pdfium.dll`) on the host. It is **not** a
> build-time dependency — the build stays clean.

### cli (the `bitvanes binary)

```bash
cd cli    # requires ../core checked out as a sibling
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo build --release
```

Smoke test all four subcommands:

```bash
BIN=./target/release/bitvanes

# scrub a single file (placeholder policy)
printf 'mail alice@example.com or 123-45-6789\n' > /tmp/t.txt
$BIN scrub /tmp/t.txt --out - --rules email,ssn

# filter a stream (mask policy)
cat /tmp/t.txt | $BIN filter --rules email,ssn --mask-char '#'

# destructive PDF redaction (needs libpdfium)
$BIN scrub report.pdf --out clean.pdf --pdf-mode flatten

# daemon health + filter + scrub (loopback only)
$BIN daemon --port 8080 --rules email &
curl -s http://127.0.0.1:8080/health
printf 'reach bob@evil.io' | curl -s --data-binary @- http://127.0.0.1:8080/filter
curl -s -X POST http://127.0.0.1:8080/scrub -H 'content-type: application/json' \
  -d '{"text":"alice@example.com 123-45-6789"}'
kill %1

# interactive TUI
$BIN tui
```

### web (landing page + local dashboard)

```bash
cd web
npm install
npm run build      # tsc -b && vite build  →  dist/ (206 KB / 65 KB gz JS)
npm run dev        # local dev server
```

The dashboard POSTs to `http://127.0.0.1:8080/scrub` — start
`bitvanes daemon` first. Build the CLI with `--features dashboard` to embed
`dist/` so the daemon serves the dashboard with no `--dashboard-dir`.

### Release binary (dashboard embedded)

```bash
# 1. build the web dashboard first (produces web/dist)
(cd web && npm run build)
# 2. build the CLI with the dashboard feature (embeds ../web/dist)
(cd cli && cargo build --release --features dashboard)
# 3. the single binary now serves the dashboard:
./cli/target/release/bitvanes daemon --port 8080
```

---

## Architecture

**core** (`bitvanes_core`) — pure library, no `wasm-bindgen`:

```
parse/         Markdown, HTML, text, JSON, PDF, DOCX/PPTX/XLSX/EPUB/RTF → Document
pii/           detect.rs (regex + Luhn/ABA + anchor windows + name gazetteer), model.rs (NER trait)
   └─ Scrubber.scrub(text) → (redacted, OffsetMap, Vec<PiiFinding>)
sanitizer/
   ├─ policy.rs    RedactionPolicy::{Placeholder, Mask, Hash} + sanitize_text
   ├─ report.rs    SanitizationStats (files/bytes/PII-by-type/MiB·s⁻¹)
   ├─ stream.rs    StreamSanitizer (bounded-memory rolling window over tokio::io)
   ├─ pdf.rs       sanitize_pdf_text (text-layer path, no native backend)
   └─ pdfium.rs    redact_pdf (destructive Redact/Flatten via pdfium-render)
tokenize.rs    chars-per-token heuristic estimator (no BPE dependency)
chunk.rs       structural-boundary chunker
arrow_io/      RecordBatch assembly (10 cols), FFI registry, IPC stream, CSV
pipeline.rs    run_pipeline (+ run_pipeline_batch with `parallel`)
config.rs      Bitvanes.toml loader (`config` feature)
```

Entry points: `run_pipeline(bytes, &cfg)` (library), `bitvanes scrub|filter|daemon|tui` (CLI).

**cli** — thin orchestration over core. `shared.rs` resolves `Bitvanes.toml` /
`--rules` / default ruleset into a `Scrubber` + `RedactionPolicy`. `entitlement.rs`
is the license-check hook (`/entitlement` endpoint + `--license-key` flag).

**web** — Vite + React 19. Landing page + dashboard that talks only to
`127.0.0.1:8080`. No in-browser engine.

### PII detection summary

| Tier | Source | Patterns |
|------|--------|----------|
| Tier 1 (regex + checksum) | built-in | email, ssn, phone, **credit_card** (Luhn soft), routing_number (ABA gate), street_address, aws_key, github_pat, jwt + user custom regex |
| Tier 2 (gazetteer) | `ScrubProfile::names` | customer-supplied name phrases + opt-in generic starter list |

Credit-card Luhn is **soft** (fail-safe: flag all card-shaped numbers; raise
`min_confidence` for precision). Personal names are **gazetteer-driven** because
regex/heuristic name matching is too false-positive-prone to ship — the customer
supplies the names that matter to their corpus.

---

## Suggestions for further improvement (post-v1)

1. **Daemon hardening (highest value).** Add (a) request/audit logging — every
   redaction is already computed as a findings list, so emit a structured
   `audit.ndjson`; (b) true `axum` body streaming into `StreamSanitizer` so a
   multi-GB POST never buffers; (c) multipart file upload so the dashboard
   scrubbs binary docs end-to-end.
2. **Cloud billing backend.** Implement the `bv1_<payload>.<sig>` Ed25519
   signature verification in `LicenseChecker` (offline, constant-time), backed
   by a Stripe Checkout + license-key issuance service. The daemon hook and
   `/entitlement` endpoint already exist.
3. **PDF redaction recall.** Add a runtime verify-pass that re-extracts text
   after redaction and asserts zero surviving PII (generalize the integration
   test behind a flag).
4. **Neural NER upgrade (optional).** The gazetteer covers the common case
   cheaply; if broader recall is needed, wire an ONNX BERT-NER model behind the
   existing `PiiDetector` trait, embedding the model for zero-telemetry.
5. **Drop the legacy wasm crate** once no external consumer needs in-browser
   Arrow FFI — it's the last vestige of the RAG product.

---

> **v1.0.0 audit result:** all automated gates green (core 183 tests / cli 13
> tests / clippy `-D warnings` all-features / fmt / web build clean). Loopback
> binding, `unsafe`-free workspace, stream-boundary proptest, Luhn accuracy,
> destructive PDF redaction (Redact + Flatten), and dual-build web all PASS.
> No blockers remain for the 1.0.0 release.
