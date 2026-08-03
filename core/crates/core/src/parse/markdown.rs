//! Markdown parser via `pulldown-cmark`.
//!
//! Walks `CommonMark` + GitHub-Flavored Markdown events and produces a
//! [`Document`] whose [`TextSpan`]s preserve heading ancestry and section
//! classification. The parser strips all inline formatting (bold, italic,
//! links) down to plain text - the chunker and downstream pipeline
//! operate on plain text, not on raw Markdown.
//!
//! # Heading ancestry
//!
//! When the parser encounters a heading of level `N`, it truncates the
//! heading stack to depth `N-1` and pushes the heading's text. This is the
//! standard table-of-contents walk and produces the correct ancestry for
//! any subsequent content span:
//!
//! ```text
//! # Title           -> stack: ["Title"]
//! ## Architecture   -> stack: ["Title", "Architecture"]
//! ### Sub           -> stack: ["Title", "Architecture", "Sub"]
//! ## Other          -> stack: ["Title", "Other"]
//! ```
//!
//! Headings themselves do NOT emit spans - they carry no text value on
//! their own. Their text lives on in the `heading_path` of subsequent
//! content spans.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::error::Result;
use crate::parse::{Document, Parser as ParserTrait, TextSpan};
use crate::schema::{PipelineConfig, SectionKind};

/// Stateless CommonMark/GFM parser.
pub struct MarkdownParser;

impl ParserTrait for MarkdownParser {
    fn parse(&self, input: &str, _cfg: &PipelineConfig) -> Result<Document> {
        let mut state = WalkerState::new(input.len());
        // The parser is a local, not a field, to avoid the
        // `&mut self.parser.by_ref()` borrow-check conflict that arises when
        // handle_event also borrows `&mut self`.
        for event in Parser::new_ext(input, gfm_options()) {
            state.handle_event(event);
        }
        state.flush_block();
        Ok(Document {
            full_text: state.full_text,
            spans: state.spans,
        })
    }
}

/// Mutable accumulation state for the markdown walker. Holds the output
/// buffer, the span list, the heading stack, and the current-block
/// accumulator. All the bookkeeping that changes per-event lives here.
struct WalkerState {
    full_text: String,
    spans: Vec<TextSpan>,
    heading_stack: Vec<String>,
    /// Current block being accumulated, if any.
    block_kind: Option<SectionKind>,
    block_start: usize,
    /// Pending heading text accumulator. Set when inside a heading block.
    pending_heading_level: Option<HeadingLevel>,
    pending_heading_text: String,
    /// Whether we're currently inside a list item. List items absorb inner
    /// paragraph breaks so that a multi-paragraph item stays as a single
    /// span (separated by `\n\n`).
    in_list_item: bool,
}

impl WalkerState {
    fn new(capacity: usize) -> Self {
        Self {
            full_text: String::with_capacity(capacity),
            spans: Vec::new(),
            heading_stack: Vec::new(),
            block_kind: None,
            block_start: 0,
            pending_heading_level: None,
            pending_heading_text: String::new(),
            in_list_item: false,
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            // ----- Headings: accumulate text for heading_stack, emit no span -----
            Event::Start(Tag::Heading { level, .. }) => {
                self.flush_block();
                self.pending_heading_level = Some(level);
                self.pending_heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = self.pending_heading_level.take() {
                    let depth = heading_depth(level);
                    // Truncate stack to depth-1, then push the new heading.
                    self.heading_stack.truncate(depth.saturating_sub(1));
                    let heading = std::mem::take(&mut self.pending_heading_text);
                    let heading = heading.trim();
                    if !heading.is_empty() {
                        self.heading_stack.push(heading.to_owned());
                    }
                }
            }

            // ----- Paragraph -----
            Event::Start(Tag::Paragraph) => {
                if self.in_list_item {
                    // Multi-paragraph list item: separate with a blank line.
                    if self.block_kind.is_some() && !self.full_text.ends_with('\n') {
                        self.full_text.push_str("\n\n");
                    }
                } else {
                    self.start_block(SectionKind::Paragraph);
                }
            }
            Event::End(TagEnd::Paragraph) if !self.in_list_item => {
                self.flush_block();
            }

            // ----- Code blocks, table cells, and horizontal rules all
            // terminate the current block and flush it -----
            Event::Start(Tag::CodeBlock(_)) => {
                self.start_block(SectionKind::Code);
            }
            Event::Start(Tag::TableCell) => {
                self.start_block(SectionKind::TableCell);
            }
            Event::End(TagEnd::CodeBlock | TagEnd::TableCell) | Event::Rule => {
                self.flush_block();
            }

            // ----- List items -----
            Event::Start(Tag::Item) => {
                self.flush_block();
                self.in_list_item = true;
                self.start_block(SectionKind::ListItem);
            }
            Event::End(TagEnd::Item) => {
                self.flush_block();
                self.in_list_item = false;
            }

            // ----- Text accumulation (guarded on active block) -----
            Event::Text(text) if self.pending_heading_level.is_some() => {
                self.pending_heading_text.push_str(&text);
            }
            Event::Text(text) if self.block_kind.is_some() => {
                self.full_text.push_str(&text);
            }
            Event::Code(code) if self.block_kind.is_some() => {
                // Inline code: append verbatim to the enclosing block.
                self.full_text.push_str(&code);
            }
            Event::SoftBreak | Event::HardBreak if self.block_kind.is_some() => {
                self.full_text.push('\n');
            }

            // Everything else (list containers, blockquotes, table structure,
            // footnotes, raw HTML, task-list markers): no state change.
            _ => {}
        }
    }

    /// Begin accumulating a new block of `kind`. Flushes any in-progress
    /// block first to keep spans non-overlapping.
    fn start_block(&mut self, kind: SectionKind) {
        self.flush_block();
        self.block_kind = Some(kind);
        self.block_start = self.full_text.len();
    }

    /// Close the current block, if any, and push a span if non-empty.
    fn flush_block(&mut self) {
        if let Some(kind) = self.block_kind.take() {
            let end = self.full_text.len();
            if end > self.block_start {
                self.spans.push(TextSpan {
                    char_offset_start: super::offset_to_u32(self.block_start),
                    char_offset_end: super::offset_to_u32(end),
                    heading_path: self.heading_stack.clone(),
                    section_kind: kind,
                });
            }
        }
    }
}

/// Maps a [`HeadingLevel`] to its numeric depth (1-6).
fn heading_depth(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Returns the GitHub-Flavored Markdown option set.
fn gfm_options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::DocumentFormat;

    fn parse_md(input: &str) -> Document {
        let cfg = PipelineConfig {
            format: DocumentFormat::Markdown,
            ..PipelineConfig::default()
        };
        MarkdownParser
            .parse(input, &cfg)
            .expect("markdown parse should not fail")
    }

    #[test]
    fn empty_input_produces_empty_document() {
        let doc = parse_md("");
        assert!(doc.full_text.is_empty());
        assert!(doc.spans.is_empty());
    }

    #[test]
    fn whitespace_only_input_produces_empty_document() {
        let doc = parse_md("   \n\n  \t  \n\n");
        assert!(doc.spans.is_empty(), "got spans: {:?}", doc.spans);
    }

    #[test]
    fn single_paragraph_emits_one_span() {
        let doc = parse_md("Hello world.");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].section_kind, SectionKind::Paragraph);
        assert_eq!(doc.spans[0].text(&doc), "Hello world.");
        assert!(doc.spans[0].heading_path.is_empty());
        doc.assert_spans_contiguous();
    }

    #[test]
    fn multiple_paragraphs_emit_distinct_spans() {
        let doc = parse_md("First paragraph.\n\nSecond paragraph.\n\nThird.");
        assert_eq!(doc.spans.len(), 3);
        for span in &doc.spans {
            assert_eq!(span.section_kind, SectionKind::Paragraph);
        }
        assert_eq!(doc.spans[0].text(&doc), "First paragraph.");
        assert_eq!(doc.spans[1].text(&doc), "Second paragraph.");
        assert_eq!(doc.spans[2].text(&doc), "Third.");
        doc.assert_spans_contiguous();
    }

    #[test]
    fn heading_populates_heading_path_for_subsequent_content() {
        let doc = parse_md(
            "# Architecture\n\
             The system has three layers.\n\
             ## Storage\n\
             We use Apache Arrow.",
        );
        assert_eq!(doc.spans.len(), 2);

        // First paragraph: under H1 "Architecture".
        assert_eq!(doc.spans[0].text(&doc), "The system has three layers.");
        assert_eq!(doc.spans[0].heading_path, vec!["Architecture".to_string()]);

        // Second paragraph: under H2 "Storage" which is under H1.
        assert_eq!(doc.spans[1].text(&doc), "We use Apache Arrow.");
        assert_eq!(
            doc.spans[1].heading_path,
            vec!["Architecture".to_string(), "Storage".to_string()]
        );

        // Headings themselves do not emit spans.
        for span in &doc.spans {
            assert_ne!(span.section_kind, SectionKind::Heading);
        }
        doc.assert_spans_contiguous();
    }

    #[test]
    fn heading_stack_truncates_correctly_on_sibling_headings() {
        let doc = parse_md(
            "# H1\n\
             ## H2-a\n\
             body-a\n\
             ## H2-b\n\
             body-b\n\
             ### H3\n\
             body-c\n\
             ## H2-c\n\
             body-d",
        );

        let paths: Vec<&Vec<String>> = doc.spans.iter().map(|s| &s.heading_path).collect();
        assert_eq!(paths.len(), 4);
        assert_eq!(paths[0], &vec!["H1".to_string(), "H2-a".to_string()]);
        assert_eq!(paths[1], &vec!["H1".to_string(), "H2-b".to_string()]);
        assert_eq!(
            paths[2],
            &vec!["H1".to_string(), "H2-b".to_string(), "H3".to_string()]
        );
        assert_eq!(paths[3], &vec!["H1".to_string(), "H2-c".to_string()]);
    }

    #[test]
    fn code_block_emits_code_section_kind() {
        let doc = parse_md(
            "Intro paragraph.\n\
             \n\
             ```rust\n\
             fn main() {}\n\
             ```\n\
             \n\
             Outro paragraph.",
        );
        assert_eq!(doc.spans.len(), 3);
        assert_eq!(doc.spans[0].section_kind, SectionKind::Paragraph);
        assert_eq!(doc.spans[1].section_kind, SectionKind::Code);
        assert_eq!(doc.spans[2].section_kind, SectionKind::Paragraph);
        assert!(doc.spans[1].text(&doc).contains("fn main()"));
        doc.assert_spans_contiguous();
    }

    #[test]
    fn list_items_emit_list_item_section_kind() {
        let doc = parse_md(
            "- First item\n\
             - Second item\n\
             - Third item",
        );
        assert_eq!(doc.spans.len(), 3);
        for span in &doc.spans {
            assert_eq!(span.section_kind, SectionKind::ListItem);
        }
        assert_eq!(doc.spans[0].text(&doc), "First item");
        assert_eq!(doc.spans[2].text(&doc), "Third item");
        doc.assert_spans_contiguous();
    }

    #[test]
    fn inline_formatting_is_stripped_to_plain_text() {
        let doc = parse_md("This is **bold**, *italic*, and [a link](https://x.com).");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "This is bold, italic, and a link.");
    }

    #[test]
    fn empty_heading_is_not_pushed_to_stack() {
        let doc = parse_md("#\nBody under empty heading.");
        assert_eq!(doc.spans.len(), 1);
        // Empty heading ("#") should not pollute the stack.
        assert!(doc.spans[0].heading_path.is_empty());
    }

    #[test]
    fn horizontal_rule_separates_blocks() {
        let doc = parse_md("Para one.\n\n---\n\nPara two.");
        assert_eq!(doc.spans.len(), 2);
        assert_eq!(doc.spans[0].text(&doc), "Para one.");
        assert_eq!(doc.spans[1].text(&doc), "Para two.");
        // The rule itself emits no span.
        doc.assert_spans_contiguous();
    }
}
