# Changelog

All notable changes to this project are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/2.0.0.html).

## [1.0.0] — Zero-trust data purification engine

First stable release. The single v1.0 blocker — destructive coordinate-aware
PDF redaction — is resolved. All automated gates are green.

### Added — destructive PDF redaction (`pdf-redact` feature, Phase 4)
- **`sanitizer::pdfium::redact_pdf`** — coordinate-aware destructive redaction
  backed by `pdfium-render`, which binds a runtime `libpdfium` (no compile-time
  native link). Two modes:
  - **`Redact`** (default): for each PII span the bounding rectangle is
    computed from glyph boxes, every text page object overlapping it is
    **deleted** from the page-object tree (the bytes cannot be extracted or
    copied), and a solid black rectangle is drawn over the region.
  - **`Flatten`**: blackout rectangles are drawn, the page is rasterized to a
    300 DPI bitmap, the entire vector/text object layer is dropped, and the
    image becomes the page's only object — no recoverable text layer.
- `--pdf-mode redact|flatten|text-only` on `bitvanes scrub`. Fails **closed**
  (`FeatureNotEnabled`) if `libpdfium` is absent, so unredacted bytes are never
  emitted.

### Added — credit-card fail-safe detection
- `Validator::Luhn` is now a **soft confidence modifier**, not a hard gate: a
  card-shaped number that fails the Luhn checksum is still flagged and
  redacted at lower confidence (`0.55`) instead of being silently dropped.
  Rationale: a transcription/OCR error on a real card must not leak the card
  number. Raise `min_confidence` (e.g. `0.85`) to restore precision.
- ABA routing-number validation remains a hard gate (9-digit numbers are too
  common to flag without the checksum).

### Added — Tier-2 personal-name gazetteer
- `ScrubProfile::names` — a customer-supplied list of name phrases, matched
  case-insensitively on word boundaries (e.g. `["Jane Doe", "Akhmad"]`).
  Multi-token names score higher than single tokens. Wired into `Bitvanes.toml`
  as `[pii] names = [...]`. Far more precise than regex name heuristics; the
  customer supplies the names relevant to their corpus.
- `ScrubProfile::use_generic_names` — opt-in bundled common-given-name starter
  list (off by default; generic names are FP-prone on general prose).

### Changed — tiktoken-rs removed
- The `tiktoken-rs` dependency (which embedded multi-MB BPE vocab files via
  `include_str!`) is **removed**. Token counting now uses a pure-arithmetic
  heuristic (≈ 4 chars/token). The `Tokenizer` API and `ChunkConfig.tokenizer`
  field are retained for wire-format compatibility; all `TokenizerKind`
  variants resolve to the same estimator. The chunker and TUI behave
  identically; the heavy vocab dependency is gone.
- `BitVanesError::FeatureNotEnabled` now carries `Box<str>` (was `&'static str`)
  so it can describe a runtime-missing backend with dynamic detail.
- `redact_pdf_blackout` (the always-erroring placeholder stub) is **removed**;
  the real implementation is `sanitizer::pdfium::redact_pdf`.

### Verification
- core: 183 unit/integration/doctests passing; `clippy -D warnings` clean
  across all features; `cargo fmt --check` clean.
- cli: 13 tests passing; `clippy -D warnings` clean; release build with the
  `dashboard` feature produces a self-contained `bitvanes` binary.
- web: `npm run build` clean (206 KB / 65 KB gzipped JS).

## [1.0.0-rc.1] — Production-readiness audit

Release candidate. Promoting to a clean 1.0.0 is gated on the single item
listed under **Known limitations** below.

### Audit fixes
- Purged 6 residual `RAG` references in source comments (json/pptx/markdown/
  schema parsers + the TUI title bar) — the codebase is now legacy-term-free.
- CLI `filter` / `scrub` / `daemon` now apply a **default ruleset**
  (email, ssn, phone, credit_card) when neither `--rules` nor a `Bitvanes.toml`
  is supplied, so bare `bitvanes filter` redacts PII out of the box.
- Added a `proptest` for the stream-boundary invariant (SSN split across an
  arbitrary byte boundary is always redacted; 256 random cases).

### Added — daemon dashboard (CLI `dashboard` feature)
- `/scrub` POST endpoint returning `{ redacted, total, categories }` JSON for
  the local dashboard.
- `rust-embed` compile-time embedding of `web/dist`; `bitvanes daemon` now
  serves the dashboard with no `--dashboard-dir` when built with
  `--features dashboard`.
- Drag-and-drop dashboard UI (file → `/scrub` → before/after + categorized
  findings).

### Known limitations at rc.1
- Destructive coordinate-aware PDF redaction was **not yet implemented** at the
  rc.1 cut — text-layer PDF sanitization was shipped (`pdf-redact` feature),
  but the destructive content-stream removal / page-flatten path needed the
  `pdfium-render` backend. The stub failed closed (`FeatureNotEnabled`). **This
  was resolved in 1.0.0** (see above).

## [0.5.0] — Rebrand to Zero-Trust Data Purification Engine

BitVanes is pivoting from a "RAG / LLM document chunker" to a **zero-trust
data purification & stream filtering engine**. This release removes the
embedding/RAG surface from the core library. Streaming, PDF coordinate
redaction, the `Bitvanes.toml` config, and the daemon are tracked in
`REBRAND.md` and land in subsequent phases.

### Removed — breaking (wire format)
- **`embeddings` cargo feature** and the ONNX Runtime integration (`ort`,
  `tokenizers`, `ndarray` dependencies) deleted entirely.
- **`EmbeddingConfig`** struct and the `PipelineConfig.embeddings` field
  removed. Old `profile.json` payloads carrying an `embeddings` key will fail
  to deserialize — a deliberate wire break.
- **`embed.rs` module** deleted: the `Embedder` trait, `OrtEmbedder`,
  `mean_pool`, and `l2_normalize` are gone.
- **`run_pipeline_with_embeddings`** and **`run_pipeline_with_strategy`**
  removed from `pipeline`. Use `run_pipeline`.
- **`ChunkStrategy::Semantic`** removed (it was embedding-guided).
  `ChunkStrategy` is now `#[non_exhaustive]` with only `Structural`;
  `ChunkConfig` derives `Eq` again.
- **`chunk_document_semantic`** removed from `chunk`.
- **Arrow `embedding` column** removed; the output schema is now **10
  columns** (was 11). `EMBEDDING_DIM`, `output_schema_with_dim`,
  `chunks_to_batch_with_embeddings`, `build_null_embedding`, and
  `build_real_embedding` removed.
- **`examples/custom_embedder.rs`** deleted.

### Changed
- Workspace `keywords`/`categories` rebranded (`rag`/`ai`/`wasm` →
  `pii`/`redaction`/`privacy`/`sanitization`).
- Crate description rewritten to the purification value proposition.

### Added — module reorganization (Phase 2)
- **`pii/` module**: `scrub.rs` renamed to `pii/detect.rs` and `pii_detect.rs`
  to `pii/model.rs` under a new `pii/mod.rs` parent. Canonical paths are now
  `bitvanes_core::pii::{Scrubber, PiiFinding, OffsetMap, PiiDetector,
  ModelDetector, scrub_document, scrub_text}`. The old `crate::scrub::` and
  `crate::pii_detect::` paths are removed (breaking).
- **`sanitizer/` module** (new): configurable output policies
  (`RedactionPolicy::{Placeholder, Mask, Hash}`) applied via `sanitize_text`,
  plus `SanitizationStats` (files / bytes / PII-by-type / MiB·s⁻¹ throughput)
  for run-level reporting.
- **`StreetAddress` built-in pattern**: conservative US street-address
  heuristic (building number + capitalized name + suffix), low base
  confidence with anchor boosting. `Name` detection is deferred to the
  Tier-2 NER trait (regex name matching is too FP-prone to ship).
- `sha2` dependency added for the `[SHA256:…]` hash policy.

### Added — async streaming sanitizer (Phase 3)
- **`stream` cargo feature** + `sanitizer::stream::StreamSanitizer`: bounded-memory
  rolling-window redaction over `tokio::io::AsyncRead` / `AsyncWrite`. Holds back
  a configurable match window (`max_match_len`, default 1024) so PII split across
  read-chunk boundaries is still redacted, and never splits a UTF-8 codepoint.
  Memory use is `O(max_match_len)` regardless of input size. Applicable to
  text/JSON/CSV pipes (binary container formats go through the document path).
- New deps: `tokio` (io-util/rt/macros) and `bytes`, both behind `stream`.

## [0.4.0] — 2026-07

### Added — File-format breadth
- **DOCX parser** (`parse/docx.rs`): ZIP + quick-xml streaming extraction of
  paragraphs, headings (`<w:pStyle>`), tables, and list items.
- **PPTX parser** (`parse/pptx.rs`): slide-by-slide text extraction with
  `heading_path = ["Slide N"]`.
- **XLSX parser** (`parse/xlsx.rs`): via `calamine`, with memory guards for
  large/sparse spreadsheets (cell-count cap, empty-row trimming, one-sheet-
  at-a-time loading).
- **EPUB parser** (`parse/epub.rs`): OPF spine ordering + per-chapter XHTML
  extraction via the existing `HtmlParser`.
- **RTF parser** (`parse/rtf.rs`): via `rtf-parser` with heading heuristics.
- New `office` feature flag gates `zip`, `quick-xml`, `calamine`, `rtf-parser`.

### Added — Performance tier
- **Streaming IPC writer** (`arrow_io::ipc::IpcStream<W>`): writes one
  `RecordBatch` per file directly to a `Write` sink — flat memory regardless
  of batch size. Enables true `bitvanes parse ./docs --format arrow | vector-db-ingest`.
- **Memory-mapped file I/O** (`mmap` feature): `memmap2`-backed zero-copy
  reads for files > 1 MB. Falls back to `fs::read` on failure.
- **Parallel regex sweep** (`parallel` feature): `rayon::par_iter` across
  compiled PII patterns — ~4–8× on multi-pattern configs for large documents.
- **Parallel embedding generation**: `embeddings` feature now implies
  `parallel`, enabling batch-wise ONNX inference.

### Added — Parser infrastructure
- `DocumentFormat` is now `#[non_exhaustive]` with 5 new variants (`Docx`,
  `Pptx`, `Xlsx`, `Epub`, `Rtf`).
- `parse/zip_util.rs`: shared ZIP-entry reader for DOCX/PPTX/EPUB.
- Entity-encoding workaround for quick-xml 0.41's streaming entity splitting.

### Added — CLI & UX
- **`--exclude-pii` flag**: detect-and-report mode. Entities listed are
  recorded in `pii_metadata` but the original text passes through unchanged.
- **`report_only` field** on `ScrubProfile`: core-side config for the above.
- **Streaming Arrow output**: one `RecordBatch` per file directly to
  `BufWriter<File>` or stdout. Memory holds one batch at a time. Enables
  `bitvanes parse ./docs --format arrow -o - | vector-db-ingest`.
- **Two-level indicatif**: `MultiProgress` with stacked files + bytes bars.
- **`--init` flag**: writes a commented `bitvanes.toml` template to CWD.
- **Web: embeddings Web Worker**: `@xenova/transformers` moved off main
  thread. Main JS bundle dropped 1546→704 KB.
- **Web: help popups**: `?` icons on every config knob.

### Changed
- `embeddings` feature now implies `parallel`.
- CLI `infer_format` recognizes `.docx`, `.pptx`, `.xlsx`, `.epub`, `.rtf`.

## [0.3.0] — 2026-07

### Added
- **PII confidence scoring** — every finding now carries a weighted-additive
  confidence score `[0, 1]` (base per pattern + contextual anchor hits × delta,
  capped). Luhn/ABA gates floor or drop candidates.
- **Contextual anchor windows** — ±N-word sliding window (default 7) scans for
  keyword anchors (`"ssn"`, `"credit card"`, `"routing"`, …) around each
  candidate, boosting confidence and reducing false positives.
- **`routing_number` pattern** — 9-digit ABA routing numbers with checksum
  validation.
- **`pii_metadata` Arrow column** — `List<Struct{entity, confidence,
  offset_start, offset_end, anchors}>`, populated per chunk. Offsets reference
  the original (pre-scrub) text for audit/UI highlighting.
- **`chunk_id` Arrow column** — deterministic blake3 content hash for
  dedup downstream.
- **`PiiDetector` trait + `ModelDetector` stub** (`pii_detect` module) —
  plug-in point for Tier-2 ML/NLP PII detection. Gated behind `pii-model`.
- **`min_confidence` + `anchor_window`** config fields on `ScrubProfile`.
- **`proptest`** property tests for `OffsetMap` round-trip and finding bounds.
- **`criterion`** benchmarks for the scrubber hot loop (`cargo bench`).
- **Wasm SIMD** — `.cargo/config.toml` enables `target-feature=+simd128`.

### Changed
- Output schema expanded from 9 → **11 columns** (added `chunk_id`,
  `pii_metadata`). Existing columns are unchanged (backwards-compatible).
- `Scrubber::scrub` / `scrub_document` / `scrub_text` now return a 3-tuple
  including `Vec<PiiFinding>`.
- `BuiltInPattern` is now `#[non_exhaustive]`.
- `ScrubProfile` lost `Eq` (now `PartialEq` only, due to `f32 min_confidence`).

## [0.2.0] — 2026-06

### Added
- `examples/` — `basic_chunks`, `pii_scrub`, `custom_embedder` runnable demos.
- **Embedding-guided (semantic) chunking** — `ChunkStrategy::Semantic` merges
  adjacent spans while cosine similarity stays above a threshold, keeping
  topical units whole. See `chunk::strategy`.
- `CHANGELOG.md`, `SECURITY.md`, `CONTRIBUTING.md`.

### Changed
- `token_count` is now an exact BPE re-count of the emitted chunk text (was a
  sum of per-span counts, which could drift by ±1 at span junctions).

### Fixed
- Overlap drop at oversized spans: the overlap tail now carries across
  oversized-split boundaries (`0.1.1` backport).

## [0.1.1] — 2026-06-24

### Added
- `cli-pdf` feature: native PDF text extraction via `pdf-extract`.
- `parallel` feature: rayon-backed `run_pipeline_batch`.
- Re-exported `OrtEmbedder`, `run_pipeline_batch` at the crate root.
- CI workflow (fmt / clippy / test / wasm size gate).

### Changed
- Documented that BPE vocab embedding is **unconditional** (tiktoken-rs has no
  toggle) — removed the phantom `embed-vocab` feature from docs.

### Fixed
- `mean_pool` stride bug when configured `dim != hidden_dim`.
- `token_count` u16 truncation: `max_tokens` is now validated `<= 65535`.
- CSV writer panic replaced with `Result` propagation.

## [0.1.0] — 2026-06-22

Initial public release: four-stage pipeline (parse → scrub → chunk → Arrow),
six OpenAI tokenizers, seven PII patterns, zero-copy Arrow FFI for wasm.
