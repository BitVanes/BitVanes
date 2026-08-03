//! JSON parser: extracts text from structured JSON for sanitization.
//!
//! For arrays, each element becomes a separate span. For objects, nested
//! keys form the heading path and string values become paragraph text.

use serde_json::Value;

use crate::error::Result;
use crate::parse::{Document, Parser as ParserTrait, TextSpan, offset_to_u32};
use crate::schema::{PipelineConfig, SectionKind};

pub struct JsonParser;

impl ParserTrait for JsonParser {
    fn parse(&self, input: &str, _cfg: &PipelineConfig) -> Result<Document> {
        let root: Value = serde_json::from_str(input)
            .map_err(|e| crate::error::BitVanesError::InvalidInput(format!("invalid JSON: {e}")))?;

        let mut full_text = String::new();
        let mut spans = Vec::new();

        match &root {
            Value::Array(arr) => {
                for (i, item) in arr.iter().enumerate() {
                    extract_json_text(
                        item,
                        &["item_".to_string() + &i.to_string()],
                        &mut full_text,
                        &mut spans,
                    );
                }
            }
            Value::Object(obj) => {
                extract_json_text(&root, &[], &mut full_text, &mut spans);
                let _ = obj; // suppress unused
            }
            _ => {
                let start = full_text.len();
                full_text.push_str(&root.to_string());
                let end = full_text.len();
                spans.push(TextSpan {
                    char_offset_start: offset_to_u32(start),
                    char_offset_end: offset_to_u32(end),
                    heading_path: vec![],
                    section_kind: SectionKind::Paragraph,
                });
            }
        }

        Ok(Document { full_text, spans })
    }
}

fn extract_json_text(
    value: &Value,
    heading_path: &[String],
    full_text: &mut String,
    spans: &mut Vec<TextSpan>,
) {
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                let start = full_text.len();
                full_text.push_str(trimmed);
                let end = full_text.len();
                spans.push(TextSpan {
                    char_offset_start: offset_to_u32(start),
                    char_offset_end: offset_to_u32(end),
                    heading_path: heading_path.to_vec(),
                    section_kind: SectionKind::Paragraph,
                });
            }
        }
        Value::Object(map) => {
            for (key, val) in map {
                let mut child_path = heading_path.to_vec();
                child_path.push(key.clone());
                extract_json_text(val, &child_path, full_text, spans);
            }
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                let mut child_path = heading_path.to_vec();
                child_path.push(format!("[{i}]"));
                extract_json_text(item, &child_path, full_text, spans);
            }
        }
        Value::Number(n) => {
            let s = n.to_string();
            let start = full_text.len();
            full_text.push_str(&s);
            let end = full_text.len();
            spans.push(TextSpan {
                char_offset_start: offset_to_u32(start),
                char_offset_end: offset_to_u32(end),
                heading_path: heading_path.to_vec(),
                section_kind: SectionKind::Paragraph,
            });
        }
        Value::Bool(b) => {
            let start = full_text.len();
            full_text.push_str(if *b { "true" } else { "false" });
            let end = full_text.len();
            spans.push(TextSpan {
                char_offset_start: offset_to_u32(start),
                char_offset_end: offset_to_u32(end),
                heading_path: heading_path.to_vec(),
                section_kind: SectionKind::Paragraph,
            });
        }
        Value::Null => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::Parser;
    use crate::schema::DocumentFormat;

    fn parse_json(input: &str) -> Document {
        let cfg = PipelineConfig {
            format: DocumentFormat::Json,
            ..PipelineConfig::default()
        };
        JsonParser.parse(input, &cfg).expect("json parse")
    }

    #[test]
    fn array_of_objects_produces_spans() {
        let doc = parse_json(
            r#"[
                {"title": "First", "body": "Hello world"},
                {"title": "Second", "body": "Goodbye"}
            ]"#,
        );
        assert!(doc.spans.len() >= 4);
        assert!(doc.full_text.contains("Hello world"));
        assert!(doc.full_text.contains("Goodbye"));
    }

    #[test]
    fn nested_keys_become_heading_path() {
        let doc = parse_json(r#"{"section": {"subsection": "content here"}}"#);
        let span = &doc.spans[0];
        assert_eq!(span.text(&doc), "content here");
        assert!(span.heading_path.contains(&"section".to_string()));
        assert!(span.heading_path.contains(&"subsection".to_string()));
    }

    #[test]
    fn primitive_values_become_text() {
        let doc = parse_json("42");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].text(&doc), "42");
    }
}
