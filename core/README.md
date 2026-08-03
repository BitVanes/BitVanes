# bitvanes-core

Zero-trust data purification engine. Written in Rust, compiled to both
`wasm32-unknown-unknown` (for the browser) and native targets (for the CLI).
BitVanes directs, filters, and purifies streams of unstructured/binary data by
stripping sensitive PII and sanitizing payloads locally — before they reach
downstream APIs, databases, or AI models.

## What it does

A pipeline that detects, redacts, and sanitizes PII across documents and byte
streams, emitting clean output (and an audit log of every redaction):

1. **Parse** - Markdown, HTML, plain text, JSON, PDF, or office formats into
   structural spans with heading ancestry and section classification. PDF
   requires the `cli-pdf` feature natively; the browser extracts PDF text via
   PDF.js.
2. **Scrub** - PII detection + redaction (email, SSN, phone, credit card, API
   keys, ...) via regex + Luhn/ABA validation, with an offset-delta map for
   projecting positions back to the original document.
3. **Chunk** - optional splitting at structural boundaries, sized via a
   lightweight token estimator (~4 chars/token heuristic — no BPE dependency).
   The `tokenizer` field is retained on `ChunkConfig` for wire-format
   compatibility but all variants resolve to the same estimator.
4. **Assemble** - Arrow `RecordBatch` with 10 columns (chunk_index, text,
   token_count, source_path, heading_path, section_kind, char offsets,
   pii_metadata), exported via zero-copy FFI pointers.

## Zero-trust guarantees

- All vocab files are embedded at compile time via `include_str!`.
- No network calls during parsing, scrubbing, or tokenization.
- In the browser, data is processed in a Web Worker sandbox.

## Workspace layout

```
core/
  Cargo.toml              workspace manifest (2 crates)
  crates/
    core/                  bitvanes-core (pure library, no wasm-bindgen)
      src/
        schema.rs          PipelineConfig, ChunkSpec, ScrubProfile
        error.rs           BitVanesError
        parse/             markdown.rs, html.rs, text.rs, pdf.rs, office formats
        pii/               detect.rs (regex + Luhn/ABA + anchors), model.rs (NER trait)
        sanitizer/         policy.rs (mask/placeholder/hash), report.rs (stats)
        tokenize.rs        chars-per-token heuristic estimator (no BPE dep)
        chunk.rs           structural-boundary chunker
        arrow_io/          batch.rs, ffi.rs, ipc.rs, csv.rs
        pipeline.rs        full pipeline orchestration (+ run_pipeline_batch)
    wasm/                  bitvanes-wasm (thin #[wasm_bindgen] wrapper)
      src/lib.rs           process(), release_batch(), array_ptr(), schema_ptr()
```

## Build

```bash
# Native (for testing and the CLI)
cargo build --workspace
cargo test --workspace --all-features

# WebAssembly (for the browser)
wasm-pack build crates/wasm --target web --out-dir pkg
```

## Usage from JavaScript (legacy wasm binding)

> **Note:** the web dashboard no longer runs the engine in the browser — it
> POSTs to the local native daemon (see `web/`). The `bitvanes-wasm` crate is
> retained as a legacy embedding surface for consumers that want in-browser
> Arrow FFI; it is not exercised by the dashboard and may be removed in a
> future major release.

```javascript
import init, { process, array_ptr, schema_ptr, release_batch, version } from './bitvanes_wasm.js';
await init();

const config = {
  format: "markdown",
  scrub: { patterns: ["email", "ssn"], custom: [] },
  chunk: { max_tokens: 512, overlap_tokens: 0, tokenizer: "cl100k_base" },
  source_label: "doc.md",
};

const slotId = process(config, new Uint8Array(fileBytes));
// Read via arrow-js-ffi using array_ptr(slotId) and schema_ptr(slotId)
// Then:
release_batch(slotId);
```

## Cargo features

| Feature | Default | Description |
|---------|---------|-------------|
| `ipc` | no | Arrow IPC stream output (`StreamWriter`) for CLI piping |
| `csv` | no | Arrow CSV output for data export |
| `parallel` | no | Rayon-based parallel batch processing (native only) |
| `cli-pdf` | no | Native PDF text extraction via `pdf-extract` (not in wasm) |
| `office` | no | DOCX/PPTX/XLSX/EPUB/RTF parsing (native only) |
| `mmap` | no | Memory-mapped file I/O for large files (>1 MB) |
| `stream` | no | Async rolling-window sanitizer over `tokio::io` (text/JSON/CSV pipes) |
| `pdf-redact` | no | PDF sanitization: text-layer redaction + destructive coordinate-aware blackout/flatten via `pdfium-render` (implies `cli-pdf`) |
| `config` | no | `Bitvanes.toml` user-facing config loader |
| `pii-model` | no | Tier-2 NER plug-in trait (stub — ONNX integration is future work) |

> **Zero-telemetry is unconditional.** Token counting is a pure-arithmetic
> heuristic (≈ 4 chars/token, no embedded vocab, no network code, no feature
> to disable), so every build is fully offline.

## Supported formats

| Format | Feature | Native only? | Notes |
|--------|---------|:------------:|-------|
| Markdown | — | no | pulldown-cmark, GFM |
| HTML | — | no | scraper / html5ever |
| Plain text | — | no | Paragraph-based splitting |
| JSON | — | no | Structural (object → heading_path) |
| PDF | `cli-pdf` | yes | pdf-extract (browser uses PDF.js) |
| **DOCX** | `office` | yes | ZIP + quick-xml, headings/tables/lists |
| **PPTX** | `office` | yes | Slide-by-slide text extraction |
| **XLSX** | `office` | yes | calamine, memory-guarded for large sheets |
| **EPUB** | `office` | yes | OPF spine + HtmlParser per chapter |
| **RTF** | `office` | yes | rtf-parser, heading heuristics |

## Performance

| Lever | Feature | Effect |
|-------|---------|--------|
| **Parallel regex sweep** | `parallel` | Rayon across PII patterns (~4–8× on 8 patterns) |
| **Memory-mapped I/O** | `mmap` | Zero-copy file reads for >1 MB files |
| **Streaming IPC output** | `ipc` | `IpcStream<W>` writes batches directly to disk/stdout |
| **Wasm SIMD** | (always on wasm) | `target-feature=+simd128` via `.cargo/config.toml` |

```bash
# Build the CLI with all performance features:
cargo build --release --features "office,mmap,parallel,ipc,csv,cli-pdf"
```

## Output schema

The Arrow `RecordBatch` emitted by `run_pipeline` has 10 columns:

| # | Column | Type | Nullable | Description |
|---|--------|------|----------|-------------|
| 0 | `chunk_index` | `UInt32` | no | 0-based ordinal |
| 1 | `chunk_id` | `Utf8` | no | Deterministic blake3 hash of chunk content |
| 2 | `text` | `Utf8` | no | Scrubbed (PII-redacted) chunk text |
| 3 | `token_count` | `UInt16` | no | BPE token count of `text` |
| 4 | `source_path` | `Utf8` | no | Source filename/label for lineage |
| 5 | `heading_path` | `List<Utf8>` | yes | Heading ancestry (H1→H6) |
| 6 | `section_kind` | `Dictionary<Int8, Utf8>` | no | paragraph, code, heading, table_cell, etc. |
| 7 | `char_offset_start` | `UInt32` | no | Start offset into scrubbed text |
| 8 | `char_offset_end` | `UInt32` | no | End offset into scrubbed text |
| 9 | `pii_metadata` | `List<Struct>` | yes | PII findings overlapping this chunk |

### `pii_metadata` struct fields

Each finding in the `pii_metadata` list is a struct with:

| Field | Type | Description |
|-------|------|-------------|
| `entity` | `Utf8` | Entity slug: `email`, `ssn`, `credit_card`, `routing_number`, ... |
| `confidence` | `Float32` | Weighted-additive score in `[0, 1]` |
| `offset_start` | `Int32` | Byte offset into the **original** (pre-scrub) text |
| `offset_end` | `Int32` | End byte offset (exclusive) |
| `anchors` | `List<Utf8>` | Contextual keywords that fired in the ±N-word window |

## PII scrubbing engine

Two-tier architecture with weighted-additive confidence scoring:

**Tier 1** (always available, no model download):

| Pattern | Base | Validator | Notes |
|---------|------|-----------|-------|
| `email` | 0.85 | — | RFC-5322-ish regex |
| `ssn` | 0.60 | — | US Social Security `XXX-XX-XXXX` |
| `phone` | 0.65 | — | E.164 (`+1…`) |
| `credit_card` | 0.70 | **Luhn (soft)** | Floors at 0.90 on pass; **0.55 on fail but still redacted** (fail-safe) |
| `routing_number` | 0.55 | **ABA checksum gate** | 9-digit US bank routing |
| `street_address` | 0.45 | — | US street address (heuristic: number + capitalized name + suffix) |
| `aws_key` | 0.95 | — | `AKIA…` prefix |
| `github_pat` | 0.95 | — | `ghp_…` / `gho_…` prefix |
| `jwt` | 0.90 | — | Three base64url segments |

Each candidate's confidence is boosted by contextual anchor keywords
(e.g. `"social security"`, `"credit card"`) found in a configurable
±N-word sliding window (default: 7 words, `+0.10` per hit, capped at `0.99`).

> **Name detection** uses a **customer-supplied gazetteer**
> (`ScrubProfile::names`) plus an opt-in bundled common-name starter list
> (`use_generic_names`). Names are matched case-insensitively on word
> boundaries — far more precise than regex name heuristics, at the cost of
> requiring the customer to supply the names relevant to their corpus. A
> future neural NER model can layer behind the `PiiDetector` trait
> (`pii-model` feature).

## Sanitization output policies

The [`sanitizer`] module decides the shape of the replacement emitted for each
detected PII span:

- `RedactionPolicy::Placeholder` — `[REDACTED_SSN]`, `[REDACTED_EMAIL]` (default).
- `RedactionPolicy::Mask { mask_char }` — `********` (preserves match length).
- `RedactionPolicy::Hash { hex_chars }` — `[SHA256:8f3a9c12]` (deterministic,
  enables offline join without revealing the secret).

`SanitizationStats` aggregates per-file counts (files, bytes, PII-by-type,
throughput in MiB/s) for the CLI/daemon completion summary.

**Tier 2** (stub): the `PiiDetector` trait + `ModelDetector` placeholder
(gated behind the `pii-model` feature) provides the plug-in point for a
future local NER model (e.g. for names and street addresses). The pipeline
contract is unchanged: one finding list, one offset map, one `pii_metadata`
column.

## License

Dual-licensed under MIT OR Apache-2.0.
