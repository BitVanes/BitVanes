//! # bitvanes-core
//!
//! Zero-trust ETL engine for AI/RAG workloads. Pure library crate with no
//! `wasm-bindgen` imports — fully testable as native Rust.
//!
//! The same `bitvanes-core` rlib is linked by:
//!
//! - `bitvanes-wasm` (compiled to `wasm32-unknown-unknown` for the web), and
//! - `bitvanes-cli` (compiled to a native binary for DevOps flows).
//!
//! ## Pipeline
//!
//! The four-stage pipeline runs in [`pipeline::run_pipeline`]:
//!
//! 1. **Parse** ([`parse`]): Markdown / HTML / text into [`Document`] with
//!    structural [`TextSpan`]s carrying heading ancestry and section kinds.
//! 2. **Scrub** ([`scrub`]): PII redaction via regex + Luhn, with an
//!    [`OffsetMap`][`scrub::OffsetMap`] for position projection.
//! 3. **Chunk** ([`chunk`]): BPE-aware splitting at structural boundaries
//!    using any of six `OpenAI` tokenizers.
//! 4. **Assemble** ([`arrow_io`]): Arrow `RecordBatch` with 11 columns
//!    (including `chunk_id` and `pii_metadata`), exported via zero-copy FFI.
//!
//! ## Module layout
//!
//! - [`schema`] - domain types (`PipelineConfig`, `ChunkSpec`,
//!   `EmbeddingConfig`, ...). All config types are `serde`-serializable.
//! - [`error`] - the [`BitVanesError`] enum and [`Result`] alias.
//! - [`parse`] - format parsers (Markdown, HTML, text). Pluggable via
//!   the [`Parser`] trait.
//! - [`scrub`] - PII scrubbing with built-in patterns and custom regex.
//! - [`tokenize`] - BPE token counting and boundary-aware splitting.
//! - [`chunk`] - structural-boundary-aware chunker.
//! - [`arrow_io`] - Arrow `RecordBatch` assembly, FFI export registry,
//!   IPC streaming, and CSV output.
//! - [`embed`] - [`Embedder`] trait for on-device embedding generation.
//! - [`pipeline`] - full pipeline orchestration tying all stages together.

#![deny(unsafe_code)]

pub mod arrow_io;
pub mod chunk;
pub mod embed;
pub mod error;
pub mod parse;
pub mod pii_detect;
pub mod pipeline;
pub mod schema;
pub mod scrub;
pub mod tokenize;

#[cfg(feature = "mmap")]
pub mod mmap;

pub use arrow_io::{EMBEDDING_DIM, output_schema};
pub use embed::Embedder;
pub use error::{BitVanesError, Result};
pub use parse::{
    Document, HtmlParser, JsonParser, MarkdownParser, Parser, TextParser, TextSpan, parse_bytes,
    parse_str,
};
pub use pii_detect::{ModelDetector, PiiDetector};
pub use pipeline::{run_pipeline, run_pipeline_with_embeddings, run_pipeline_with_strategy};
pub use schema::{
    BuiltInPattern, ChunkConfig, ChunkSpec, ChunkStrategy, CustomPattern, DocumentFormat,
    EmbeddingConfig, PipelineConfig, ScrubProfile, SectionKind, TokenizerKind,
};
pub use scrub::{OffsetMap, PiiFinding, Scrubber};

#[cfg(feature = "embeddings")]
pub use embed::OrtEmbedder;
#[cfg(feature = "parallel")]
pub use pipeline::run_pipeline_batch;

/// Returns the semver version of the `bitvanes-core` crate, baked in at
/// compile time via the `CARGO_PKG_VERSION` environment variable.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
