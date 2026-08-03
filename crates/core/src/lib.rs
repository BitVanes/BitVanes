//! # bitvanes-core
//!
//! Zero-trust data purification engine: detect, redact, and sanitize PII
//! across document and byte streams. Pure library crate with no
//! `wasm-bindgen` imports — fully testable as native Rust.
//!
//! The same `bitvanes-core` rlib is linked by:
//!
//! - `bitvanes-wasm` (compiled to `wasm32-unknown-unknown` for the web), and
//! - `bitvanes-cli` (compiled to a native binary for DevOps flows).
//!
//! ## Pipeline
//!
//! The pipeline runs in [`pipeline::run_pipeline`]:
//!
//! 1. **Parse** ([`parse`]): Markdown / HTML / text / office / PDF into a
//!    [`Document`] with structural [`TextSpan`]s carrying heading ancestry
//!    and section kinds.
//! 2. **Scrub** ([`pii`]): PII detection + redaction via regex + Luhn/ABA,
//!    with an [`OffsetMap`][`pii::OffsetMap`] for position projection.
//! 3. **Chunk** ([`chunk`]): optional BPE-aware splitting at structural
//!    boundaries using any of six `OpenAI` tokenizers.
//! 4. **Assemble** ([`arrow_io`]): Arrow `RecordBatch` (10 columns, including
//!    `chunk_id` and `pii_metadata`), exported via zero-copy FFI.
//!
//! ## Module layout
//!
//! - [`schema`] - domain types (`PipelineConfig`, `ChunkSpec`, ...). All
//!   config types are `serde`-serializable.
//! - [`error`] - the [`BitVanesError`] enum and [`Result`] alias.
//! - [`parse`] - format parsers. Pluggable via the [`Parser`] trait.
//! - [`pii`] - PII detection + redaction with built-in patterns, custom
//!   regex, and a Tier-2 NER plug-in trait.
//! - [`sanitizer`] - output policies (masked placeholders, SHA-256 hashes)
//!   and run-level sanitization statistics.
//! - [`tokenize`] - BPE token counting and boundary-aware splitting.
//! - [`chunk`] - structural-boundary-aware chunker.
//! - [`arrow_io`] - Arrow `RecordBatch` assembly, FFI export registry,
//!   IPC streaming, and CSV output.
//! - [`pipeline`] - full pipeline orchestration tying all stages together.

#![deny(unsafe_code)]

pub mod arrow_io;
pub mod chunk;
pub mod error;
pub mod parse;
pub mod pii;
pub mod pipeline;
pub mod sanitizer;
pub mod schema;
pub mod tokenize;

#[cfg(feature = "config")]
pub mod config;

#[cfg(feature = "mmap")]
pub mod mmap;

pub use arrow_io::output_schema;
pub use error::{BitVanesError, Result};
pub use parse::{
    Document, HtmlParser, JsonParser, MarkdownParser, Parser, TextParser, TextSpan, parse_bytes,
    parse_str,
};
pub use pii::{ModelDetector, OffsetMap, PiiDetector, PiiFinding, Scrubber};
pub use pipeline::run_pipeline;
pub use schema::{
    BuiltInPattern, ChunkConfig, ChunkSpec, ChunkStrategy, CustomPattern, DocumentFormat,
    PipelineConfig, ScrubProfile, SectionKind, TokenizerKind,
};

#[cfg(feature = "parallel")]
pub use pipeline::run_pipeline_batch;

#[cfg(feature = "config")]
pub use config::BitvanesConfig;

/// Returns the semver version of the `bitvanes-core` crate, baked in at
/// compile time via the `CARGO_PKG_VERSION` environment variable.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
