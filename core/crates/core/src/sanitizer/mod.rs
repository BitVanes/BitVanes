//! Output policies for redacted PII and run-level sanitization statistics.
//!
//! The [`pii::detect`] engine finds PII and emits [`PiiFinding`]s with
//! original-text offsets. This module decides the *shape* of the replacement
//! emitted into the sanitized output:
//!
//! - [`RedactionPolicy::Mask`] — mask with a fixed character (`***`,
//!   `XXXX-XX-XXXX`, ...).
//! - [`RedactionPolicy::Placeholder`] — emit a typed placeholder such as
//!   `[REDACTED_SSN]` or `[REDACTED_NAME]` (the engine default for the
//!   existing `[EMAIL]` / `[SSN]` style).
//! - [`RedactionPolicy::Hash`] — emit a content-derived token such as
//!   `[SHA256:8f3a…]` so joins/re-identification are possible without
//!   exposing the secret.
//!
//! [`pii::detect`]: crate::pii::detect
//! [`PiiFinding`]: crate::pii::PiiFinding

pub mod policy;
pub mod report;

#[cfg(feature = "stream")]
pub mod stream;

#[cfg(feature = "pdf-redact")]
pub mod pdf;

#[cfg(feature = "pdf-redact")]
pub mod pdfium;

pub use policy::{RedactionPolicy, sanitize_text};
pub use report::{CategoryCount, SanitizationStats};

#[cfg(feature = "stream")]
pub use stream::{DEFAULT_MAX_MATCH_LEN, StreamSanitizer};

#[cfg(feature = "pdf-redact")]
pub use pdf::{PdfSanitizationResult, sanitize_pdf_text};

#[cfg(feature = "pdf-redact")]
pub use pdfium::{PdfRedactMode, PdfSanitizeResult as PdfiumSanitizeResult, redact_pdf};
