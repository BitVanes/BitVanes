//! PPTX (Microsoft `PowerPoint`) parser via ZIP + quick-xml.
//!
//! Opens the `.pptx` ZIP container, enumerates `ppt/slides/slideN.xml` files
//! in numeric order, and extracts text runs (`<a:t>`) from each slide. Each
//! slide becomes a [`TextSpan`] with `heading_path = ["Slide N"]`, so chunks
//! carry slide-level lineage for downstream retrieval.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{BitVanesError, Result};
use crate::parse::zip_util::{list_entries, read_zip_entry};
use crate::parse::{Document, TextSpan, offset_to_u32};
use crate::schema::{PipelineConfig, SectionKind};

/// Entry point: parses `.pptx` bytes into a [`Document`].
pub fn parse_pptx_bytes(bytes: &[u8], _cfg: &PipelineConfig) -> Result<Document> {
    let mut slide_names = list_entries(bytes, "ppt/slides/")?;
    // Sort by slide number (slide1.xml < slide2.xml < ... slide10.xml).
    slide_names.sort_by(|a, b| {
        let na = slide_number(a);
        let nb = slide_number(b);
        na.cmp(&nb)
    });
    slide_names.retain(|n| n.to_ascii_lowercase().ends_with(".xml"));

    let mut full_text = String::new();
    let mut spans: Vec<TextSpan> = Vec::new();

    for name in &slide_names {
        let xml = read_zip_entry(bytes, name)?;
        let xml = super::docx::encode_xml_entities_pub(&xml);
        let slide_num = slide_number(name).unwrap_or(0);
        let heading = format!("Slide {slide_num}");

        let text = extract_slide_text(&xml)?;
        if text.trim().is_empty() {
            continue;
        }

        let start = full_text.len();
        full_text.push_str(&text);
        full_text.push('\n');

        spans.push(TextSpan {
            char_offset_start: offset_to_u32(start),
            char_offset_end: offset_to_u32(full_text.len()),
            heading_path: vec![heading],
            section_kind: SectionKind::Paragraph,
        });
    }

    Ok(Document { full_text, spans })
}

/// Extracts the numeric suffix from a slide path like `ppt/slides/slide3.xml`.
fn slide_number(name: &str) -> Option<u32> {
    let base = name.rsplit('/').next()?;
    let num_part = base.strip_prefix("slide")?;
    let num_part = num_part.strip_suffix(".xml")?;
    num_part.parse().ok()
}

/// Walks a slide's XML and collects all `<a:t>` text content, joined by newlines.
fn extract_slide_text(xml: &str) -> Result<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut text = String::new();
    let mut in_t = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == b"t" => {
                in_t = true;
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
            }
            Ok(Event::End(e)) if local_name(e.name().as_ref()) == b"t" => in_t = false,
            Ok(Event::Text(e)) if in_t => {
                let decoded =
                    super::docx::decode_xml_entities_pub(&String::from_utf8_lossy(e.as_ref()));
                text.push_str(&decoded);
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(BitVanesError::InvalidInput(format!(
                    "pptx xml parse error: {e}"
                )));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(text)
}

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

#[cfg(test)]
#[allow(clippy::needless_raw_string_hashes)]
mod tests {
    use super::*;
    use crate::schema::DocumentFormat;
    use std::io::Write;

    fn build_pptx(slides: &[(usize, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (num, body) in slides {
                let path = format!("ppt/slides/slide{num}.xml");
                writer.start_file(&path, opts).unwrap();
                let xml = format!(
                    r#"<sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                      {body}
                    </sld>"#
                );
                writer.write_all(xml.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn parses_multiple_slides_in_order() {
        let pptx = build_pptx(&[
            (1, r#"<a:t>Welcome</a:t><a:t>Intro slide</a:t>"#),
            (2, r#"<a:t>Data results</a:t>"#),
            (10, r#"<a:t>Tenth slide</a:t>"#),
        ]);
        let doc = parse_pptx_bytes(
            &pptx,
            &PipelineConfig {
                format: DocumentFormat::Pptx,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        assert_eq!(doc.spans.len(), 3);
        assert!(doc.full_text.contains("Welcome"));
        assert!(doc.full_text.contains("Tenth slide"));
        // Slides sorted numerically: slide 1, 2, 10 (not lexicographic).
        assert_eq!(doc.spans[0].heading_path, vec!["Slide 1"]);
        assert_eq!(doc.spans[1].heading_path, vec!["Slide 2"]);
        assert_eq!(doc.spans[2].heading_path, vec!["Slide 10"]);
        doc.assert_spans_contiguous();
    }

    #[test]
    fn empty_slides_skipped() {
        let pptx = build_pptx(&[(1, r#"<a:t></a:t>"#), (2, r#"<a:t>Content</a:t>"#)]);
        let doc = parse_pptx_bytes(
            &pptx,
            &PipelineConfig {
                format: DocumentFormat::Pptx,
                ..PipelineConfig::default()
            },
        )
        .unwrap();
        assert_eq!(doc.spans.len(), 1);
    }
}
