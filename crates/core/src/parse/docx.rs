//! DOCX (Microsoft Word) parser via ZIP + quick-xml streaming.
//!
//! Opens the `.docx` ZIP container, reads `word/document.xml`, and walks
//! the Office Open XML event stream to extract paragraphs, headings, tables,
//! and list items into [`TextSpan`]s with the same structural contract as
//! the Markdown parser.
//!
//! # Structure mapping
//!
//! | OOXML element           | BitVanes output            |
//! |-------------------------|-----------------------------|
//! | `<w:p>`                 | one span (paragraph)        |
//! | `<w:pStyle val=HeadingN>` | heading ancestry push     |
//! | `<w:tbl>` / `<w:tc>`    | `SectionKind::TableCell`   |
//! | `<w:numPr>`             | `SectionKind::ListItem`    |
//! | `<w:t>`                 | appended text              |
//! | `<w:tab/>` / `<w:br/>`  | tab / newline              |

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{BitVanesError, Result};
use crate::parse::zip_util::read_zip_entry;
use crate::parse::{Document, TextSpan, offset_to_u32};
#[cfg(test)]
use crate::schema::DocumentFormat;
use crate::schema::{PipelineConfig, SectionKind};

/// Entry point: parses `.docx` bytes into a [`Document`].
pub fn parse_docx_bytes(bytes: &[u8], _cfg: &PipelineConfig) -> Result<Document> {
    let xml = read_zip_entry(bytes, "word/document.xml")?;
    // quick-xml's streaming Reader splits text at entity boundaries and drops
    // the resolved characters. Pre-encode entities as placeholders so they
    // survive the XML parse, then decode them in the extracted text.
    let xml = encode_xml_entities(&xml);
    parse_docx_xml(&xml)
}

/// Replaces XML entity references with control-delimited placeholders that
/// survive quick-xml's streaming parse (which would otherwise drop them).
pub(crate) fn encode_xml_entities_pub(xml: &str) -> String {
    encode_xml_entities(xml)
}

/// Restores the original characters from the placeholders.
pub(crate) fn decode_xml_entities_pub(text: &str) -> String {
    decode_xml_entities(text)
}

/// Replaces XML entity references with control-delimited placeholders that
/// survive quick-xml's streaming parse (which would otherwise drop them).
fn encode_xml_entities(xml: &str) -> String {
    xml.replace("&amp;", "\u{1}amp;")
        .replace("&lt;", "\u{1}lt;")
        .replace("&gt;", "\u{1}gt;")
        .replace("&quot;", "\u{1}quot;")
        .replace("&apos;", "\u{1}apos;")
}

/// Restores the original characters from the placeholders.
fn decode_xml_entities(text: &str) -> String {
    if !text.contains('\u{1}') {
        return text.to_string();
    }
    text.replace("\u{1}amp;", "&")
        .replace("\u{1}lt;", "<")
        .replace("\u{1}gt;", ">")
        .replace("\u{1}quot;", "\"")
        .replace("\u{1}apos;", "'")
}

/// Returns the local (namespace-stripped) part of an XML element name.
/// `b"w:p"` → `b"p"`, `b"p"` → `b"p"`.
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// Extracts the `w:val` attribute value from a start/empty element.
fn attr_val(
    attrs: &mut quick_xml::events::attributes::Attributes<'_>,
    key: &str,
) -> Option<String> {
    for attr in attrs.flatten() {
        if local_name(attr.key.as_ref()) == key.as_bytes() {
            return Some(String::from_utf8_lossy(attr.value.as_ref()).into_owned());
        }
    }
    None
}

#[allow(clippy::struct_excessive_bools)]
struct DocxState {
    full_text: String,
    spans: Vec<TextSpan>,
    heading_stack: Vec<String>,
    /// Pending paragraph text buffer.
    para_text: String,
    /// Heading level for the current paragraph (0 = not a heading).
    heading_level: u8,
    /// Is the current paragraph a list item?
    is_list: bool,
    /// Nesting depth of `<w:tbl>` elements.
    table_depth: u32,
    /// Are we inside `<w:pPr>` (paragraph properties)?
    in_ppr: bool,
    /// Are we inside `<w:numPr>`?
    in_numpr: bool,
    /// Are we currently inside a `<w:t>` text element?
    in_text: bool,
}

impl DocxState {
    fn finish_paragraph(&mut self) {
        let trimmed = self.para_text.trim();
        if trimmed.is_empty() {
            self.para_text.clear();
            self.heading_level = 0;
            self.is_list = false;
            return;
        }

        // Update heading stack.
        if self.heading_level > 0 {
            // Pop headings at same or deeper level.
            self.heading_stack
                .truncate((self.heading_level - 1) as usize);
            self.heading_stack
                .push(format!("Heading {}", self.heading_level));
        }

        let start = self.full_text.len();
        self.full_text.push_str(&self.para_text);
        self.full_text.push('\n');

        let section_kind = if self.table_depth > 0 {
            SectionKind::TableCell
        } else if self.is_list {
            SectionKind::ListItem
        } else if self.heading_level > 0 {
            SectionKind::Heading
        } else {
            SectionKind::Paragraph
        };

        self.spans.push(TextSpan {
            char_offset_start: offset_to_u32(start),
            char_offset_end: offset_to_u32(self.full_text.len()),
            heading_path: self.heading_stack.clone(),
            section_kind,
        });

        self.para_text.clear();
        self.heading_level = 0;
        self.is_list = false;
    }
}

fn parse_docx_xml(xml: &str) -> Result<Document> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut state = DocxState {
        full_text: String::with_capacity(xml.len() / 2),
        spans: Vec::new(),
        heading_stack: Vec::new(),
        para_text: String::new(),
        heading_level: 0,
        is_list: false,
        table_depth: 0,
        in_ppr: false,
        in_numpr: false,
        in_text: false,
    };

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                b"p" => {
                    state.para_text.clear();
                    state.heading_level = 0;
                    state.is_list = false;
                }
                b"pPr" => state.in_ppr = true,
                b"tbl" => state.table_depth += 1,
                b"numPr" => {
                    state.in_numpr = true;
                    state.is_list = true;
                }
                b"t" => state.in_text = true,
                _ => {}
            },
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                b"p" => state.finish_paragraph(),
                b"pPr" => state.in_ppr = false,
                b"tbl" => state.table_depth = state.table_depth.saturating_sub(1),
                b"numPr" => state.in_numpr = false,
                b"t" => state.in_text = false,
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let ln = local_name(qname.as_ref());
                if ln == b"tab" {
                    if state.in_text || !state.para_text.is_empty() {
                        state.para_text.push('\t');
                    }
                } else if ln == b"br" {
                    if state.in_text || !state.para_text.is_empty() {
                        state.para_text.push('\n');
                    }
                } else if state.in_ppr && ln == b"pStyle" {
                    if let Some(val) = attr_val(&mut e.attributes(), "val") {
                        // "Heading1".."Heading6" or "Title" or named styles.
                        let lower = val.to_ascii_lowercase();
                        if let Some(level_str) = lower.strip_prefix("heading") {
                            if let Ok(level) = level_str.parse::<u8>() {
                                if (1..=6).contains(&level) {
                                    state.heading_level = level;
                                }
                            }
                        } else if lower == "title" {
                            state.heading_level = 1;
                        }
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if state.in_text {
                    let text = decode_xml_entities(&String::from_utf8_lossy(e.as_ref()));
                    state.para_text.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(BitVanesError::InvalidInput(format!(
                    "docx xml parse error at byte {}: {e}",
                    reader.buffer_position()
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    // Guarantee at least one span even for empty docs (chunker expects non-empty
    // spans; an empty doc produces zero chunks which is fine).
    if state.full_text.is_empty() && state.spans.is_empty() {
        // No content — return empty document.
    }

    Ok(Document {
        full_text: state.full_text,
        spans: state.spans,
    })
}

#[cfg(test)]
#[allow(clippy::needless_raw_string_hashes)]
mod tests {
    use super::*;

    /// Builds a minimal valid .docx (ZIP) in memory from the given document.xml body.
    fn build_docx(body_xml: &str) -> Vec<u8> {
        use std::io::Write;
        let document_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {body_xml}
  </w:body>
</w:document>"#
        );

        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file("word/document.xml", options).unwrap();
            writer.write_all(document_xml.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn parses_simple_paragraphs() {
        let xml = r#"
            <w:p><w:r><w:t>First paragraph.</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second paragraph.</w:t></w:r></w:p>
        "#;
        let docx = build_docx(xml);
        let doc = parse_docx_bytes(
            &docx,
            &PipelineConfig {
                format: DocumentFormat::Docx,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        assert!(doc.full_text.contains("First paragraph."));
        assert!(doc.full_text.contains("Second paragraph."));
        assert_eq!(doc.spans.len(), 2);
        assert_eq!(doc.spans[0].section_kind, SectionKind::Paragraph);
        doc.assert_spans_contiguous();
    }

    #[test]
    fn parses_headings_and_body() {
        let xml = r#"
            <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>My Title</w:t></w:r></w:p>
            <w:p><w:r><w:t>Body text under title.</w:t></w:r></w:p>
        "#;
        let docx = build_docx(xml);
        let doc = parse_docx_bytes(
            &docx,
            &PipelineConfig {
                format: DocumentFormat::Docx,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        assert_eq!(doc.spans.len(), 2);
        assert_eq!(doc.spans[0].section_kind, SectionKind::Heading);
        assert!(!doc.spans[0].heading_path.is_empty());
        assert_eq!(doc.spans[1].section_kind, SectionKind::Paragraph);
        doc.assert_spans_contiguous();
    }

    #[test]
    fn parses_table_cells() {
        let xml = r#"
            <w:tbl>
              <w:tr>
                <w:tc><w:p><w:r><w:t>Cell A</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>Cell B</w:t></w:r></w:p></w:tc>
              </w:tr>
            </w:tbl>
        "#;
        let docx = build_docx(xml);
        let doc = parse_docx_bytes(
            &docx,
            &PipelineConfig {
                format: DocumentFormat::Docx,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        assert_eq!(doc.spans.len(), 2);
        assert!(
            doc.spans
                .iter()
                .all(|s| s.section_kind == SectionKind::TableCell)
        );
        doc.assert_spans_contiguous();
    }

    #[test]
    fn parses_list_items() {
        let xml = r#"
            <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>
              <w:r><w:t>First item</w:t></w:r></w:p>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>
              <w:r><w:t>Second item</w:t></w:r></w:p>
        "#;
        let docx = build_docx(xml);
        let doc = parse_docx_bytes(
            &docx,
            &PipelineConfig {
                format: DocumentFormat::Docx,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        assert_eq!(doc.spans.len(), 2);
        assert!(
            doc.spans
                .iter()
                .all(|s| s.section_kind == SectionKind::ListItem)
        );
    }

    #[test]
    fn empty_docx_produces_empty_document() {
        let xml = "";
        let docx = build_docx(xml);
        let doc = parse_docx_bytes(
            &docx,
            &PipelineConfig {
                format: DocumentFormat::Docx,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        assert!(doc.spans.is_empty());
    }

    #[test]
    fn invalid_zip_returns_error() {
        let result = parse_docx_bytes(
            b"not a zip",
            &PipelineConfig {
                format: DocumentFormat::Docx,
                ..PipelineConfig::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn xml_entities_are_unescaped() {
        let xml = r#"<w:p><w:r><w:t>A &amp; B &lt; C</w:t></w:r></w:p>"#;
        let docx = build_docx(xml);
        let doc = parse_docx_bytes(
            &docx,
            &PipelineConfig {
                format: DocumentFormat::Docx,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        eprintln!("DEBUG full_text={:?}", doc.full_text);
        eprintln!("DEBUG spans={:?}", doc.spans.len());
        assert!(
            doc.full_text.contains("A & B < C"),
            "text: {:?}",
            doc.full_text
        );
        assert!(!doc.full_text.contains("&amp;"));
    }
}
