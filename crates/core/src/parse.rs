//! Document parsing: turning raw input bytes into logical [`Document`]s.
//!
//! The parser is the first stage of the pipeline. It runs BEFORE PII
//! scrubbing and BEFORE tokenization. Its job is to:
//!
//! 1. Convert raw bytes to a UTF-8 string (rejecting invalid UTF-8).
//! 2. Walk the source format's structure (Markdown events, paragraphs, etc.)
//!    and emit a sequence of [`TextSpan`]s.
//! 3. Each span carries its structural context (heading ancestry, section
//!    kind) so the chunker can produce contextually-aware chunks.
//!
//! All spans reference into a single shared [`Document::full_text`] buffer
//! via half-open `[start, end)` character offsets. Spans are contiguous:
//! for any two adjacent spans `s[i]` and `s[i+1]`,
//! `s[i].char_offset_end == s[i+1].char_offset_start`. This invariant is
//! verified by tests and relied on by the scrubber.
//!
//! ## Format dispatch
//!
//! [`parse_bytes`] is the entry point used by both the wasm wrapper and the
//! CLI. It selects a parser based on [`PipelineConfig::format`].
//!
//! ## PDF note
//!
//! [`DocumentFormat::Pdf`] is binary, so it cannot go through the UTF-8
//! string path. [`parse_bytes`] routes PDF bytes directly to the native
//! `pdf` module (behind the `cli-pdf` feature) *before* attempting UTF-8
//! decoding. When `cli-pdf` is not compiled in, PDF requests surface a
//! [`BitVanesError::ParserUnavailable`]. The browser path instead extracts
//! text via PDF.js in JavaScript and reaches the engine as
//! [`DocumentFormat::Markdown`] or [`DocumentFormat::Text`].

use crate::error::{BitVanesError, Result};
use crate::schema::{DocumentFormat, PipelineConfig, SectionKind};

pub mod html;
pub mod json;
pub mod markdown;
pub mod text;

#[cfg(feature = "cli-pdf")]
pub mod pdf;

#[cfg(feature = "office")]
pub mod docx;
#[cfg(feature = "office")]
pub mod epub;
#[cfg(feature = "office")]
pub mod pptx;
#[cfg(feature = "office")]
pub mod rtf;
#[cfg(feature = "office")]
pub mod xlsx;
#[cfg(feature = "office")]
pub mod zip_util;

pub use html::HtmlParser;
pub use json::JsonParser;
pub use markdown::MarkdownParser;
pub use text::TextParser;

/// A parsed document: the full plain-text content plus structural spans
/// referencing into it.
///
/// Both fields are owned; the parser allocates `full_text` once and pushes
/// into it as it walks the source format.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    /// The full plain-text content of the document with all source-format
    /// markup stripped. Spans reference into this buffer via character
    /// offsets.
    pub full_text: String,

    /// Logical spans within `full_text`, each carrying structural context.
    /// Always sorted by `char_offset_start` and contiguous.
    pub spans: Vec<TextSpan>,
}

/// A logical region of text within a [`Document`] with consistent
/// structural context.
///
/// The text content of this span is `doc.full_text[char_offset_start..char_offset_end]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSpan {
    /// Half-open `[start, end)` character offset into
    /// [`Document::full_text`].
    pub char_offset_start: u32,
    /// End offset (exclusive).
    pub char_offset_end: u32,

    /// Enclosing heading ancestry, H1 outermost. Copied verbatim into the
    /// `heading_path` column of the output `RecordBatch` for every chunk
    /// derived from this span.
    pub heading_path: Vec<String>,

    /// Structural classification of this span's source region.
    pub section_kind: SectionKind,
}

impl TextSpan {
    /// Returns the character length of this span.
    #[must_use]
    pub fn len(&self) -> usize {
        (self.char_offset_end - self.char_offset_start) as usize
    }

    /// Returns `true` if this span contains no characters.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.char_offset_end == self.char_offset_start
    }

    /// Borrows the span's text from the document buffer.
    #[must_use]
    pub fn text<'a>(&self, doc: &'a Document) -> &'a str {
        let start = self.char_offset_start as usize;
        let end = self.char_offset_end as usize;
        &doc.full_text[start..end]
    }
}

impl Document {
    /// Test-only assertion that spans are contiguous and cover `full_text`
    /// exactly with no gaps or overlaps. Used by parser tests across the
    /// crate to verify the contiguity invariant.
    #[cfg(test)]
    pub(crate) fn assert_spans_contiguous(&self) {
        if let Some(first) = self.spans.first() {
            assert_eq!(
                first.char_offset_start, 0,
                "first span must start at offset 0"
            );
        }
        for pair in self.spans.windows(2) {
            assert_eq!(
                pair[0].char_offset_end, pair[1].char_offset_start,
                "spans must be contiguous (gap or overlap detected)"
            );
        }
        if let Some(last) = self.spans.last() {
            assert_eq!(
                last.char_offset_end as usize,
                self.full_text.len(),
                "last span must end at full_text.len()"
            );
        }
    }
}

/// A parser converts a source-format string into a [`Document`].
///
/// Implementations are stateless unit structs (`MarkdownParser`,
/// `TextParser`). The trait exists to enforce a uniform contract across
/// formats and to allow the chunker to be format-agnostic.
pub trait Parser {
    /// Parse `input` into a [`Document`]. The `cfg` reference is available
    /// for format-specific options (none currently, but reserved).
    fn parse(&self, input: &str, cfg: &PipelineConfig) -> Result<Document>;
}

/// Parses raw bytes as UTF-8 and dispatches to the format-specific parser
/// selected by [`PipelineConfig::format`].
///
/// This is the entry point used by both the wasm wrapper (via
/// `serde-wasm-bindgen` config bridge) and the CLI.
///
/// # Errors
///
/// Returns [`BitVanesError::InvalidInput`] if the bytes are not valid UTF-8,
/// or [`BitVanesError::ParserUnavailable`] if the requested format is not
/// compiled into this build (e.g. PDF without the `cli-pdf` feature).
pub fn parse_bytes(bytes: &[u8], cfg: &PipelineConfig) -> Result<Document> {
    // Binary container formats are routed before UTF-8 decoding.
    #[cfg(feature = "cli-pdf")]
    if cfg.format == DocumentFormat::Pdf {
        return pdf::parse_pdf_bytes(bytes, cfg);
    }
    #[cfg(not(feature = "cli-pdf"))]
    if cfg.format == DocumentFormat::Pdf {
        return Err(BitVanesError::ParserUnavailable(
            "pdf parsing requires the `cli-pdf` feature (native only)",
        ));
    }

    #[cfg(feature = "office")]
    {
        match cfg.format {
            DocumentFormat::Docx => return docx::parse_docx_bytes(bytes, cfg),
            DocumentFormat::Pptx => return pptx::parse_pptx_bytes(bytes, cfg),
            DocumentFormat::Xlsx => return xlsx::parse_xlsx_bytes(bytes, cfg),
            DocumentFormat::Epub => return epub::parse_epub_bytes(bytes, cfg),
            DocumentFormat::Rtf => return rtf::parse_rtf_bytes(bytes, cfg),
            _ => {}
        }
    }
    #[cfg(not(feature = "office"))]
    if matches!(
        cfg.format,
        DocumentFormat::Docx
            | DocumentFormat::Pptx
            | DocumentFormat::Xlsx
            | DocumentFormat::Epub
            | DocumentFormat::Rtf
    ) {
        return Err(BitVanesError::ParserUnavailable(
            "office-format parsing requires the `office` feature (native only). \
             The browser must extract text via JS and pass it as markdown/text.",
        ));
    }

    let input = std::str::from_utf8(bytes)
        .map_err(|e| BitVanesError::InvalidInput(format!("input is not valid UTF-8: {e}")))?;
    parse_str(input, cfg)
}

/// Like [`parse_bytes`] but accepts a string slice directly.
///
/// PDF cannot be parsed through this entry point because it requires raw
/// bytes; a [`DocumentFormat::Pdf`] request here always returns
/// [`BitVanesError::ParserUnavailable`].
pub fn parse_str(input: &str, cfg: &PipelineConfig) -> Result<Document> {
    match cfg.format {
        DocumentFormat::Markdown => MarkdownParser.parse(input, cfg),
        DocumentFormat::Text => TextParser.parse(input, cfg),
        DocumentFormat::Html => HtmlParser.parse(input, cfg),
        DocumentFormat::Json => JsonParser.parse(input, cfg),
        DocumentFormat::Pdf
        | DocumentFormat::Docx
        | DocumentFormat::Pptx
        | DocumentFormat::Xlsx
        | DocumentFormat::Epub
        | DocumentFormat::Rtf => Err(BitVanesError::ParserUnavailable(
            "binary formats (pdf/docx/pptx/xlsx/epub/rtf) must be parsed from bytes via parse_bytes",
        )),
    }
}

/// Converts a `usize` character offset to `u32`.
///
/// Panics if the document exceeds `u32::MAX` characters (~4 GiB), which
/// would exceed the wasm linear memory budget regardless.
#[track_caller]
pub(crate) fn offset_to_u32(n: usize) -> u32 {
    u32::try_from(n).expect("document offset exceeds u32::MAX (>4 GiB)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bytes_rejects_invalid_utf8() {
        let cfg = PipelineConfig {
            format: DocumentFormat::Text,
            ..PipelineConfig::default()
        };
        // 0xFF is never valid UTF-8.
        let err = parse_bytes(&[0xFF, 0xFE, 0xFD], &cfg).unwrap_err();
        assert!(
            matches!(err, BitVanesError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );
    }

    #[test]
    fn parse_str_rejects_unimplemented_formats() {
        let cfg = |fmt| PipelineConfig {
            format: fmt,
            ..PipelineConfig::default()
        };

        let err = parse_str("hello", &cfg(DocumentFormat::Pdf)).unwrap_err();
        assert!(matches!(err, BitVanesError::ParserUnavailable(_)));
    }
}
