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

/// A raw per-subword model output before word-level merging. The inference
/// boundary produces a slice of these; [`merge_subwords`] turns them into the
/// word-level [`TokenPred`]s that [`aggregate`] consumes.
#[derive(Debug, Clone)]
pub(crate) struct RawToken {
    /// Argmax label index into [`CONLL_LABELS`].
    pub label_id: usize,
    /// Softmax probability of the argmax label.
    pub confidence: f32,
    /// Byte span of this subword in the source text (from the tokenizer's
    /// offset mapping). `(0, 0)` for special tokens.
    pub offset: (u32, u32),
    /// The word this subword belongs to (`None` for special tokens like
    /// `[CLS]` / `[SEP]` / padding, which are dropped before merging).
    pub word_id: Option<u32>,
}

/// Label-index → ConLL string map. MUST match the bundled model's
/// `config.json` `id2label`. Current model: `Xenova/bert-base-NER`
/// @ `24c7e5ab`, whose output head orders MISC before PER/ORG/LOC — NOT the
/// textbook ConLL order. A mismatch here silently mislabels every finding
/// (verified the hard way via the runtime test). The inference boundary
/// argmaxes logits against this; a future model swap must update this (or
/// load `id2label` from config dynamically).
pub(crate) const CONLL_LABELS: [&str; 9] = [
    "O", "B-MISC", "I-MISC", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC",
    "I-LOC",
];

/// Map a model label id to a [`Tag`] via [`CONLL_LABELS`]. Out-of-range ids
/// become [`Tag::Outside`] (fail-safe).
#[must_use]
pub(crate) fn label_id_to_tag(id: usize) -> Tag {
    CONLL_LABELS
        .get(id)
        .copied()
        .map_or(Tag::Outside, parse_tag)
}

/// Merge subword predictions into word-level [`TokenPred`]s.
///
/// Standard BERT-NER post-step: group subwords by `word_id` (dropping special
/// tokens where `word_id` is `None`), take the **first subword's** label
/// (the canonical strategy), and compute the word's byte span as the union of
/// its subwords' offsets. Confidence is the first subword's confidence.
///
/// Returned in original word order.
pub(crate) fn merge_subwords(tokens: &[RawToken]) -> Vec<TokenPred> {
    // Walk in order; a change in word_id closes the current word group.
    let mut out = Vec::new();
    let mut cur_word: Option<(u32, Tag, f32, u32, u32)> = None; // (id, tag, conf, start, end)

    for tok in tokens {
        let Some(word_id) = tok.word_id else {
            // Special token — flush any open word (words don't span special
            // tokens) and skip.
            if let Some((_, tag, conf, start, end)) = cur_word.take() {
                out.push(TokenPred {
                    tag,
                    start,
                    end,
                    confidence: conf,
                });
            }
            continue;
        };

        match cur_word {
            Some((cw, _, _, _, _)) if cw == word_id => {
                // Same word: extend the byte span.
                if let Some((_, tag, conf, start, end)) = cur_word.as_mut() {
                    if tok.offset.0 < *start {
                        *start = tok.offset.0;
                    }
                    if tok.offset.1 > *end {
                        *end = tok.offset.1;
                    }
                    // first-subword label/confidence are kept (no update)
                    let _ = (tag, conf);
                }
            }
            _ => {
                // New word: flush previous, open a new one with this subword.
                if let Some((_, tag, conf, start, end)) = cur_word.take() {
                    out.push(TokenPred {
                        tag,
                        start,
                        end,
                        confidence: conf,
                    });
                }
                cur_word = Some((
                    word_id,
                    label_id_to_tag(tok.label_id),
                    tok.confidence,
                    tok.offset.0,
                    tok.offset.1,
                ));
            }
        }
    }
    if let Some((_, tag, conf, start, end)) = cur_word {
        out.push(TokenPred {
            tag,
            start,
            end,
            confidence: conf,
        });
    }
    out
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
pub(crate) fn aggregate(
    text: &str,
    tokens: &[TokenPred],
    min_confidence: f32,
) -> Vec<NerFinding> {
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

    // ----- label_id_to_tag + merge_subwords -----

    #[test]
    fn label_id_maps_to_conll_tags() {
        // Order MUST match CONLL_LABELS (the bundled Xenova/bert-base-NER
        // config.json id2label): O, B-MISC, I-MISC, B-PER, I-PER, B-ORG, ...
        assert_eq!(label_id_to_tag(0), Tag::Outside);
        assert_eq!(label_id_to_tag(1), Tag::Begin(Entity::Misc));
        assert_eq!(label_id_to_tag(2), Tag::Inside(Entity::Misc));
        assert_eq!(label_id_to_tag(3), Tag::Begin(Entity::Per));
        assert_eq!(label_id_to_tag(4), Tag::Inside(Entity::Per));
        assert_eq!(label_id_to_tag(5), Tag::Begin(Entity::Org));
        assert_eq!(label_id_to_tag(6), Tag::Inside(Entity::Org));
        assert_eq!(label_id_to_tag(7), Tag::Begin(Entity::Loc));
        assert_eq!(label_id_to_tag(8), Tag::Inside(Entity::Loc));
        assert_eq!(
            label_id_to_tag(99),
            Tag::Outside,
            "out-of-range is fail-safe"
        );
        assert_eq!(label_id_to_tag(usize::MAX), Tag::Outside);
    }

    fn raw(
        label_id: usize,
        conf: f32,
        off: (u32, u32),
        word: Option<u32>,
    ) -> RawToken {
        RawToken {
            label_id,
            confidence: conf,
            offset: off,
            word_id: word,
        }
    }

    #[test]
    fn merge_drops_special_tokens() {
        // [CLS] Alice [SEP] → only "Alice" survives as word 0. B-PER = id 3.
        let toks = vec![
            raw(0, 0.99, (0, 0), None),    // [CLS]
            raw(3, 0.97, (0, 5), Some(0)), // Alice (B-PER)
            raw(0, 0.99, (0, 0), None),    // [SEP]
        ];
        let merged = merge_subwords(&toks);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].tag, Tag::Begin(Entity::Per));
        assert_eq!((merged[0].start, merged[0].end), (0, 5));
    }

    #[test]
    fn merge_unions_subword_offsets_and_keeps_first_label() {
        // "Smithsonian" → subwords "Smith" (B-ORG) + "##son" (I-ORG) + "##ian"
        // (I-ORG), same word id. First label (B-ORG) wins; span = union.
        // B-ORG = id 5, I-ORG = id 6 (see CONLL_LABELS).
        let toks = vec![
            raw(5, 0.90, (0, 5), Some(0)),  // "Smith"   B-ORG
            raw(6, 0.80, (5, 8), Some(0)),  // "##son"   I-ORG
            raw(6, 0.75, (8, 11), Some(0)), // "##ian"   I-ORG
        ];
        let merged = merge_subwords(&toks);
        assert_eq!(merged.len(), 1, "one word");
        assert_eq!(
            merged[0].tag,
            Tag::Begin(Entity::Org),
            "first-subword label"
        );
        assert_eq!(
            (merged[0].start, merged[0].end),
            (0, 11),
            "union of offsets"
        );
        assert!(
            (merged[0].confidence - 0.90).abs() < 1e-6,
            "first confidence"
        );
    }

    #[test]
    fn merge_splits_adjacent_words() {
        // "Alice Bob" → two words; each gets its own TokenPred.
        let toks = vec![
            raw(1, 0.95, (0, 5), Some(0)), // Alice
            raw(1, 0.93, (6, 9), Some(1)), // Bob
        ];
        let merged = merge_subwords(&toks);
        assert_eq!(merged.len(), 2);
        assert_eq!((merged[0].start, merged[0].end), (0, 5));
        assert_eq!((merged[1].start, merged[1].end), (6, 9));
    }

    #[test]
    fn merge_handles_trailing_word_without_special_token() {
        // No [SEP] at the end — the open word must still be flushed.
        let toks = vec![raw(1, 0.9, (0, 5), Some(0))];
        let merged = merge_subwords(&toks);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn merge_then_aggregate_end_to_end_pure() {
        // Full pure-Rust path: raw subwords -> merged words -> findings,
        // exercised without any model. Label ids match CONLL_LABELS:
        // 0=O, 3=B-PER, 5=B-ORG.
        let text = "Mary acme";
        let m = text.find("Mary").unwrap() as u32;
        let a = text.find("acme").unwrap() as u32;
        let raws = vec![
            raw(0, 0.99, (0, 0), None),        // [CLS]
            raw(3, 0.96, (m, m + 4), Some(0)), // Mary B-PER
            raw(5, 0.91, (a, a + 4), Some(1)), // acme B-ORG
            raw(0, 0.99, (0, 0), None),        // [SEP]
        ];
        let merged = merge_subwords(&raws);
        let findings = aggregate(text, &merged, 0.50);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].entity, "person_name");
        assert_eq!((findings[0].start, findings[0].end), (m, m + 4));
        assert_eq!(findings[1].entity, "organization");
        assert_eq!((findings[1].start, findings[1].end), (a, a + 4));
    }
}
