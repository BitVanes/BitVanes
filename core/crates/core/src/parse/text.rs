//! Plain-text fallback parser.
//!
//! Splits input on blank-line paragraph boundaries. Each non-empty
//! paragraph becomes a [`TextSpan`] of kind [`SectionKind::Paragraph`].
//! There is no heading ancestry - all `heading_path` values are empty.

use crate::error::Result;
use crate::parse::{Document, Parser as ParserTrait, TextSpan};
use crate::schema::{PipelineConfig, SectionKind};

/// Stateless plain-text parser. Splits on `\n\n` boundaries.
pub struct TextParser;

impl ParserTrait for TextParser {
    fn parse(&self, input: &str, _cfg: &PipelineConfig) -> Result<Document> {
        // Normalize Windows-style CRLF line endings to Unix LF so that
        // blank-line paragraph splitting works consistently across platforms.
        let normalized = input.replace("\r\n", "\n");
        let input = normalized.as_str();

        let mut full_text = String::with_capacity(input.len());
        let mut spans = Vec::new();

        for paragraph in input.split("\n\n") {
            let trimmed = paragraph.trim();
            if trimmed.is_empty() {
                continue;
            }
            let start = full_text.len();
            full_text.push_str(trimmed);
            let end = full_text.len();
            spans.push(TextSpan {
                char_offset_start: super::offset_to_u32(start),
                char_offset_end: super::offset_to_u32(end),
                heading_path: Vec::new(),
                section_kind: SectionKind::Paragraph,
            });
        }

        Ok(Document { full_text, spans })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::DocumentFormat;

    fn parse_text(input: &str) -> Document {
        let cfg = PipelineConfig {
            format: DocumentFormat::Text,
            ..PipelineConfig::default()
        };
        TextParser
            .parse(input, &cfg)
            .expect("text parse should not fail")
    }

    #[test]
    fn empty_input_produces_empty_document() {
        let doc = parse_text("");
        assert!(doc.full_text.is_empty());
        assert!(doc.spans.is_empty());
    }

    #[test]
    fn whitespace_only_input_produces_empty_document() {
        let doc = parse_text("   \n\n\n   \t\t\n\n");
        assert!(doc.spans.is_empty());
    }

    #[test]
    fn single_paragraph_emits_one_span() {
        let doc = parse_text("Just one paragraph of text.");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "Just one paragraph of text.");
        assert!(doc.spans[0].heading_path.is_empty());
        doc.assert_spans_contiguous();
    }

    #[test]
    fn multiple_paragraphs_split_on_blank_lines() {
        let doc = parse_text("Para one.\n\nPara two.\n\nPara three.");
        assert_eq!(doc.spans.len(), 3);
        assert_eq!(doc.spans[0].text(&doc), "Para one.");
        assert_eq!(doc.spans[1].text(&doc), "Para two.");
        assert_eq!(doc.spans[2].text(&doc), "Para three.");
        doc.assert_spans_contiguous();
    }

    #[test]
    fn paragraph_internal_single_newlines_are_preserved() {
        // Single newlines within a paragraph should NOT split - only blank
        // lines split. This preserves soft-wrapped text.
        let doc = parse_text("Line one\nLine two\nLine three");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "Line one\nLine two\nLine three");
    }

    #[test]
    fn paragraph_whitespace_is_trimmed() {
        let doc = parse_text("  \n  padded paragraph  \n  ");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "padded paragraph");
    }

    #[test]
    fn windows_style_crlf_boundaries_split_correctly() {
        // "\r\n\r\n" should also split paragraphs after trim normalizes it.
        let doc = parse_text("Para one.\r\n\r\nPara two.");
        assert_eq!(doc.spans.len(), 2);
        assert_eq!(doc.spans[0].text(&doc), "Para one.");
        assert_eq!(doc.spans[1].text(&doc), "Para two.");
        doc.assert_spans_contiguous();
    }
}
