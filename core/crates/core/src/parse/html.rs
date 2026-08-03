//! HTML parser via `scraper` (built on Servo's `html5ever`).
//!
//! Walks the DOM tree and produces a [`Document`] whose [`TextSpan`]s
//! preserve heading ancestry (`<h1>`-`<h6>`) and section classification
//! (`<p>` = paragraph, `<pre>` = code, `<li>` = list item, etc.).
//!
//! Inline elements (`<b>`, `<i>`, `<a>`, `<span>`) are stripped to plain
//! text — the chunker operates on plain text, not on HTML markup.
//!
//! # Algorithm
//!
//! 1. Parse the HTML into a DOM via `scraper::Html::parse_document`.
//! 2. Locate the `<body>` element (or use the root for fragments).
//! 3. Iterate over all descendant elements in document order.
//! 4. For each heading (`<h1>`-`<h6>`): update the heading stack.
//! 5. For each "leaf block" (`<p>`, `<pre>`, `<li>`, etc. that contains no
//!    further block-level children): collect its text and emit a span.
//! 6. Container elements (`<div>`, `<section>`, `<ul>`, etc.) are
//!    transparent — their children are processed individually.

use scraper::{ElementRef, Html, Selector};

use crate::error::Result;
use crate::parse::{Document, Parser as ParserTrait, TextSpan, offset_to_u32};
use crate::schema::{PipelineConfig, SectionKind};

/// Stateless HTML parser.
pub struct HtmlParser;

impl ParserTrait for HtmlParser {
    fn parse(&self, input: &str, _cfg: &PipelineConfig) -> Result<Document> {
        let document = Html::parse_document(input);
        let body = select_body(&document);

        let mut full_text = String::with_capacity(input.len());
        let mut spans = Vec::new();
        let mut heading_stack: Vec<String> = Vec::new();

        for node in body.descendants() {
            let Some(element) = ElementRef::wrap(node) else {
                continue;
            };
            let tag = element.value().name();

            if let Some(level) = heading_level(tag) {
                let text = collect_text(&element);
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    heading_stack.truncate(level.saturating_sub(1));
                    heading_stack.push(trimmed.to_string());
                }
            } else if let Some(kind) = block_kind(tag) {
                // Only process "leaf" blocks — those without further block
                // children — to prevent double-counting text.
                if !has_block_children(&element) {
                    let text = collect_text(&element);
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        let start = full_text.len();
                        full_text.push_str(trimmed);
                        let end = full_text.len();
                        spans.push(TextSpan {
                            char_offset_start: offset_to_u32(start),
                            char_offset_end: offset_to_u32(end),
                            heading_path: heading_stack.clone(),
                            section_kind: kind,
                        });
                    }
                }
            }
        }

        Ok(Document { full_text, spans })
    }
}

fn select_body(document: &Html) -> ElementRef<'_> {
    match Selector::parse("body") {
        Ok(selector) => document
            .select(&selector)
            .next()
            .unwrap_or_else(|| document.root_element()),
        Err(_) => document.root_element(),
    }
}

fn collect_text(element: &ElementRef) -> String {
    element.text().collect()
}

fn heading_level(tag: &str) -> Option<usize> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn block_kind(tag: &str) -> Option<SectionKind> {
    match tag {
        "p" | "dd" | "dt" => Some(SectionKind::Paragraph),
        "pre" => Some(SectionKind::Code),
        "li" => Some(SectionKind::ListItem),
        "blockquote" => Some(SectionKind::BlockQuote),
        "td" | "th" => Some(SectionKind::TableCell),
        _ => None,
    }
}

fn has_block_children(element: &ElementRef) -> bool {
    element
        .children()
        .filter_map(ElementRef::wrap)
        .any(|child| {
            let tag = child.value().name();
            block_kind(tag).is_some() || heading_level(tag).is_some()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::Parser;
    use crate::schema::DocumentFormat;

    fn parse_html(input: &str) -> Document {
        let cfg = PipelineConfig {
            format: DocumentFormat::Html,
            ..PipelineConfig::default()
        };
        HtmlParser.parse(input, &cfg).expect("html parse")
    }

    #[test]
    fn empty_html_produces_empty_document() {
        let doc = parse_html("");
        assert!(doc.spans.is_empty());
    }

    #[test]
    fn simple_paragraph_emits_one_span() {
        let doc = parse_html("<p>Hello world.</p>");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "Hello world.");
        assert_eq!(doc.spans[0].section_kind, SectionKind::Paragraph);
        doc.assert_spans_contiguous();
    }

    #[test]
    fn multiple_paragraphs_emit_distinct_spans() {
        let doc = parse_html("<div><p>First.</p><p>Second.</p><p>Third.</p></div>");
        assert_eq!(doc.spans.len(), 3);
        assert_eq!(doc.spans[0].text(&doc), "First.");
        assert_eq!(doc.spans[1].text(&doc), "Second.");
        assert_eq!(doc.spans[2].text(&doc), "Third.");
        doc.assert_spans_contiguous();
    }

    #[test]
    fn headings_populate_heading_path() {
        let doc = parse_html(
            "<article>\
             <h1>Architecture</h1>\
             <p>The system has layers.</p>\
             <h2>Storage</h2>\
             <p>We use Arrow.</p>\
             </article>",
        );
        assert_eq!(doc.spans.len(), 2);
        assert_eq!(doc.spans[0].text(&doc), "The system has layers.");
        assert_eq!(doc.spans[0].heading_path, vec!["Architecture".to_string()]);
        assert_eq!(doc.spans[1].text(&doc), "We use Arrow.");
        assert_eq!(
            doc.spans[1].heading_path,
            vec!["Architecture".to_string(), "Storage".to_string()]
        );
    }

    #[test]
    fn inline_formatting_is_stripped() {
        let doc = parse_html("<p>This is <b>bold</b> and <a href='#'>linked</a> text.</p>");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "This is bold and linked text.");
    }

    #[test]
    fn pre_block_emits_code_section_kind() {
        let doc = parse_html("<pre>fn main() {}</pre>");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].section_kind, SectionKind::Code);
        assert_eq!(doc.spans[0].text(&doc), "fn main() {}");
    }

    #[test]
    fn list_items_emit_list_item_spans() {
        let doc = parse_html("<ul><li>One</li><li>Two</li><li>Three</li></ul>");
        assert_eq!(doc.spans.len(), 3);
        for span in &doc.spans {
            assert_eq!(span.section_kind, SectionKind::ListItem);
        }
        assert_eq!(doc.spans[0].text(&doc), "One");
        assert_eq!(doc.spans[2].text(&doc), "Three");
        doc.assert_spans_contiguous();
    }

    #[test]
    fn table_cells_emit_table_cell_spans() {
        let doc = parse_html(
            "<table><tr><td>A1</td><td>B1</td></tr><tr><td>A2</td><td>B2</td></tr></table>",
        );
        assert_eq!(doc.spans.len(), 4);
        for span in &doc.spans {
            assert_eq!(span.section_kind, SectionKind::TableCell);
        }
        doc.assert_spans_contiguous();
    }

    #[test]
    fn script_and_style_are_skipped() {
        let doc = parse_html(
            "<p>Visible text.</p>\
             <script>var x = 'hidden';</script>\
             <style>.hidden { display: none; }</style>",
        );
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "Visible text.");
    }

    #[test]
    fn full_html_document_uses_body_element() {
        let doc = parse_html(
            "<!DOCTYPE html>\
             <html><head><title>Page Title</title></head>\
             <body><p>Body content.</p></body></html>",
        );
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "Body content.");
    }
}
