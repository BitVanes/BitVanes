//! Error types for the `BitVanes` core engine.
//!
//! Library-crate convention: a single error enum implementing
//! [`std::error::Error`] via [`thiserror`]. The companion [`Result`] alias
//! is used throughout the crate. Binary crates (`bitvanes-cli`) convert
//! [`BitVanesError`] into `anyhow::Error` at the boundary.

use thiserror::Error;

/// Top-level error type for all fallible `bitvanes-core` operations.
#[derive(Debug, Error)]
pub enum BitVanesError {
    /// A malformed or unsupported input document.
    #[error("invalid input document: {0}")]
    InvalidInput(String),

    /// A requested parser is not available in this build configuration
    /// (for example, PDF parsing requested without the `cli-pdf` feature).
    #[error("parser not available in this build: {0}")]
    ParserUnavailable(&'static str),

    /// A pipeline configuration is malformed, inconsistent, or out of range.
    #[error("invalid pipeline configuration: {0}")]
    InvalidConfig(String),

    /// A requested optional feature was not compiled into this build, or its
    /// runtime backend is unavailable (for example, PDF destructive redaction
    /// requested without a runtime `libpdfium` on the host).
    #[error("feature not enabled: {0}")]
    FeatureNotEnabled(Box<str>),

    /// A Tier-2 detector (e.g. the NER model) failed to load or run
    /// inference. Treated as **fail-closed**: if a configured detector cannot
    /// run, the pipeline refuses to emit output rather than risk leaking PII
    /// that the detector would have caught.
    #[error("pii detector inference failure: {0}")]
    Inference(String),

    /// An error from the underlying Apache Arrow library while building or
    /// exporting a [`RecordBatch`](arrow::record_batch::RecordBatch).
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Generic I/O error (used by the IPC stream writer and any native-only
    /// file paths).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, BitVanesError>;
