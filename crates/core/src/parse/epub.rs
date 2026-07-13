//! EPUB parser: opens the `.epub` ZIP container, reads the OPF manifest to
//! determine spine (reading) order, then extracts each chapter's XHTML and
//! feeds it through the existing [`HtmlParser`].
//!
//! This reuses the HTML pipeline — no new parsing logic, just ZIP extraction
//! + OPF ordering. Each chapter's title becomes a heading-ancestry entry.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{BitVanesError, Result};
use crate::parse::zip_util::read_zip_entry;
use crate::parse::{Document, HtmlParser, Parser, TextSpan, offset_to_u32};
use crate::schema::{DocumentFormat, PipelineConfig};

/// Entry point: parses `.epub` bytes into a [`Document`].
pub fn parse_epub_bytes(bytes: &[u8], cfg: &PipelineConfig) -> Result<Document> {
    // 1. Find and parse the OPF file (container.xml → OPF path).
    let opf_path = find_opf_path(bytes)?;
    let opf_xml = read_zip_entry(bytes, &opf_path)?;

    // 2. Extract spine order + manifest hrefs + titles.
    let spine = parse_opf_spine(&opf_xml)?;

    // 3. For each spine item, read its XHTML and parse via HtmlParser.
    let html_cfg = PipelineConfig {
        format: DocumentFormat::Html,
        ..cfg.clone()
    };
    let html_parser = HtmlParser;

    let mut full_text = String::new();
    let mut spans: Vec<TextSpan> = Vec::new();
    let base_dir = opf_path_dir(&opf_path);

    for item in &spine {
        let chapter_path = if item.href.starts_with('/') {
            item.href.clone()
        } else {
            format!("{base_dir}{}", item.href)
        };

        let Ok(xhtml) = read_zip_entry(bytes, &chapter_path) else {
            continue;
        };

        let doc = html_parser.parse(&xhtml, &html_cfg)?;
        if doc.spans.is_empty() {
            continue;
        }

        // Merge chapter doc into the main document with a heading prefix.
        let heading = vec![item.title.clone()];
        let offset = full_text.len();
        full_text.push_str(&doc.full_text);

        for span in &doc.spans {
            let mut combined_heading = heading.clone();
            combined_heading.extend(span.heading_path.iter().cloned());
            spans.push(TextSpan {
                char_offset_start: offset_to_u32(offset + span.char_offset_start as usize),
                char_offset_end: offset_to_u32(offset + span.char_offset_end as usize),
                heading_path: combined_heading,
                section_kind: span.section_kind,
            });
        }
    }

    Ok(Document { full_text, spans })
}

struct SpineItem {
    href: String,
    title: String,
}

/// Reads `META-INF/container.xml` to find the OPF file path.
fn find_opf_path(bytes: &[u8]) -> Result<String> {
    let container = read_zip_entry(bytes, "META-INF/container.xml")?;
    let mut reader = Reader::from_str(&container);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                if local_name(e.name().as_ref()) == b"rootfile" {
                    for attr in e.attributes().flatten() {
                        if local_name(attr.key.as_ref()) == b"full-path" {
                            return Ok(String::from_utf8_lossy(attr.value.as_ref()).into_owned());
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(BitVanesError::InvalidInput(format!(
                    "container.xml parse error: {e}"
                )));
            }
            _ => {}
        }
        buf.clear();
    }
    Err(BitVanesError::InvalidInput(
        "container.xml: no <rootfile> element found".to_string(),
    ))
}

/// Parses the OPF `<manifest>` + `<spine>` to produce the reading-order list.
fn parse_opf_spine(opf_xml: &str) -> Result<Vec<SpineItem>> {
    let mut reader = Reader::from_str(opf_xml);
    let mut buf = Vec::new();

    // Collect manifest items: id → href.
    let mut manifest: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    // Collect metadata title.
    let mut title = String::from("Chapter#");
    // Spine idrefs in order.
    let mut spine_ids: Vec<String> = Vec::new();
    let mut in_metadata = false;
    let mut in_title = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                b"metadata" => in_metadata = true,
                b"title" if in_metadata => in_title = true,
                b"item" => extract_item(&e, &mut manifest),
                b"itemref" => extract_itemref(&e, &mut spine_ids),
                _ => {}
            },
            // Self-closing elements like <item .../> and <itemref .../>.
            Ok(Event::Empty(e)) => match local_name(e.name().as_ref()) {
                b"item" => extract_item(&e, &mut manifest),
                b"itemref" => extract_itemref(&e, &mut spine_ids),
                _ => {}
            },
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                b"metadata" => in_metadata = false,
                b"title" if in_metadata => in_title = false,
                _ => {}
            },
            Ok(Event::Text(e)) if in_title => {
                title = super::docx::decode_xml_entities_pub(&String::from_utf8_lossy(e.as_ref()))
                    .trim()
                    .to_string();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(BitVanesError::InvalidInput(format!("opf parse error: {e}"))),
            _ => {}
        }
        buf.clear();
    }

    let mut items = Vec::new();
    let mut chapter_num = 0;
    for (i, id) in spine_ids.iter().enumerate() {
        if let Some(href) = manifest.get(id) {
            chapter_num += 1;
            let item_title = if i == 0 && !title.is_empty() {
                title.clone()
            } else {
                format!("Chapter {chapter_num}")
            };
            items.push(SpineItem {
                href: href.clone(),
                title: item_title,
            });
        }
    }
    Ok(items)
}

/// Returns the directory component of an OPF path (for resolving relative hrefs).
fn opf_path_dir(opf_path: &str) -> String {
    match opf_path.rfind('/') {
        Some(i) => opf_path[..=i].to_string(),
        None => String::new(),
    }
}

/// Extracts `id` + `href` from a `<manifest><item>` element.
fn extract_item(
    e: &quick_xml::events::BytesStart<'_>,
    manifest: &mut std::collections::HashMap<String, String>,
) {
    let mut id = String::new();
    let mut href = String::new();
    for attr in e.attributes().flatten() {
        match local_name(attr.key.as_ref()) {
            b"id" => id = String::from_utf8_lossy(attr.value.as_ref()).into_owned(),
            b"href" => href = String::from_utf8_lossy(attr.value.as_ref()).into_owned(),
            _ => {}
        }
    }
    if !id.is_empty() && !href.is_empty() {
        manifest.insert(id, href);
    }
}

/// Extracts `idref` from a `<spine><itemref>` element.
fn extract_itemref(e: &quick_xml::events::BytesStart<'_>, spine_ids: &mut Vec<String>) {
    for attr in e.attributes().flatten() {
        if local_name(attr.key.as_ref()) == b"idref" {
            spine_ids.push(String::from_utf8_lossy(attr.value.as_ref()).into_owned());
        }
    }
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
    use std::io::Write;

    fn build_epub(opf: &str, chapters: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            // container.xml
            let container = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
            writer.start_file("META-INF/container.xml", opts).unwrap();
            writer.write_all(container.as_bytes()).unwrap();

            // OPF
            writer.start_file("OEBPS/content.opf", opts).unwrap();
            writer.write_all(opf.as_bytes()).unwrap();

            // Chapters
            for (path, body) in chapters {
                writer.start_file(path, opts).unwrap();
                writer.write_all(body.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn parses_two_chapter_epub() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="chapter2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#;
        let chapters = [
            (
                "OEBPS/chapter1.xhtml",
                "<html><body><p>First chapter text.</p></body></html>",
            ),
            (
                "OEBPS/chapter2.xhtml",
                "<html><body><p>Second chapter text.</p></body></html>",
            ),
        ];
        let epub = build_epub(opf, &chapters);
        let doc = parse_epub_bytes(
            &epub,
            &PipelineConfig {
                format: DocumentFormat::Epub,
                ..PipelineConfig::default()
            },
        )
        .unwrap();

        assert!(doc.full_text.contains("First chapter text."));
        assert!(doc.full_text.contains("Second chapter text."));
        assert!(doc.spans.len() >= 2);
        // First chapter heading should carry the book title.
        assert!(
            doc.spans[0]
                .heading_path
                .iter()
                .any(|h| h.contains("Test Book"))
        );
        doc.assert_spans_contiguous();
    }
}
