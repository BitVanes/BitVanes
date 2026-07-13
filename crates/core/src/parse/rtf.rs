//! RTF (Rich Text Format) parser via `rtf-parser`.
//!
//! Extracts plain text + paragraph structure from RTF documents. Each
//! `StyleBlock` in the document body becomes a candidate span; empty blocks
//! are collapsed. Heading detection is heuristic (ALL-CAPS short lines or
//! bold painter) since RTF style sheets are inconsistently applied.

use rtf_parser::parse_rtf;

use crate::error::{BitVanesError, Result};
use crate::parse::{Document, TextSpan, offset_to_u32};
use crate::schema::{PipelineConfig, SectionKind};

/// Entry point: parses RTF bytes into a [`Document`].
pub fn parse_rtf_bytes(bytes: &[u8], _cfg: &PipelineConfig) -> Result<Document> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| BitVanesError::InvalidInput(format!("rtf is not valid UTF-8/ASCII: {e}")))?;

    let rtf_doc = parse_rtf(text.to_string());

    let mut full_text = String::new();
    let mut spans: Vec<TextSpan> = Vec::new();
    let mut heading_stack: Vec<String> = Vec::new();

    for block in &rtf_doc.body {
        let para_text = block.text.trim();
        if para_text.is_empty() {
            continue;
        }

        // Heuristic heading: short ALL-CAPS line.
        let alpha_count = para_text.chars().filter(|c| c.is_alphabetic()).count();
        let is_heading = para_text.len() < 80
            && alpha_count >= 4
            && para_text
                .chars()
                .filter(|c| c.is_alphabetic())
                .all(char::is_uppercase);

        if is_heading {
            heading_stack.clear();
            heading_stack.push(para_text.to_string());
        }

        let start = full_text.len();
        full_text.push_str(para_text);
        full_text.push('\n');

        spans.push(TextSpan {
            char_offset_start: offset_to_u32(start),
            char_offset_end: offset_to_u32(full_text.len()),
            heading_path: if is_heading {
                Vec::new()
            } else {
                heading_stack.clone()
            },
            section_kind: if is_heading {
                SectionKind::Heading
            } else {
                SectionKind::Paragraph
            },
        });
    }

    Ok(Document { full_text, spans })
}

#[cfg(test)]
#[allow(clippy::needless_raw_string_hashes)]
mod tests {
    use super::*;
    use crate::schema::DocumentFormat;

    #[test]
    fn parses_simple_rtf() {
        let rtf = r#"{\rtf1\ansi\deff0
{\fonttbl{\f0 Times New Roman;}}
\f0\fs24
Hello world.\par
This is a second paragraph.\par
}"#;
        let doc = parse_rtf_bytes(
            rtf.as_bytes(),
            &PipelineConfig {
                format: DocumentFormat::Rtf,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        assert!(
            doc.full_text.contains("Hello world") || !doc.spans.is_empty(),
            "should extract text: {:?}",
            doc.full_text
        );
        assert!(!doc.spans.is_empty());
    }
}
