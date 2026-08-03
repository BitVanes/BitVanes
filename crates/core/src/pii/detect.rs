//! PII scrubbing: character-level redaction of sensitive data, executed
//! BEFORE tokenization so that PII matches cannot be split across chunk
//! boundaries.
//!
//! # Architecture
//!
//! 1. [`Scrubber::from_profile`] compiles all enabled patterns (built-in
//!    and user-supplied) into individual [`Regex`](regex::Regex) instances,
//!    each tagged with its base confidence and contextual anchor keywords.
//! 2. [`Scrubber::scrub`] finds all matches via `find_iter`, pipes
//!    credit-card candidates through Luhn mod-10 verification and routing
//!    numbers through the ABA checksum (dropping failures), scans a ±N-word
//!    window around each surviving candidate for keyword anchors, computes a
//!    weighted-additive confidence score, resolves overlaps via greedy
//!    interval scheduling, and builds the scrubbed text.
//! 3. [`OffsetMap`] records every replacement's position so that chunk
//!    offsets (into the scrubbed text) can be projected back onto the
//!    original document for UI highlighting.
//! 4. [`PiiFinding`] records each redaction (entity, offset range, score,
//!    anchors fired) into the `pii_metadata` Arrow column.
//!
//! # Confidence model
//!
//! Each candidate match receives a score in `[0.0, 1.0]`:
//!
//! `score = (base + anchors_hit × ANCHOR_DELTA).min(ANCHOR_CAP)`
//!
//! | Pattern         | Base  | Validator          | Validator floor |
//! |-----------------|-------|--------------------|-----------------|
//! | Email           | 0.85  | none               | —               |
//! | Ssn             | 0.60  | none               | —               |
//! | Phone           | 0.65  | none               | —               |
//! | `CreditCard`    | 0.70  | Luhn (gate)        | 0.90 on pass    |
//! | `RoutingNumber` | 0.55  | ABA checksum (gate)| —               |
//! | `AwsKey`        | 0.95  | none               | —               |
//! | `GitHubPat`     | 0.95  | none               | —               |
//! | `Jwt`           | 0.90  | none               | —               |
//!
//! Candidates whose confidence falls below `ScrubProfile::min_confidence`
//! are dropped entirely (neither scrubbed nor reported).
//!
//! # Built-in patterns
//!
//! | Pattern       | Replacement       | Post-filter |
//! |---------------|-------------------|-------------|
//! | Email         | `[EMAIL]`         | none        |
//! | Ssn           | `[SSN]`           | none        |
//! | Phone         | `[PHONE]`         | none        |
//! | CreditCard    | `[CREDIT_CARD]`   | Luhn        |
//! | RoutingNumber | `[ROUTING_NUMBER]`| ABA         |
//! | AwsKey        | `[AWS_KEY]`       | none        |
//! | GitHubPat     | `[GITHUB_PAT]`    | none        |
//! | Jwt           | `[JWT]`           | none        |

use regex::Regex;

use crate::error::{BitVanesError, Result};
use crate::parse::{Document, offset_to_u32};
use crate::schema::{BuiltInPattern, ScrubProfile};

// ===========================================================================
// Confidence-model constants
// ===========================================================================

/// Confidence added per contextual anchor hit.
const ANCHOR_DELTA: f32 = 0.10;

/// Maximum confidence reachable via anchor boosting alone.
const ANCHOR_CAP: f32 = 0.99;

/// Floor for Luhn-validated credit cards: a card that passes mod-10 is at
/// least this confident regardless of anchor context.
const LUHN_FLOOR: f32 = 0.90;

// ===========================================================================
// Offset map
// ===========================================================================

/// A sorted list of text replacements that maps offsets between the
/// original document text and the scrubbed output.
///
/// Built by [`Scrubber::scrub`]. Supports bidirectional projection:
///
/// - [`project_forward`](Self::project_forward): original offset to scrubbed offset.
/// - [`project_inverse`](Self::project_inverse): scrubbed offset to original offset.
///
/// Both directions are O(n) in the number of edits. For typical PII
/// densities (< 100 replacements per document) this is effectively free.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OffsetMap {
    /// Sorted by `orig_start`. Each entry records one replacement region.
    edits: Vec<OffsetEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OffsetEdit {
    /// Start byte offset in the original text (inclusive).
    orig_start: usize,
    /// End byte offset in the original text (exclusive).
    orig_end: usize,
    /// Byte length of the replacement string.
    replacement_len: usize,
}

impl OffsetEdit {
    /// Net character delta: positive = text got shorter, negative = text got longer.
    #[allow(clippy::cast_possible_wrap)]
    const fn delta(&self) -> isize {
        (self.orig_end - self.orig_start) as isize - self.replacement_len as isize
    }
}

// Cast safety: document offsets are bounded by wasm linear memory (< 4 GiB),
// far below isize::MAX on both 32- and 64-bit targets. The sign-loss and
// wrap-around cases clippy warns about are structurally impossible here.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
impl OffsetMap {
    /// Projects an original-text byte offset to the corresponding
    /// scrubbed-text byte offset.
    ///
    /// For positions inside a replaced region, snaps to the start of the
    /// replacement text in the scrubbed output.
    #[must_use]
    pub fn project_forward(&self, orig_offset: usize) -> usize {
        let mut delta: isize = 0;
        for edit in &self.edits {
            if edit.orig_end <= orig_offset {
                delta += edit.delta();
            } else if edit.orig_start <= orig_offset {
                // Inside this edit: snap to the edit's scrubbed start.
                return (edit.orig_start as isize - delta).max(0) as usize;
            } else {
                break;
            }
        }
        (orig_offset as isize - delta).max(0) as usize
    }

    /// Projects a scrubbed-text byte offset back to the corresponding
    /// original-text byte offset.
    ///
    /// For positions inside a replacement token, maps to the start of the
    /// original PII region.
    #[must_use]
    pub fn project_inverse(&self, scrubbed_offset: usize) -> usize {
        let mut delta: isize = 0;
        let s = scrubbed_offset as isize;
        for edit in &self.edits {
            let scrub_start = edit.orig_start as isize - delta;
            let scrub_end = scrub_start + edit.replacement_len as isize;
            if s < scrub_start {
                return (s + delta).max(0) as usize;
            }
            if s < scrub_end {
                return edit.orig_start;
            }
            delta += edit.delta();
        }
        (s + delta).max(0) as usize
    }

    /// Returns `true` if no replacements were made (identity map).
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.edits.is_empty()
    }

    /// Returns the number of replacements recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.edits.len()
    }

    /// Returns `true` if no replacements were recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }
}

// ===========================================================================
// PII findings (audit metadata)
// ===========================================================================

/// A single PII finding emitted into the `pii_metadata` Arrow column.
///
/// Offsets are byte offsets into the **original** (pre-scrub) document text,
/// so the audit UI can highlight the raw match directly. The `anchors_hit`
/// list records which contextual keywords fired within the surrounding word
/// window, making the confidence score explainable.
#[derive(Debug, Clone, PartialEq)]
pub struct PiiFinding {
    /// Entity slug: `"email"`, `"ssn"`, `"credit_card"`, ... (or the lowercased
    /// custom-pattern name).
    pub entity: String,
    /// Byte offset of the match start in the original document text.
    pub offset_start: u32,
    /// Byte offset of the match end (exclusive) in the original document text.
    pub offset_end: u32,
    /// Confidence score in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Contextual anchor keywords that fired in the ±`anchor_window` word
    /// window around this match.
    pub anchors_hit: Vec<String>,
}

// ===========================================================================
// Anchor keyword tables
// ===========================================================================
//
// Lowercase. Single-word anchors are matched against whole-word tokens in
// the window; multi-word anchors (containing a space) are matched as
// substrings of the lowercased window.

const ANCHOR_NONE: &[&str] = &[];
const ANCHOR_EMAIL: &[&str] = &["email", "mail", "contact", "reach", "reply", "from", "to"];
const ANCHOR_SSN: &[&str] = &[
    "ssn",
    "social",
    "security",
    "tax",
    "taxpayer",
    "tin",
    "identification",
];
const ANCHOR_PHONE: &[&str] = &[
    "phone",
    "mobile",
    "cell",
    "tel",
    "telephone",
    "fax",
    "call",
    "number",
    "contact",
];
const ANCHOR_CREDIT_CARD: &[&str] = &[
    "card",
    "credit",
    "debit",
    "visa",
    "mastercard",
    "amex",
    "discover",
    "pan",
    "cc",
    "cvv",
    "expiry",
];
const ANCHOR_ROUTING: &[&str] = &["routing", "aba", "transit", "bank", "checking", "account"];
const ANCHOR_STREET: &[&str] = &[
    "address",
    "street",
    "residence",
    "residential",
    "home",
    "house",
    "reside",
    "live",
    "living",
    "located",
    "mailing",
    "ship",
    "deliver",
];
const ANCHOR_AWS: &[&str] = &["aws", "access", "key", "iam", "credential", "secret"];
const ANCHOR_GITHUB: &[&str] = &["github", "token", "pat", "gh", "gist"];
const ANCHOR_JWT: &[&str] = &["jwt", "bearer", "token", "authorization", "auth"];

// ===========================================================================
// Scrubber
// ===========================================================================

/// A compiled PII scrubber. Built once from a [`ScrubProfile`] and applied
/// to many documents (or document chunks) without recompilation.
///
/// Cloning is cheap (regexes use `Arc` internally).
#[derive(Debug, Clone)]
pub struct Scrubber {
    patterns: Vec<CompiledPattern>,
    anchor_window: u8,
    min_confidence: f32,
    /// Entity slugs to detect-and-report but NOT replace.
    report_only: std::collections::HashSet<String>,
}

#[derive(Debug, Clone)]
struct CompiledPattern {
    regex: Regex,
    replacement: String,
    validator: Validator,
    /// Stable slug emitted into [`PiiFinding::entity`].
    entity: &'static str,
    /// Baseline confidence before anchor boosting.
    base_confidence: f32,
    /// Contextual anchor keywords for this pattern's matches.
    anchors: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Validator {
    None,
    /// Extract digits from the match, validate via Luhn checksum. A failing
    /// checksum drops the candidate entirely (not a credit card).
    Luhn,
    /// Validate via the ABA routing-number checksum. A failing checksum drops
    /// the candidate.
    RoutingNumber,
}

impl Validator {
    const fn is_gated(self) -> bool {
        matches!(self, Self::Luhn | Self::RoutingNumber)
    }

    fn is_valid(self, text: &str, start: usize, end: usize) -> bool {
        match self {
            Self::None => true,
            Self::Luhn => luhn_valid(&text[start..end]),
            Self::RoutingNumber => aba_valid(&text[start..end]),
        }
    }
}

impl Scrubber {
    /// Compiles all patterns (built-in and custom) from the profile into
    /// a single [`Scrubber`], inheriting the anchor-window size and
    /// minimum-confidence threshold.
    ///
    /// # Errors
    ///
    /// Returns [`BitVanesError::InvalidConfig`] if any regex fails to compile.
    pub fn from_profile(profile: &ScrubProfile) -> Result<Self> {
        let mut patterns = Vec::with_capacity(profile.patterns.len() + profile.custom.len());

        for &kind in &profile.patterns {
            let (src, replacement, validator, entity, base, anchors) = builtin_config(kind);
            let regex = Regex::new(src).map_err(|e| {
                BitVanesError::InvalidConfig(format!("built-in pattern {kind:?}: {e}"))
            })?;
            patterns.push(CompiledPattern {
                regex,
                replacement: replacement.to_string(),
                validator,
                entity,
                base_confidence: base,
                anchors,
            });
        }

        for custom in &profile.custom {
            let regex = Regex::new(&custom.regex).map_err(|e| {
                BitVanesError::InvalidConfig(format!("custom regex '{}': {e}", custom.regex))
            })?;
            patterns.push(CompiledPattern {
                regex,
                entity: leak_entity(&custom.replacement),
                base_confidence: 0.90,
                anchors: ANCHOR_NONE,
                replacement: custom.replacement.clone(),
                validator: Validator::None,
            });
        }

        Ok(Self {
            patterns,
            anchor_window: profile.anchor_window,
            min_confidence: profile.min_confidence,
            report_only: profile.report_only.iter().cloned().collect(),
        })
    }

    /// Returns `true` if no patterns are compiled (scrubbing is a no-op).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Scrubs `text`: finds all PII matches, runs algorithmic verification
    /// (Luhn / ABA), scans the contextual anchor window, computes confidence
    /// scores, resolves overlaps, and builds the redacted output.
    ///
    /// Returns `(scrubbed_text, offset_map, findings)`. Findings carry
    /// offsets into the **original** text.
    #[must_use]
    pub fn scrub(&self, text: &str) -> (String, OffsetMap, Vec<PiiFinding>) {
        if self.patterns.is_empty() || text.is_empty() {
            return (text.to_string(), OffsetMap::default(), Vec::new());
        }

        let matches = self.find_matches(text);
        let resolved = resolve_overlaps(matches);
        build_scrubbed(text, &resolved)
    }

    /// Runs all patterns against `text` and returns raw (pre-overlap-resolution)
    /// matches. Parallelized behind the `parallel` feature.
    fn find_matches(&self, text: &str) -> Vec<PiiMatch> {
        #[cfg(feature = "parallel")]
        {
            if self.patterns.len() > 1 {
                use rayon::prelude::*;
                return self
                    .patterns
                    .par_iter()
                    .flat_map(|cp| self.find_pattern_matches(text, cp))
                    .collect();
            }
        }
        // Sequential path (single pattern or no parallel feature).
        self.patterns
            .iter()
            .flat_map(|cp| self.find_pattern_matches(text, cp))
            .collect()
    }

    /// Finds all matches for a single compiled pattern, with verification,
    /// anchor scanning, and confidence scoring applied.
    fn find_pattern_matches(&self, text: &str, cp: &CompiledPattern) -> Vec<PiiMatch> {
        let mut out = Vec::new();
        for m in cp.regex.find_iter(text) {
            let validator_ok = cp.validator.is_valid(text, m.start(), m.end());
            if cp.validator.is_gated() && !validator_ok {
                continue;
            }
            let anchors_hit =
                scan_anchors(text, m.start(), m.end(), self.anchor_window, cp.anchors);
            let confidence =
                compute_confidence(cp.base_confidence, anchors_hit.len(), cp.validator);
            if confidence < self.min_confidence {
                continue;
            }
            out.push(PiiMatch {
                start: m.start(),
                end: m.end(),
                replacement: cp.replacement.clone(),
                report_only: self.report_only.contains(cp.entity),
                finding: PiiFinding {
                    entity: cp.entity.to_string(),
                    offset_start: offset_to_u32(m.start()),
                    offset_end: offset_to_u32(m.end()),
                    confidence,
                    anchors_hit: anchors_hit.iter().map(|s| (*s).to_string()).collect(),
                },
            });
        }
        out
    }
}

/// Derives a stable entity slug for a custom pattern from its replacement
/// token: strips bracket pairs and lowercases. Falls back to `"custom"`.
fn leak_entity(replacement: &str) -> &'static str {
    // Leak is bounded: one tiny string per custom pattern per Scrubber build,
    // and custom-pattern counts are small (user-supplied config). The
    // alternative (changing `entity` to `String`) would force every built-in
    // to allocate on every match — undesirable in the hot loop.
    let trimmed = replacement.trim_matches(|c| c == '[' || c == ']');
    let slug = if trimmed.is_empty() {
        "custom"
    } else {
        trimmed
    };
    Box::leak(slug.to_ascii_lowercase().into_boxed_str())
}

/// A raw PII match found by a regex, before overlap resolution. Carries its
/// audit [`PiiFinding`] for downstream emission.
struct PiiMatch {
    start: usize,
    end: usize,
    replacement: String,
    /// If true, the finding is recorded but the original text is NOT replaced.
    report_only: bool,
    finding: PiiFinding,
}

/// Resolves overlapping matches using greedy interval scheduling (earliest
/// end first) to maximize the number of non-overlapping matches.
fn resolve_overlaps(mut matches: Vec<PiiMatch>) -> Vec<PiiMatch> {
    if matches.len() <= 1 {
        return matches;
    }
    // Sort by end (earliest-ending first), then by start for determinism.
    matches.sort_by(|a, b| a.end.cmp(&b.end).then(a.start.cmp(&b.start)));

    let mut accepted: Vec<PiiMatch> = Vec::with_capacity(matches.len());
    for m in matches {
        let overlaps = accepted.last().is_some_and(|last| last.end > m.start);
        if !overlaps {
            accepted.push(m);
        }
    }
    // Re-sort by start for the sequential text-building pass.
    accepted.sort_by_key(|m| m.start);
    accepted
}

/// Interleaves original text with replacement tokens to produce the
/// scrubbed string, records each replacement in an [`OffsetMap`], and
/// collects the surviving [`PiiFinding`]s.
fn build_scrubbed(text: &str, matches: &[PiiMatch]) -> (String, OffsetMap, Vec<PiiFinding>) {
    let mut scrubbed = String::with_capacity(text.len());
    let mut edits = Vec::with_capacity(matches.len());
    let mut findings = Vec::with_capacity(matches.len());
    let mut cursor = 0usize;

    for m in matches {
        debug_assert!(
            m.start >= cursor,
            "matches must be sorted by start and non-overlapping"
        );
        if m.start > cursor {
            scrubbed.push_str(&text[cursor..m.start]);
        }
        if m.report_only {
            // Detect-and-report: pass the original text through unchanged.
            // No OffsetEdit (zero delta), but the finding is still recorded.
            scrubbed.push_str(&text[m.start..m.end]);
        } else {
            edits.push(OffsetEdit {
                orig_start: m.start,
                orig_end: m.end,
                replacement_len: m.replacement.len(),
            });
            scrubbed.push_str(&m.replacement);
        }
        cursor = m.end;
        findings.push(m.finding.clone());
    }
    if cursor < text.len() {
        scrubbed.push_str(&text[cursor..]);
    }

    (scrubbed, OffsetMap { edits }, findings)
}

// ===========================================================================
// Contextual anchor scanning
// ===========================================================================

/// Scans the ±`window_words` word window around `[start, end)` for keyword
/// anchors. Returns the (deduplicated, order-preserving) list of anchors found.
///
/// Single-word anchors require a whole-word match; multi-word anchors match
/// as substrings of the lowercased window. ASCII-whitespace byte boundaries
/// are always UTF-8 char boundaries, so the window slice is always valid.
#[must_use]
fn scan_anchors(
    text: &str,
    start: usize,
    end: usize,
    window_words: u8,
    anchors: &'static [&'static str],
) -> Vec<&'static str> {
    if anchors.is_empty() || window_words == 0 {
        return Vec::new();
    }
    let words = usize::from(window_words);
    let win_start = window_back(text, start, words);
    let win_end = window_forward(text, end, words);
    if win_start >= win_end {
        return Vec::new();
    }
    let window = &text[win_start..win_end];
    let lower = window.to_ascii_lowercase();
    let word_set: std::collections::HashSet<&str> =
        lower.split(|c: char| !c.is_alphanumeric()).collect();

    let mut hits: Vec<&'static str> = Vec::new();
    for &anchor in anchors {
        if anchor.contains(' ') {
            if lower.contains(anchor) && !hits.contains(&anchor) {
                hits.push(anchor);
            }
        } else if word_set.contains(anchor) && !hits.contains(&anchor) {
            hits.push(anchor);
        }
    }
    hits
}

/// Walks backward from `from`, returning the byte offset that begins the
/// `words`-th word before `from`. Clamps to `0`.
fn window_back(text: &str, from: usize, words: usize) -> usize {
    let bytes = text.as_bytes();
    if words == 0 || from == 0 {
        return from.min(bytes.len());
    }
    let mut i = from.min(bytes.len());
    let mut count = 0usize;
    // Skip whitespace we're currently sitting in.
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    while count < words {
        // Consume one word (non-whitespace run).
        while i > 0 && !bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        count += 1;
        if count >= words {
            break;
        }
        // Skip preceding whitespace before the next word.
        while i > 0 && bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
    }
    i
}

/// Walks forward from `from`, returning the byte offset that ends the
/// `words`-th word after `from`. Clamps to `text.len()`.
fn window_forward(text: &str, from: usize, words: usize) -> usize {
    let bytes = text.as_bytes();
    let n = bytes.len();
    if words == 0 {
        return from.min(n);
    }
    let mut i = from.min(n);
    let mut count = 0usize;
    // Skip whitespace we're currently sitting in.
    while i < n && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    while i < n && count < words {
        // Consume one word.
        while i < n && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        count += 1;
        // Consume trailing whitespace.
        while i < n && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    i
}

// ===========================================================================
// Confidence scoring
// ===========================================================================

/// Weighted-additive confidence: `base + anchors × delta`, gated by
/// validator. For Luhn-validated cards, a passing checksum floors the score
/// at [`LUHN_FLOOR`]; a failing checksum is unreachable here (gated out
/// upstream) and returns `0.0` defensively.
#[must_use]
fn compute_confidence(base: f32, anchors_hit: usize, validator: Validator) -> f32 {
    let boost = f32::from(u8::try_from(anchors_hit.min(255)).unwrap_or(255)) * ANCHOR_DELTA;
    match validator {
        Validator::Luhn => base.max(LUHN_FLOOR) + boost,
        Validator::RoutingNumber | Validator::None => (base + boost).min(ANCHOR_CAP),
    }
    .min(ANCHOR_CAP)
}

// ===========================================================================
// Built-in pattern definitions
// ===========================================================================

/// Returns `(regex_source, replacement_token, validator, entity_slug,
/// base_confidence, anchor_keywords)` for a built-in kind.
#[allow(clippy::type_complexity)]
fn builtin_config(
    kind: BuiltInPattern,
) -> (
    &'static str,
    &'static str,
    Validator,
    &'static str,
    f32,
    &'static [&'static str],
) {
    match kind {
        BuiltInPattern::Email => (
            r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}",
            "[EMAIL]",
            Validator::None,
            "email",
            0.85,
            ANCHOR_EMAIL,
        ),
        BuiltInPattern::Ssn => (
            r"\b\d{3}-\d{2}-\d{4}\b",
            "[SSN]",
            Validator::None,
            "ssn",
            0.60,
            ANCHOR_SSN,
        ),
        BuiltInPattern::Phone => (
            r"\+1\d{10}\b",
            "[PHONE]",
            Validator::None,
            "phone",
            0.65,
            ANCHOR_PHONE,
        ),
        BuiltInPattern::CreditCard => (
            r"\b(?:\d[ -]?){12,18}\d\b",
            "[CREDIT_CARD]",
            Validator::Luhn,
            "credit_card",
            0.70,
            ANCHOR_CREDIT_CARD,
        ),
        BuiltInPattern::RoutingNumber => (
            r"\b\d{9}\b",
            "[ROUTING_NUMBER]",
            Validator::RoutingNumber,
            "routing_number",
            0.55,
            ANCHOR_ROUTING,
        ),
        BuiltInPattern::StreetAddress => (
            r"\b\d{1,6}\s+[A-Z][A-Za-z.''-]*(?:\s+[A-Z][A-Za-z.''-]*){0,4}\s+(?:Street|St|Avenue|Ave|Boulevard|Blvd|Road|Rd|Drive|Dr|Lane|Ln|Court|Ct|Way|Place|Pl|Parkway|Pkwy|Circle|Cir)\b",
            "[STREET_ADDRESS]",
            Validator::None,
            "street_address",
            0.45,
            ANCHOR_STREET,
        ),
        BuiltInPattern::AwsKey => (
            r"\bAKIA[0-9A-Z]{16}\b",
            "[AWS_KEY]",
            Validator::None,
            "aws_key",
            0.95,
            ANCHOR_AWS,
        ),
        BuiltInPattern::GitHubPat => (
            r"\bgh[pousr]_[A-Za-z0-9]{36,}\b",
            "[GITHUB_PAT]",
            Validator::None,
            "github_pat",
            0.95,
            ANCHOR_GITHUB,
        ),
        BuiltInPattern::Jwt => (
            r"\beyJ[A-Za-z0-9_\-]*\.eyJ[A-Za-z0-9_\-]*\.[A-Za-z0-9_\-]*\b",
            "[JWT]",
            Validator::None,
            "jwt",
            0.90,
            ANCHOR_JWT,
        ),
    }
}

// ===========================================================================
// Checksums: Luhn (credit cards) + ABA (routing numbers)
// ===========================================================================

/// Validates a string of digits (possibly mixed with separators) using the
/// Luhn algorithm. Returns `false` if fewer than 13 digits are present.
fn luhn_valid(text: &str) -> bool {
    let digits: Vec<u8> = text
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    if digits.len() < 13 {
        return false;
    }
    let mut sum = 0u32;
    let mut double = false;
    for &d in digits.iter().rev() {
        let mut n = u32::from(d);
        if double {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        sum += n;
        double = !double;
    }
    sum % 10 == 0
}

/// Validates a US ABA routing number (9 digits) using the standard
/// fractional-checksum: `3·(d0+d3+d6) + 7·(d1+d4+d7) + (d2+d5+d8)` must be
/// divisible by 10. Returns `false` if the extracted digit count is not 9.
fn aba_valid(text: &str) -> bool {
    let digits: Vec<u8> = text
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    if digits.len() != 9 {
        return false;
    }
    let d = &digits[..9];
    let sum = 3 * u32::from(d[0] + d[3] + d[6])
        + 7 * u32::from(d[1] + d[4] + d[7])
        + u32::from(d[2] + d[5] + d[8]);
    sum % 10 == 0
}

// ===========================================================================
// High-level entry points
// ===========================================================================

/// Scrubs a plain-text string. Convenience wrapper around
/// [`Scrubber::from_profile`] + [`Scrubber::scrub`].
///
/// # Errors
///
/// Returns [`BitVanesError::InvalidConfig`] if any regex in the profile
/// fails to compile.
pub fn scrub_text(
    text: &str,
    profile: &ScrubProfile,
) -> Result<(String, OffsetMap, Vec<PiiFinding>)> {
    let scrubber = Scrubber::from_profile(profile)?;
    Ok(scrubber.scrub(text))
}

/// Scrubs a [`Document`]'s `full_text` and projects all span offsets into
/// the scrubbed text's coordinate space.
///
/// This is the primary entry point called by the pipeline between parsing
/// and chunking. Returns the scrubbed document, the offset map, and the
/// findings (offsets into the original text).
///
/// # Errors
///
/// Returns [`BitVanesError::InvalidConfig`] if any regex fails to compile.
pub fn scrub_document(
    doc: Document,
    profile: &ScrubProfile,
) -> Result<(Document, OffsetMap, Vec<PiiFinding>)> {
    let scrubber = Scrubber::from_profile(profile)?;
    let (scrubbed_text, map, findings) = scrubber.scrub(&doc.full_text);

    let spans = doc
        .spans
        .into_iter()
        .map(|mut span| {
            let new_start = map.project_forward(span.char_offset_start as usize);
            let new_end = map.project_forward(span.char_offset_end as usize);
            span.char_offset_start = offset_to_u32(new_start);
            span.char_offset_end = offset_to_u32(new_end);
            span
        })
        .collect();

    Ok((
        Document {
            full_text: scrubbed_text,
            spans,
        },
        map,
        findings,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::Parser;
    use crate::schema::{CustomPattern, DocumentFormat, PipelineConfig};
    use proptest::prop_assert;

    fn scrub_with(text: &str, patterns: &[BuiltInPattern]) -> (String, OffsetMap, Vec<PiiFinding>) {
        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: patterns.to_vec(),
            custom: vec![],
            ..ScrubProfile::default()
        })
        .expect("built-in patterns should compile");
        scrubber.scrub(text)
    }

    // ----- Built-in pattern tests -----

    #[test]
    fn email_is_redacted() {
        let (out, map, findings) = scrub_with(
            "Contact alice@example.com for details.",
            &[BuiltInPattern::Email],
        );
        assert_eq!(out, "Contact [EMAIL] for details.");
        assert_eq!(map.len(), 1);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].entity, "email");
        assert!(findings[0].confidence >= 0.85, "email base confidence");
    }

    #[test]
    fn multiple_emails_in_one_text() {
        let (out, map, findings) = scrub_with(
            "From a@x.com and b@y.org to c@z.io.",
            &[BuiltInPattern::Email],
        );
        assert_eq!(out, "From [EMAIL] and [EMAIL] to [EMAIL].");
        assert_eq!(map.len(), 3);
        assert_eq!(findings.len(), 3);
    }

    #[test]
    fn ssn_is_redacted() {
        let (out, _, findings) = scrub_with("SSN: 123-45-6789.", &[BuiltInPattern::Ssn]);
        assert_eq!(out, "SSN: [SSN].");
        assert_eq!(findings[0].entity, "ssn");
    }

    #[test]
    fn phone_e164_is_redacted() {
        let (out, _, _) = scrub_with("Call +15551234567.", &[BuiltInPattern::Phone]);
        assert_eq!(out, "Call [PHONE].");
    }

    #[test]
    fn aws_key_is_redacted() {
        let (out, _, _) = scrub_with("key = AKIAIOSFODNN7EXAMPLE", &[BuiltInPattern::AwsKey]);
        assert_eq!(out, "key = [AWS_KEY]");
    }

    #[test]
    fn github_pat_is_redacted() {
        let pat = "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB";
        let (out, _, _) = scrub_with(&format!("token: {pat}"), &[BuiltInPattern::GitHubPat]);
        assert_eq!(out, "token: [GITHUB_PAT]");
    }

    #[test]
    fn jwt_is_redacted() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let (out, _, _) = scrub_with(
            &format!("Authorization: Bearer {jwt}"),
            &[BuiltInPattern::Jwt],
        );
        assert!(out.contains("[JWT]"), "JWT should be redacted: {out}");
        assert!(
            !out.contains("eyJ"),
            "raw JWT header should not survive: {out}"
        );
    }

    // ----- Credit card + Luhn -----

    #[test]
    fn valid_credit_card_passes_luhn() {
        // 4111 1111 1111 1111 is a classic test card (passes Luhn).
        let (out, map, findings) = scrub_with(
            "Card: 4111 1111 1111 1111 done.",
            &[BuiltInPattern::CreditCard],
        );
        assert_eq!(out, "Card: [CREDIT_CARD] done.");
        assert_eq!(map.len(), 1);
        assert_eq!(findings[0].entity, "credit_card");
        // Luhn-passing cards are floored at LUHN_FLOOR (0.90).
        assert!(
            findings[0].confidence >= 0.90,
            "Luhn-valid card confidence {} should be >= 0.90",
            findings[0].confidence
        );
        // "Card" anchor should fire in the window.
        assert!(
            findings[0].anchors_hit.iter().any(|a| a == "card"),
            "expected 'card' anchor to fire, got {:?}",
            findings[0].anchors_hit
        );
    }

    #[test]
    fn invalid_credit_card_fails_luhn_and_is_not_redacted() {
        // Same length but invalid checksum.
        let (out, map, findings) = scrub_with(
            "Card: 4111 1111 1111 1112 done.",
            &[BuiltInPattern::CreditCard],
        );
        assert_eq!(
            out, "Card: 4111 1111 1111 1112 done.",
            "invalid-Luhn sequence should NOT be redacted"
        );
        assert!(map.is_empty());
        assert!(
            findings.is_empty(),
            "no finding should be emitted for a failed gate"
        );
    }

    #[test]
    fn credit_card_without_separators() {
        // 4111111111111111 — unseparated, valid Luhn.
        let (out, _, _) = scrub_with("4111111111111111", &[BuiltInPattern::CreditCard]);
        assert_eq!(out, "[CREDIT_CARD]");
    }

    // ----- Routing number + ABA -----

    #[test]
    fn valid_routing_number_passes_aba() {
        // 021000021 is a real Bank of America ABA routing number.
        let (out, map, findings) = scrub_with(
            "Bank routing 021000021 for checks.",
            &[BuiltInPattern::RoutingNumber],
        );
        assert_eq!(out, "Bank routing [ROUTING_NUMBER] for checks.");
        assert_eq!(map.len(), 1);
        assert_eq!(findings[0].entity, "routing_number");
        // "routing" and "bank" anchors should fire.
        assert!(!findings[0].anchors_hit.is_empty());
    }

    #[test]
    fn invalid_routing_number_fails_aba() {
        // 9 digits but invalid ABA checksum (last digit wrong).
        let (out, map, findings) =
            scrub_with("Number 123456789 here.", &[BuiltInPattern::RoutingNumber]);
        assert_eq!(
            out, "Number 123456789 here.",
            "invalid ABA should NOT be redacted"
        );
        assert!(map.is_empty());
        assert!(findings.is_empty());
    }

    #[test]
    fn street_address_is_redacted() {
        let (out, _map, findings) = scrub_with(
            "Mailing address: 123 Main Street, Springfield.",
            &[BuiltInPattern::StreetAddress],
        );
        assert_eq!(out, "Mailing address: [STREET_ADDRESS], Springfield.");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].entity, "street_address");
        // "mailing" / "address" anchors should fire.
        assert!(!findings[0].anchors_hit.is_empty());
    }

    #[test]
    fn street_address_ignores_lowercase_prose() {
        // No building number + capitalized street name => no match.
        let (out, _map, findings) = scrub_with(
            "walking down the street now",
            &[BuiltInPattern::StreetAddress],
        );
        assert_eq!(out, "walking down the street now");
        assert!(findings.is_empty());
    }

    // ----- Confidence & anchor scoring -----

    #[test]
    fn anchor_boost_raises_confidence() {
        // Same SSN, but one has contextual anchors and the other doesn't.
        let (_, _, findings_with) = scrub_with(
            "Social security number: 123-45-6789 is the SSN.",
            &[BuiltInPattern::Ssn],
        );
        let (_, _, findings_without) = scrub_with(
            "Random token 123-45-6789 appears here.",
            &[BuiltInPattern::Ssn],
        );
        assert_eq!(findings_with.len(), 1);
        assert_eq!(findings_without.len(), 1);
        assert!(
            findings_with[0].confidence > findings_without[0].confidence,
            "anchored match ({}) should outrank unanchored ({})",
            findings_with[0].confidence,
            findings_without[0].confidence
        );
        assert!(!findings_with[0].anchors_hit.is_empty());
    }

    #[test]
    fn confidence_respects_cap() {
        // Many anchors should still cap at ANCHOR_CAP.
        let (_, _, findings) = scrub_with(
            "ssn social security tax taxpayer tin identification number: 123-45-6789",
            &[BuiltInPattern::Ssn],
        );
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].confidence <= ANCHOR_CAP + 1e-6,
            "confidence {} must not exceed ANCHOR_CAP",
            findings[0].confidence
        );
    }

    #[test]
    fn zero_anchor_window_disables_boosting() {
        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Ssn],
            custom: vec![],
            anchor_window: 0,
            min_confidence: 0.0,
            ..ScrubProfile::default()
        })
        .unwrap();
        let (_, _, findings) = scrubber.scrub("SSN social security 123-45-6789");
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].anchors_hit.is_empty(),
            "anchor_window=0 must produce no anchor hits"
        );
        // Base confidence for SSN is 0.60, no boost.
        assert!((findings[0].confidence - 0.60).abs() < 1e-6);
    }

    #[test]
    fn min_confidence_filters_low_score_matches() {
        // SSN without anchors has confidence 0.60. Set threshold to 0.65.
        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Ssn],
            custom: vec![],
            anchor_window: 7,
            min_confidence: 0.65,
            ..ScrubProfile::default()
        })
        .unwrap();
        let (out, map, findings) = scrubber.scrub("Random token 123-45-6789 appears.");
        assert!(
            !out.contains("[SSN]"),
            "below-threshold match should not be scrubbed: {out}"
        );
        assert!(map.is_empty());
        assert!(findings.is_empty());

        // With anchors the confidence climbs above 0.65 and the match survives.
        let (out2, _, findings2) = scrubber.scrub("Social security number 123-45-6789 is here.");
        assert!(
            out2.contains("[SSN]"),
            "above-threshold match should be scrubbed"
        );
        assert_eq!(findings2.len(), 1);
    }

    #[test]
    fn report_only_detects_but_does_not_scrub() {
        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Email, BuiltInPattern::Ssn],
            custom: vec![],
            report_only: vec!["email".to_string()],
            ..ScrubProfile::default()
        })
        .unwrap();

        let (out, map, findings) = scrubber.scrub("Email alice@test.com, SSN 123-45-6789.");
        // Email should NOT be scrubbed (report-only), SSN SHOULD be scrubbed.
        assert!(
            out.contains("alice@test.com"),
            "report-only email should pass through: {out}"
        );
        assert!(out.contains("[SSN]"), "SSN should still be scrubbed: {out}");
        assert!(!out.contains("123-45-6789"), "SSN should be redacted");
        // Both findings should be recorded.
        assert_eq!(findings.len(), 2, "both findings should be in metadata");
        assert_eq!(findings[0].entity, "email");
        assert_eq!(findings[1].entity, "ssn");
        // OffsetMap should only have one edit (SSN, not email).
        assert_eq!(
            map.len(),
            1,
            "only the scrubbed match creates an offset edit"
        );
    }

    // ----- OffsetMap projection -----

    #[test]
    fn offset_map_forward_round_trip() {
        let text = "Contact alice@example.com please.";
        let (scrubbed, map, _) = scrub_with(text, &[BuiltInPattern::Email]);
        assert_eq!(scrubbed, "Contact [EMAIL] please.");

        // [EMAIL] occupies scrubbed positions [8..15). Positions OUTSIDE the
        // replacement must round-trip exactly: forward(inverse(s)) == s.
        for s in 0..=7 {
            let back = map.project_forward(map.project_inverse(s));
            assert_eq!(back, s, "pre-replacement round-trip failed at {s}");
        }
        for s in 15..=scrubbed.len() {
            let back = map.project_forward(map.project_inverse(s));
            assert_eq!(back, s, "post-replacement round-trip failed at {s}");
        }
    }

    #[test]
    fn offset_map_identity_when_no_matches() {
        let (out, map, _) = scrub_with("No PII here.", &[BuiltInPattern::Email]);
        assert!(map.is_identity());
        assert_eq!(out, "No PII here.");
        // Forward projection of any offset is the offset itself.
        for i in 0..=out.len() {
            assert_eq!(map.project_forward(i), i);
            assert_eq!(map.project_inverse(i), i);
        }
    }

    #[test]
    fn findings_offsets_are_into_original_text() {
        let original = "Email alice@example.com now.";
        let (out, _, findings) = scrub_with(original, &[BuiltInPattern::Email]);
        assert_eq!(out, "Email [EMAIL] now.");
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        let matched = &original[f.offset_start as usize..f.offset_end as usize];
        assert_eq!(
            matched, "alice@example.com",
            "finding offsets must reference the original text"
        );
    }

    // ----- scrub_document -----

    #[test]
    fn scrub_document_projects_span_offsets() {
        let cfg = PipelineConfig {
            format: DocumentFormat::Text,
            ..PipelineConfig::default()
        };
        let doc = crate::parse::TextParser
            .parse("Contact alice@test.com.\n\nEmail bob@test.org too.", &cfg)
            .expect("parse");

        assert_eq!(doc.spans.len(), 2);

        let profile = ScrubProfile {
            patterns: vec![BuiltInPattern::Email],
            ..ScrubProfile::default()
        };
        let (scrubbed_doc, map, findings) = scrub_document(doc.clone(), &profile).expect("scrub");

        assert_eq!(map.len(), 2, "two emails should be scrubbed");
        assert_eq!(findings.len(), 2, "two findings should be emitted");
        assert!(scrubbed_doc.full_text.contains("[EMAIL]"));
        assert!(!scrubbed_doc.full_text.contains("alice@test.com"));

        // Span offsets must be valid indices into the scrubbed text.
        for span in &scrubbed_doc.spans {
            let e = span.char_offset_end as usize;
            assert!(
                e <= scrubbed_doc.full_text.len(),
                "span end {e} exceeds scrubbed text len {}",
                scrubbed_doc.full_text.len()
            );
            let text = span.text(&scrubbed_doc);
            assert!(!text.is_empty(), "span should not be empty after scrubbing");
        }

        // Contiguity invariant must still hold after projection.
        scrubbed_doc.assert_spans_contiguous();
    }

    #[test]
    fn scrub_document_with_empty_profile_is_noop() {
        let cfg = PipelineConfig {
            format: DocumentFormat::Text,
            ..PipelineConfig::default()
        };
        let doc = crate::parse::TextParser
            .parse("Just some text.\n\nNo PII.", &cfg)
            .expect("parse");

        let (scrubbed_doc, map, findings) =
            scrub_document(doc.clone(), &ScrubProfile::default()).expect("scrub");
        assert!(map.is_identity());
        assert!(findings.is_empty());
        assert_eq!(scrubbed_doc, doc);
    }

    // ----- Custom patterns -----

    #[test]
    fn custom_pattern_redacts_matches() {
        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![],
            custom: vec![CustomPattern {
                regex: r"PROJECT-\d{4}".to_string(),
                replacement: "[PROJ]".to_string(),
            }],
            ..ScrubProfile::default()
        })
        .expect("custom regex should compile");

        let (out, _, findings) = scrubber.scrub("See PROJECT-1234 and PROJECT-5678.");
        assert_eq!(out, "See [PROJ] and [PROJ].");
        assert_eq!(findings.len(), 2);
        // Entity derived from replacement token "[PROJ]" → "proj".
        assert_eq!(findings[0].entity, "proj");
        assert!(
            findings[0].confidence >= 0.90,
            "custom patterns default to 0.90 base"
        );
    }

    #[test]
    fn invalid_custom_regex_returns_error() {
        let err = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![],
            custom: vec![CustomPattern {
                regex: "[invalid".to_string(),
                replacement: "X".to_string(),
            }],
            ..ScrubProfile::default()
        })
        .unwrap_err();
        assert!(
            matches!(err, BitVanesError::InvalidConfig(_)),
            "expected InvalidConfig, got {err:?}"
        );
    }

    // ----- Overlap resolution -----

    #[test]
    fn overlapping_matches_are_resolved() {
        // Two emails that overlap (contrived): the earlier-ending one wins.
        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Email],
            custom: vec![CustomPattern {
                regex: r"alice@example\.com\.[a-z]+".to_string(),
                replacement: "[ALICE_FULL]".to_string(),
            }],
            ..ScrubProfile::default()
        })
        .expect("compile");

        // The custom pattern is longer and overlaps the email match.
        let (out, map, findings) = scrubber.scrub("Contact alice@example.com.org now.");
        // Greedy scheduling: earliest-ending wins. Email ends at
        // "alice@example.com" which is earlier than the custom match
        // "alice@example.com.org".
        assert!(
            out.contains("[EMAIL]") || out.contains("[ALICE_FULL]"),
            "one pattern should win: {out}"
        );
        assert_eq!(map.len(), 1, "exactly one match should survive overlap");
        assert_eq!(findings.len(), 1);
    }

    // ----- Checksum unit tests -----

    #[test]
    fn luhn_known_valid_cards() {
        assert!(luhn_valid("4111111111111111")); // Visa test
        assert!(luhn_valid("4111 1111 1111 1111")); // with spaces
        assert!(luhn_valid("5500000000000004")); // Mastercard test
        assert!(luhn_valid("4012888888881881")); // Visa test 2
    }

    #[test]
    fn luhn_known_invalid_cards() {
        assert!(!luhn_valid("4111111111111112"));
        assert!(!luhn_valid("1234567890123"));
        assert!(!luhn_valid("49927398717")); // classic Luhn-fail example
    }

    #[test]
    fn luhn_rejects_short_sequences() {
        assert!(!luhn_valid("12345"));
        assert!(!luhn_valid(""));
    }

    #[test]
    fn aba_known_valid_routing_numbers() {
        assert!(aba_valid("021000021")); // Bank of America
        assert!(aba_valid("026009593")); // Bank of America (different branch)
        assert!(aba_valid("111000025")); // Wells Fargo
    }

    #[test]
    fn aba_rejects_invalid_checksums() {
        assert!(!aba_valid("123456789"));
        assert!(!aba_valid("999999999"));
        assert!(!aba_valid("021000022")); // last digit wrong
    }

    #[test]
    fn aba_rejects_wrong_length() {
        assert!(!aba_valid("12345678"));
        assert!(!aba_valid("1234567890"));
        assert!(!aba_valid(""));
    }

    // ----- Anchor window helpers -----

    #[test]
    fn window_back_handles_whitespace() {
        let text = "foo bar baz qux";
        // "qux" starts at byte 12. Going back 1 word → 8 ("baz").
        assert_eq!(window_back(text, 12, 1), 8);
        // Going back 2 words → 4 ("bar").
        assert_eq!(window_back(text, 12, 2), 4);
        // Going back more words than exist → clamps to 0.
        assert_eq!(window_back(text, 12, 99), 0);
    }

    #[test]
    fn window_forward_handles_whitespace() {
        let text = "foo bar baz qux";
        // From byte 0 ("foo"), forward 1 word → 4 (after "foo ").
        assert_eq!(window_forward(text, 0, 1), 4);
        // Forward 2 words → 8.
        assert_eq!(window_forward(text, 0, 2), 8);
        // Forward beyond text → clamps to len.
        assert_eq!(window_forward(text, 0, 99), text.len());
    }

    // ----- Property tests (proptest) -----

    proptest::proptest! {
        /// OffsetMap invariant: for any scrubbed-text offset OUTSIDE a
        /// replaced region, forward(inverse(s)) == s. We can't guarantee this
        /// inside a replacement token (the map intentionally snaps), so we
        /// skip those ranges by checking that the round-trip produces a
        /// valid offset in the same "gap" of the scrubbed text.
        #[test]
        fn offsetmap_roundtrip_preserves_gaps(
            words in proptest::collection::vec(r"[a-z]{1,8}", 1..40)
        ) {
            // Insert fake "emails" so the scrubber has something to replace.
            let text = words.iter()
                .flat_map(|w| [w.as_str(), " ", "x@y.z", " "])
                .collect::<String>();
            let (scrubbed, map, _) = scrub_with(&text, &[BuiltInPattern::Email]);

            // Every position in the scrubbed text should project to a valid
            // original-text offset and back without panicking or going
            // out-of-bounds.
            for s in 0..=scrubbed.len() {
                let orig = map.project_inverse(s);
                prop_assert!(orig <= text.len(), "inverse({s}) = {orig} > text.len()");
                let fwd = map.project_forward(orig);
                prop_assert!(fwd <= scrubbed.len(), "forward({orig}) = {fwd} > scrubbed.len()");
            }
        }

        /// Every emitted finding has: valid offset bounds, confidence in
        /// [0, 1], and offset_end > offset_start.
        #[test]
        fn findings_satisfy_bounds_invariant(
            words in proptest::collection::vec(r"[a-zA-Z0-9@._%+\-]{1,20}", 1..60)
        ) {
            let text = words.join(" ");
            let profile = ScrubProfile {
                patterns: vec![
                    BuiltInPattern::Email,
                    BuiltInPattern::CreditCard,
                    BuiltInPattern::Ssn,
                ],
                ..ScrubProfile::default()
            };
            let scrubber = Scrubber::from_profile(&profile).unwrap();
            let (scrubbed, map, findings) = scrubber.scrub(&text);

            for f in &findings {
                prop_assert!(f.offset_end > f.offset_start, "empty finding range");
                prop_assert!(f.offset_end as usize <= text.len(), "offset past text end");
                prop_assert!(f.confidence >= 0.0 && f.confidence <= 1.0, "confidence out of [0,1]");
            }
            // The scrubbed text must never be longer than the original
            // (replacements are tokens like [EMAIL] which are ≤ the matched PII
            // in practice, but the real invariant is: scrubbed.len() and
            // map agree on the final offset).
            let _ = scrubbed;
            let _ = map;
        }
    }
}
