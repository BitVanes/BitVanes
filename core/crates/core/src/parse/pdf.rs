//! Native PDF text extraction (requires the `cli-pdf` feature).
//!
//! PDF bytes are binary and cannot be UTF-8 decoded, so this parser is
//! invoked from [`crate::parse::parse_bytes`] *before* the UTF-8 conversion.
//! `pdf-extract` pulls the plain text out of the content streams; the result
//! is then structured into paragraphs by [`crate::parse::TextParser`].
//!
//! Limitation: this reads the embedded text layer only. Scanned image PDFs
//! (no text layer) produce no output and are reported as invalid input —
//! OCR is out of scope for the native path. The browser path uses PDF.js,
//! which has the same limitation.

use crate::error::{BitVanesError, Result};
use crate::parse::{Document, Parser, TextParser};
use crate::schema::PipelineConfig;

/// Parses PDF bytes into a structured [`Document`].
///
/// The extracted text is re-parsed as plain text (paragraph splitting), so
/// heading ancestry is generally empty — PDFs rarely carry reliable heading
/// structure in their text layer.
pub fn parse_pdf_bytes(bytes: &[u8], cfg: &PipelineConfig) -> Result<Document> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| BitVanesError::InvalidInput(format!("pdf extraction failed: {e}")))?;

    if text.trim().is_empty() {
        return Err(BitVanesError::InvalidInput(
            "pdf contains no extractable text (it may be a scanned image)".to_string(),
        ));
    }

    TextParser.parse(&text, cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::DocumentFormat;

    fn pdf_cfg() -> PipelineConfig {
        PipelineConfig {
            format: DocumentFormat::Pdf,
            ..PipelineConfig::default()
        }
    }

    #[test]
    fn empty_bytes_are_rejected() {
        let err = parse_pdf_bytes(&[], &pdf_cfg()).unwrap_err();
        assert!(
            matches!(err, BitVanesError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );
    }

    #[test]
    fn non_pdf_garbage_is_rejected() {
        let err = parse_pdf_bytes(b"definitely not a pdf", &pdf_cfg()).unwrap_err();
        assert!(
            matches!(err, BitVanesError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );
    }

    #[test]
    fn text_pdf_extracts_to_a_document() {
        let doc = parse_pdf_bytes(include_bytes!("../../tests/fixtures/hello.pdf"), &pdf_cfg())
            .expect("text pdf should parse");
        assert!(
            !doc.full_text.trim().is_empty(),
            "extracted text must be non-empty"
        );
        assert!(
            !doc.spans.is_empty(),
            "document must have at least one span"
        );
        assert!(
            doc.full_text.contains("Hello"),
            "extracted text should contain 'Hello': {:?}",
            doc.full_text
        );
    }
}
