//! Pure-Rust NER post-processing: turn a sequence of per-token BIO
//! predictions (each carrying a byte span into the source text) into the
//! [`NerFinding`] spans the wire contract emits.
//!
//! This module is intentionally **dependency-free and fully testable** — it
//! contains the data-leak-critical logic (offset alignment), so it is kept
//! isolated from the ONNX Runtime boundary. The inference path
//! ([`crate::run_inference`]) is responsible for tokenizing, running the
//! model, and producing word-level [`TokenPred`]s; this module only aggregates
//! them.
//!
//! # Contract
//!
//! [`aggregate`] expects **word-level** predictions (not raw subwords): the
//! caller merges a word's WordPiece subwords into one [`TokenPred`] carrying
//! the word's full byte span and the predicted label (standard NER
//! post-step). Each `TokenPred` span MUST be byte offsets into the same
//! `text` passed to [`aggregate`]; spans that are empty, out of range, or not
//! on a UTF-8 boundary are dropped defensively rather than redacting the
//! wrong bytes.
//!
//! # BIO scheme (ConLL-2003)
//!
//! `O` = outside any entity. `B-X` = beginning of an entity of type X.
//! `I-X` = inside an entity of type X. Entity types: `PER`, `ORG`, `LOC`,
//! `MISC`, mapped to the engine's entity slugs (`person_name`,
//! `organization`, `location`, `misc_entity`).

// Plumbed and fully unit-tested, but not yet called from `run_inference` —
// the `ort`/`tokenizers` inference boundary that produces `TokenPred`s lands
// next. Remove this allow once `run_inference` calls `aggregate`.
#![allow(dead_code)]
#![forbid(unsafe_code)]

use crate::NerFinding;

/// A ConLL entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entity {
    Per,
    Org,
    Loc,
    Misc,
}

impl Entity {
    /// The engine-side entity slug emitted in [`NerFinding::entity`]. These
    /// MUST match `pii::detect::ner_replacement` in `bitvanes-core`.
    #[must_use]
    pub(crate) const fn slug(self) -> &'static str {
        match self {
            Self::Per => "person_name",
            Self::Org => "organization",
            Self::Loc => "location",
            Self::Misc => "misc_entity",
        }
    }
}

/// A decoded BIO tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tag {
    /// `O` — outside any entity.
    Outside,
    /// `B-X` — beginning of an entity.
    Begin(Entity),
    /// `I-X` — inside an entity.
    Inside(Entity),
}

/// Parse a ConLL label string (`"O"`, `"B-PER"`, `"I-LOC"`, …) into a [`Tag`].
/// Unknown labels and malformed input map to [`Tag::Outside`] — fail-safe: an
/// unrecognized tag never starts a span that could redact the wrong bytes.
#[must_use]
pub(crate) fn parse_tag(label: &str) -> Tag {
    let (prefix, rest) = label.split_once('-').unwrap_or((label, ""));
    match prefix {
        "B" => Entity::from_slug(rest).map_or(Tag::Outside, Tag::Begin),
        "I" => Entity::from_slug(rest).map_or(Tag::Outside, Tag::Inside),
        _ => Tag::Outside, // "O", "", or anything unrecognized
    }
}

impl Entity {
    /// Parse the ConLL type suffix (`"PER"`, `"ORG"`, `"LOC"`, `"MISC"`).
    fn from_slug(s: &str) -> Option<Self> {
        match s {
            "PER" => Some(Self::Per),
            "ORG" => Some(Self::Org),
            "LOC" => Some(Self::Loc),
            "MISC" => Some(Self::Misc),
            _ => None,
        }
    }
}

/// A single word-level prediction: its BIO tag, its byte span in the source
/// text, and the model's confidence for the predicted label.
#[derive(Debug, Clone)]
pub(crate) struct TokenPred {
    pub tag: Tag,
    /// Byte offset of the word start in the source text.
    pub start: u32,
    /// Byte offset of the word end (exclusive) in the source text.
    pub end: u32,
    /// Model confidence in `[0.0, 1.0]` for this token's label.
    pub confidence: f32,
}

/// A span being accumulated across consecutive `B-X` / `I-X` tokens.
#[derive(Debug)]
struct OpenSpan {
    entity: Entity,
    start: u32,
    end: u32,
    /// Max token confidence across the span — the most conservative (highest)
    /// prediction dominates, matching standard NER span-scoring.
    confidence: f32,
}

impl OpenSpan {
    fn from(tok: &TokenPred, entity: Entity) -> Self {
        Self {
            entity,
            start: tok.start,
            end: tok.end,
            confidence: tok.confidence,
        }
    }

    fn extend(&mut self, tok: &TokenPred) {
        self.end = tok.end;
        if tok.confidence > self.confidence {
            self.confidence = tok.confidence;
        }
    }

    /// Finalize into a [`NerFinding`], validating offsets against `text`.
    /// Returns `None` if the span is empty, out of range, not on a UTF-8
    /// boundary, or below `min_confidence`.
    fn finalize(self, text: &str, min_confidence: f32) -> Option<NerFinding> {
        if self.confidence < min_confidence {
            return None;
        }
        let start = usize::try_from(self.start).ok()?;
        let end = usize::try_from(self.end).ok()?;
        if start >= end || end > text.len() {
            return None;
        }
        // Offsets are byte offsets; slicing requires char boundaries. A model
        // that emits mid-codepoint spans (shouldn't happen with a correct
        // tokenizer) is dropped rather than risking a panic or wrong-byte
        // redaction.
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return None;
        }
        Some(NerFinding {
            entity: self.entity.slug().to_string(),
            start: self.start,
            end: self.end,
            confidence: self.confidence,
        })
    }
}

/// Aggregate word-level BIO predictions into entity spans, returning the
/// [`NerFinding`]s to emit on the wire.
///
/// - A `B-X` token opens a span of type X; `I-X` extends it. A stray `I-X`
///   with no open span of the same type is treated as `B-X` (fail-safe: never
///   drop an entity the model found just because of a tagging glitch).
/// - Any token whose span is invalid (empty, out of range, non-boundary) is
///   skipped for offset purposes — see [`OpenSpan::finalize`].
/// - Spans below `min_confidence` are dropped.
///
/// Findings are returned in left-to-right order with non-overlapping,
/// monotonically increasing byte ranges — guaranteed by the BIO scheme.
pub(crate) fn aggregate(text: &str, tokens: &[TokenPred], min_confidence: f32) -> Vec<NerFinding> {
    let mut out = Vec::new();
    let mut open: Option<OpenSpan> = None;

    let flush = |open: &mut Option<OpenSpan>, out: &mut Vec<NerFinding>| {
        if let Some(s) = open.take() {
            if let Some(f) = s.finalize(text, min_confidence) {
                out.push(f);
            }
        }
    };

    for tok in tokens {
        match tok.tag {
            Tag::Outside => {
                flush(&mut open, &mut out);
            }
            Tag::Begin(entity) => {
                flush(&mut open, &mut out);
                open = Some(OpenSpan::from(tok, entity));
            }
            Tag::Inside(entity) => {
                match &mut open {
                    // Continuation of the current entity: extend it.
                    Some(s) if s.entity == entity => s.extend(tok),
                    // Different entity, or no open span: treat as a new begin
                    // (lenient — redact rather than miss).
                    _ => {
                        flush(&mut open, &mut out);
                        open = Some(OpenSpan::from(tok, entity));
                    }
                }
            }
        }
    }
    flush(&mut open, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a TokenPred with sensible defaults.
    fn tok(tag: Tag, start: u32, end: u32, conf: f32) -> TokenPred {
        TokenPred {
            tag,
            start,
            end,
            confidence: conf,
        }
    }

    fn per_at(text: &str, word: &str, conf: f32) -> TokenPred {
        let s = text.find(word).unwrap() as u32;
        tok(
            Tag::Begin(Entity::Per),
            s,
            s + u32::try_from(word.len()).unwrap(),
            conf,
        )
    }

    // ----- parse_tag -----

    #[test]
    fn parse_tag_handles_bio_scheme() {
        assert_eq!(parse_tag("O"), Tag::Outside);
        assert_eq!(parse_tag("B-PER"), Tag::Begin(Entity::Per));
        assert_eq!(parse_tag("I-ORG"), Tag::Inside(Entity::Org));
        assert_eq!(parse_tag("B-LOC"), Tag::Begin(Entity::Loc));
        assert_eq!(parse_tag("I-MISC"), Tag::Inside(Entity::Misc));
    }

    #[test]
    fn parse_tag_treats_unknown_as_outside_fail_safe() {
        // Unknown labels must NOT start a span (could redact wrong bytes).
        assert_eq!(parse_tag("B-FOO"), Tag::Outside);
        assert_eq!(parse_tag("X-PER"), Tag::Outside);
        assert_eq!(parse_tag(""), Tag::Outside);
        assert_eq!(parse_tag("garbage"), Tag::Outside);
    }

    #[test]
    fn entity_slugs_match_engine_contract() {
        // These MUST match pii::detect::ner_replacement in bitvanes-core, or
        // the engine will report-but-not-redact NER findings.
        assert_eq!(Entity::Per.slug(), "person_name");
        assert_eq!(Entity::Org.slug(), "organization");
        assert_eq!(Entity::Loc.slug(), "location");
        assert_eq!(Entity::Misc.slug(), "misc_entity");
    }

    // ----- aggregate: happy paths -----

    #[test]
    fn single_person_emits_one_span() {
        let text = "Reach Alice today.";
        let toks = vec![
            tok(Tag::Outside, 0, 5, 0.99), // "Reach"
            per_at(text, "Alice", 0.97),
            tok(Tag::Outside, 11, 17, 0.99), // "today."
        ];
        let f = aggregate(text, &toks, 0.50);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].entity, "person_name");
        let alice = text.find("Alice").unwrap() as u32;
        assert_eq!((f[0].start, f[0].end), (alice, alice + 5));
        assert!((f[0].confidence - 0.97).abs() < 1e-6);
    }

    #[test]
    fn multi_token_org_aggregates_into_one_span() {
        let text = "Bank of America Corp";
        // Word-level preds for "Bank" "of" "America" "Corp" all B/I-ORG.
        let b = text.find("Bank").unwrap() as u32;
        let o = text.find("of").unwrap() as u32;
        let a = text.find("America").unwrap() as u32;
        let c = text.find("Corp").unwrap() as u32;
        let toks = vec![
            tok(Tag::Begin(Entity::Org), b, o - 1, 0.90),
            tok(Tag::Inside(Entity::Org), o, a - 1, 0.85),
            tok(Tag::Inside(Entity::Org), a, c - 1, 0.92),
            tok(Tag::Inside(Entity::Org), c, c + 4, 0.88),
        ];
        let f = aggregate(text, &toks, 0.50);
        assert_eq!(f.len(), 1, "one org span");
        assert_eq!(f[0].entity, "organization");
        assert_eq!((f[0].start, f[0].end), (b, c + 4));
        // max confidence across tokens
        assert!((f[0].confidence - 0.92).abs() < 1e-6);
    }

    #[test]
    fn adjacent_different_entities_split_correctly() {
        // "Alice Google" → PER then ORG, back to back.
        let text = "Alice Google";
        let a = text.find("Alice").unwrap() as u32;
        let g = text.find("Google").unwrap() as u32;
        let toks = vec![
            tok(Tag::Begin(Entity::Per), a, g - 1, 0.95),
            tok(Tag::Begin(Entity::Org), g, g + 6, 0.93),
        ];
        let f = aggregate(text, &toks, 0.50);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].entity, "person_name");
        assert_eq!(f[0].end, g - 1); // ends before "Google"
        assert_eq!(f[1].entity, "organization");
        assert_eq!((f[1].start, f[1].end), (g, g + 6));
    }

    // ----- aggregate: edge cases / fail-safety -----

    #[test]
    fn stray_inside_without_begin_is_treated_as_begin() {
        // A tagging glitch (I-PER with no B-PER) must still redact.
        let text = "hello Alice";
        let a = text.find("Alice").unwrap() as u32;
        let toks = vec![tok(Tag::Inside(Entity::Per), a, a + 5, 0.9)];
        let f = aggregate(text, &toks, 0.50);
        assert_eq!(f.len(), 1, "stray I-X redacted as a new span");
        assert_eq!(f[0].entity, "person_name");
    }

    #[test]
    fn inside_different_entity_starts_new_span() {
        // "Alice" B-PER then "Bob" I-ORG (mismatched) → two spans.
        let text = "Alice Bob";
        let a = text.find("Alice").unwrap() as u32;
        let b = text.find("Bob").unwrap() as u32;
        let toks = vec![
            tok(Tag::Begin(Entity::Per), a, b - 1, 0.9),
            tok(Tag::Inside(Entity::Org), b, b + 3, 0.9),
        ];
        let f = aggregate(text, &toks, 0.50);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].entity, "person_name");
        assert_eq!(f[1].entity, "organization");
    }

    #[test]
    fn below_min_confidence_is_dropped() {
        let text = "Alice";
        let a = text.find("Alice").unwrap() as u32;
        let toks = vec![tok(Tag::Begin(Entity::Per), a, a + 5, 0.40)];
        let f = aggregate(text, &toks, 0.50);
        assert!(f.is_empty(), "low-confidence span must not be emitted");
    }

    #[test]
    fn empty_or_out_of_range_spans_are_dropped() {
        let text = "Alice";
        let toks = vec![
            // empty span (start == end)
            tok(Tag::Begin(Entity::Per), 3, 3, 0.99),
            // end before start
            tok(Tag::Begin(Entity::Per), 4, 2, 0.99),
            // out of range
            tok(Tag::Begin(Entity::Per), 100, 200, 0.99),
        ];
        let f = aggregate(text, &toks, 0.50);
        assert!(f.is_empty(), "invalid spans dropped, no panic");
    }

    #[test]
    fn invalid_spans_never_panic_on_slice() {
        // Regression: a mid-codepoint span must be dropped, not panic.
        let text = "héllo Alice"; // é occupies bytes 1-2 (2-byte UTF-8)
        let toks = vec![
            // start=2 is the CONTINUATION byte of é (mid-codepoint, not a
            // char boundary). start=1 would be the START of é (valid).
            tok(Tag::Begin(Entity::Per), 2, 5, 0.99),
        ];
        let f = aggregate(text, &toks, 0.50);
        assert!(f.is_empty(), "non-boundary span dropped, no panic");
    }

    #[test]
    fn utf8_entity_on_char_boundary_is_kept() {
        // A name after a multibyte char must be detected at the correct BYTE
        // offset.
        let text = "café Alice";
        let a = text.find("Alice").unwrap() as u32;
        let toks = vec![tok(Tag::Begin(Entity::Per), a, a + 5, 0.95)];
        let f = aggregate(text, &toks, 0.50);
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].start, f[0].end), (a, a + 5));
    }

    #[test]
    fn empty_input_is_noop() {
        assert!(aggregate("", &[], 0.50).is_empty());
        assert!(aggregate("text", &[], 0.50).is_empty());
        let all_outside = vec![tok(Tag::Outside, 0, 4, 0.99)];
        assert!(aggregate("text", &all_outside, 0.50).is_empty());
    }

    #[test]
    fn findings_are_ordered_and_non_overlapping() {
        // PER ... ORG ... LOC — output must be sorted by start, no overlap.
        let text = "Alice in Google USA";
        let a = text.find("Alice").unwrap() as u32;
        let g = text.find("Google").unwrap() as u32;
        let u = text.find("USA").unwrap() as u32;
        let toks = vec![
            tok(Tag::Begin(Entity::Per), a, a + 5, 0.9),
            tok(Tag::Begin(Entity::Org), g, g + 6, 0.9),
            tok(Tag::Begin(Entity::Loc), u, u + 3, 0.9),
        ];
        let f = aggregate(text, &toks, 0.50);
        assert_eq!(f.len(), 3);
        assert!(f[0].end <= f[1].start);
        assert!(f[1].end <= f[2].start);
    }
}
