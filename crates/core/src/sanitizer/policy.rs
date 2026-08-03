//! Redaction output policies: how a detected PII span is rewritten into the
//! sanitized output.
//!
//! The [`pii::detect`](crate::pii::detect) engine finds spans in the original
//! text and emits [`PiiFinding`][crate::pii::PiiFinding]s carrying original-text
//! byte offsets. [`sanitize_text`] applies a [`RedactionPolicy`] to rebuild the
//! text with each finding replaced.
//!
//! # Length-leak trade-off
//!
//! [`RedactionPolicy::Mask`] preserves the match length, which leaks the PII
//! length (e.g. digit count). [`RedactionPolicy::Hash`] and
//! [`RedactionPolicy::Placeholder`] emit fixed-width tokens and do not leak
//! length. Prefer `Hash` or `Placeholder` when the sanitized output is shown to
//! parties who should not learn the original length.

use std::borrow::Cow;

use crate::pii::PiiFinding;
use crate::schema::BuiltInPattern;

/// How a detected PII span is rewritten in the sanitized output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RedactionPolicy {
    /// Typed bracket placeholder derived from the entity slug, e.g.
    /// `[REDACTED_SSN]`, `[REDACTED_EMAIL]`. Mirrors the engine's built-in
    /// replacement style but namespaced under `REDACTED_`.
    #[default]
    Placeholder,
    /// Mask each character of the match with `mask_char` (e.g. `********`).
    /// Preserves the match length (see the length-leak note above).
    Mask {
        /// Character used to mask each position of the match.
        mask_char: char,
    },
    /// Content-derived token `[SHA256:8f3a9c12]`. Deterministic per match text,
    /// so downstream joins/re-identification are possible without revealing
    /// the secret. `hex_chars` caps the emitted hex prefix (default 8).
    Hash {
        /// Number of hex characters of the SHA-256 digest to emit.
        hex_chars: u8,
    },
}

impl RedactionPolicy {
    /// Computes the replacement string for a single match.
    #[must_use]
    pub fn replacement<'a>(&self, matched: &'a str, entity: &str) -> Cow<'a, str> {
        match self {
            Self::Placeholder => {
                let slug = placeholder_slug(entity);
                Cow::Owned(format!("[REDACTED_{slug}]"))
            }
            Self::Mask { mask_char } => {
                Cow::Owned(mask_char.to_string().repeat(matched.chars().count()))
            }
            Self::Hash { hex_chars } => {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(matched.as_bytes());
                let digest = hasher.finalize();
                let hex = hex_encode(&digest);
                let n = (*hex_chars as usize).min(hex.len());
                Cow::Owned(format!("[SHA256:{}]", &hex[..n]))
            }
        }
    }
}

/// Rebuilds `text` with every finding in `findings` replaced according to
/// `policy`.
///
/// `findings` carry offsets into `text` (the original, pre-sanitization
/// document). Overlaps are resolved defensively by skipping any finding whose
/// start precedes the current cursor (the scrubber already resolves overlaps,
/// so this is a safety net).
///
/// # Errors
///
/// Returns [`crate::BitVanesError::InvalidInput`] if a finding's byte range is
/// not on a UTF-8 boundary or is out of bounds for `text`.
pub fn sanitize_text(
    text: &str,
    findings: &[PiiFinding],
    policy: &RedactionPolicy,
) -> crate::Result<String> {
    let mut ordered: Vec<&PiiFinding> = findings.iter().collect();
    ordered.sort_by_key(|f| f.offset_start);

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for f in ordered {
        let start = usize::try_from(f.offset_start).unwrap_or(usize::MAX);
        let end = usize::try_from(f.offset_end).unwrap_or(usize::MAX);
        if start < cursor || end < start || end > text.len() {
            continue;
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Err(crate::BitVanesError::InvalidInput(format!(
                "PII finding offset [{start}..{end}] is not on a UTF-8 boundary"
            )));
        }
        out.push_str(&text[cursor..start]);
        out.push_str(&policy.replacement(&text[start..end], &f.entity));
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Ok(out)
}

/// Maps an entity slug (or built-in pattern name) to the `UPPER_SNAKE` placeholder
/// tail used by [`RedactionPolicy::Placeholder`].
fn placeholder_slug(entity: &str) -> String {
    // Built-in entities are already snake_case slugs (e.g. `credit_card`).
    // A `BuiltInPattern` variant name is rendered as its snake_case serde form.
    if let Ok(p) = serde_json::from_str::<BuiltInPattern>(&format!("\"{entity}\"")) {
        return serde_json::to_string(&p)
            .unwrap_or_default()
            .trim_matches('"')
            .to_ascii_uppercase();
    }
    entity.to_ascii_uppercase()
}

/// Lowercase hex encoding without pulling in the `hex` crate.
fn hex_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(TABLE[(b >> 4) as usize] as char);
        s.push(TABLE[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pii::PiiFinding;

    fn finding(entity: &str, start: u32, end: u32) -> PiiFinding {
        PiiFinding {
            entity: entity.to_string(),
            offset_start: start,
            offset_end: end,
            confidence: 0.95,
            anchors_hit: vec![],
        }
    }

    #[test]
    fn placeholder_uses_redacted_namespace() {
        let p = RedactionPolicy::Placeholder;
        assert_eq!(p.replacement("x", "ssn"), "[REDACTED_SSN]");
        assert_eq!(p.replacement("x", "credit_card"), "[REDACTED_CREDIT_CARD]");
        assert_eq!(p.replacement("x", "aws_key"), "[REDACTED_AWS_KEY]");
    }

    #[test]
    fn mask_preserves_match_length() {
        let p = RedactionPolicy::Mask { mask_char: '*' };
        assert_eq!(p.replacement("123-45-6789", "ssn"), "***********");
        assert_eq!(p.replacement(" José ", "name"), "******"); // 6 chars
    }

    #[test]
    fn hash_is_deterministic_and_truncated() {
        let p = RedactionPolicy::Hash { hex_chars: 8 };
        let a = p.replacement("alice@example.com", "email");
        let b = p.replacement("alice@example.com", "email");
        assert_eq!(a, b, "hash must be deterministic");
        assert!(a.starts_with("[SHA256:") && a.ends_with(']'));
        // 8 hex chars between the brackets.
        let inner = &a["[SHA256:".len()..a.len() - 1];
        assert_eq!(inner.len(), 8);
        assert!(inner.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sanitize_text_replaces_all_findings() {
        let text = "email alice@example.com or ssn 123-45-6789 end";
        //        indices:  6..23 = email               31..42 = ssn
        let findings = vec![finding("email", 6, 23), finding("ssn", 31, 42)];
        let out = sanitize_text(text, &findings, &RedactionPolicy::Placeholder).unwrap();
        assert!(!out.contains("alice@example.com"));
        assert!(!out.contains("123-45-6789"));
        assert!(out.contains("[REDACTED_EMAIL]"));
        assert!(out.contains("[REDACTED_SSN]"));
        assert!(out.starts_with("email "));
        assert!(out.ends_with(" end"));
    }

    #[test]
    fn sanitize_text_rejects_non_boundary_offset() {
        // 'é' is two bytes; offset 1 is mid-code-point.
        let text = "é";
        let findings = vec![finding("x", 1, 2)];
        let err = sanitize_text(text, &findings, &RedactionPolicy::Placeholder);
        assert!(err.is_err(), "mid-codepoint offset must be rejected");
    }

    #[test]
    fn sanitize_text_skips_overlapping_and_oob_findings() {
        let text = "abc";
        let findings = vec![
            finding("x", 0, 2),
            finding("y", 1, 3),   // overlaps previous → skipped
            finding("z", 10, 20), // out of bounds → skipped
        ];
        let out =
            sanitize_text(text, &findings, &RedactionPolicy::Mask { mask_char: '#' }).unwrap();
        assert_eq!(out, "##c");
    }

    #[test]
    fn no_findings_returns_text_unchanged() {
        let text = "nothing here";
        let out = sanitize_text(text, &[], &RedactionPolicy::default()).unwrap();
        assert_eq!(out, text);
    }
}
