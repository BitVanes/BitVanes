//! PDF sanitization (text-layer path).
//!
//! - **Text-layer sanitization** ([`sanitize_pdf_text`]): extract the PDF's
//!   text layer, detect PII with the [`Scrubber`], and emit sanitized text
//!   (masked / placeholder / hashed per a [`RedactionPolicy`]). This needs no
//!   native backend — it works on any build with the `pdf-redact` feature.
//! - **Destructive coordinate-aware blackout** (`redact_pdf` in
//!   [`super::pdfium`]): rewrite the PDF content stream to *remove* the PII
//!   text bytes and/or flatten pages to images. Backed by `pdfium-render`
//!   (loaded at runtime); fail-closed if `libpdfium` is absent.
//!
//! [`Scrubber`]: crate::pii::Scrubber
//! [`RedactionPolicy`]: crate::sanitizer::RedactionPolicy

use crate::error::{BitVanesError, Result};
use crate::pii::{PiiFinding, Scrubber};
use crate::sanitizer::policy::{RedactionPolicy, sanitize_text};

/// Output of text-layer PDF sanitization.
#[derive(Debug, Clone)]
pub struct PdfSanitizationResult {
    /// The sanitized text of the PDF (PII replaced per `policy`).
    pub redacted_text: String,
    /// PII findings detected in the extracted text (original-text offsets).
    pub findings: Vec<PiiFinding>,
}

/// Extracts the PDF text layer, detects PII, and returns the sanitized text
/// plus the findings list. No PII survives in `redacted_text`.
///
/// Applicable only to PDFs with an embedded text layer; scanned image PDFs
/// produce [`BitVanesError::InvalidInput`] (OCR is out of scope).
///
/// # Errors
///
/// - [`BitVanesError::InvalidInput`] if the bytes are not a PDF or the PDF has
///   no extractable text.
/// - [`BitVanesError`] propagated from [`sanitize_text`] (e.g. a finding offset
///   not on a UTF-8 boundary — should not occur for pdf-extract output).
pub fn sanitize_pdf_text(
    bytes: &[u8],
    scrubber: &Scrubber,
    policy: &RedactionPolicy,
) -> Result<PdfSanitizationResult> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| BitVanesError::InvalidInput(format!("pdf extraction failed: {e}")))?;
    if text.trim().is_empty() {
        return Err(BitVanesError::InvalidInput(
            "pdf contains no extractable text (it may be a scanned image)".to_string(),
        ));
    }
    let (_engine_redacted, _offset_map, findings) = scrubber.scrub(&text);
    let redacted_text = sanitize_text(&text, &findings, policy)?;
    Ok(PdfSanitizationResult {
        redacted_text,
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{BuiltInPattern, ScrubProfile};

    fn email_scrubber() -> Scrubber {
        Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Email, BuiltInPattern::Ssn],
            ..ScrubProfile::default()
        })
        .expect("scrubber compiles")
    }

    #[test]
    fn text_pdf_is_sanitized_and_findings_recorded() {
        let bytes = include_bytes!("../../tests/fixtures/hello.pdf");
        let res = sanitize_pdf_text(bytes, &email_scrubber(), &RedactionPolicy::default())
            .expect("hello.pdf sanitizes");
        assert!(!res.redacted_text.trim().is_empty());
        assert!(res.redacted_text.contains("Hello"));
        assert!(res.findings.is_empty(), "hello.pdf has no PII");
    }

    #[test]
    fn garbage_input_is_rejected() {
        let err = sanitize_pdf_text(b"not a pdf", &email_scrubber(), &RedactionPolicy::default())
            .unwrap_err();
        assert!(matches!(err, BitVanesError::InvalidInput(_)));
    }
}
