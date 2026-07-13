# Changelog

All notable changes to this project are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/2.0.0.html).

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
