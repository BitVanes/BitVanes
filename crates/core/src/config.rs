//! `Bitvanes.toml` user-facing configuration loader.
//!
//! The engine's internal config ([`PipelineConfig`]) is JSON and tuned for the
//! wire format shared with the web/daemon. End users edit a friendlier TOML
//! file at the path they pass to `bitvanes --config Bitvanes.toml`. This module
//! parses that TOML into a [`BitvanesConfig`] and projects it into the engine's
//! [`ScrubProfile`] + a [`RedactionPolicy`].
//!
//! # Example
//!
//! ```toml
//! [pii]
//! patterns = ["email", "ssn", "credit_card", "street_address"]
//! min_confidence = 0.5
//! anchor_window = 7
//! report_only = ["phone"]   # detect but don't redact
//!
//! [[pii.custom]]
//! name = "project_id"
//! regex = "\\bPROJECT-\\d+\\b"
//! replacement = "[PROJECT-ID]"
//!
//! [output]
//! redaction = "placeholder" # "placeholder" | "mask" | "hash"
//! mask_char = "*"
//! hash_hex_chars = 8
//! ```
//!
//! Enabled behind the `config` cargo feature.

use serde::Deserialize;

use crate::error::{BitVanesError, Result};
use crate::sanitizer::RedactionPolicy;
use crate::schema::{BuiltInPattern, CustomPattern, ScrubProfile};

/// Top-level TOML config.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BitvanesConfig {
    /// PII detection rules.
    #[serde(default)]
    pub pii: PiiSection,
    /// Output / redaction policy.
    #[serde(default)]
    pub output: OutputSection,
}

/// The `[pii]` section.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PiiSection {
    /// Built-in pattern slugs (`"email"`, `"ssn"`, `"credit_card"`,
    /// `"routing_number"`, `"phone"`, `"street_address"`, `"aws_key"`,
    /// `"github_pat"`, `"jwt"`).
    #[serde(default)]
    pub patterns: Vec<String>,
    /// User-supplied regex rules.
    #[serde(default)]
    pub custom: Vec<CustomRule>,
    /// Minimum confidence `[0, 1]` for a candidate to be redacted.
    #[serde(default)]
    pub min_confidence: f32,
    /// Half-window (words) for contextual anchor boosting. Default 7.
    #[serde(default)]
    pub anchor_window: Option<u8>,
    /// Entity slugs to detect but not redact (report-only).
    #[serde(default)]
    pub report_only: Vec<String>,
}

/// A `[[pii.custom]]` user regex rule.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomRule {
    /// Stable name emitted as the finding's `entity` slug.
    pub name: String,
    /// Rust `regex` syntax.
    pub regex: String,
    /// Replacement emitted by the engine's built-in placeholder path.
    pub replacement: String,
}

/// The `[output]` section.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OutputSection {
    /// Redaction style: `"placeholder"` (default), `"mask"`, or `"hash"`.
    #[serde(default)]
    pub redaction: RedactionStyle,
    /// Mask character for `redaction = "mask"`. Default `'*'`.
    #[serde(default = "default_mask_char")]
    pub mask_char: char,
    /// Hex characters for `redaction = "hash"`. Default 8.
    #[serde(default = "default_hash_hex_chars")]
    pub hash_hex_chars: u8,
}

fn default_mask_char() -> char {
    '*'
}
const fn default_hash_hex_chars() -> u8 {
    8
}

/// TOML-friendly redaction style selector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RedactionStyle {
    /// `[REDACTED_<ENTITY>]` (default).
    #[default]
    Placeholder,
    /// Fixed mask of `mask_char`.
    Mask,
    /// `[SHA256:8f3a9c12]`.
    Hash,
}

impl BitvanesConfig {
    /// Parses a `Bitvanes.toml` document from a string.
    ///
    /// # Errors
    ///
    /// Returns [`BitVanesError::InvalidConfig`] if the TOML is malformed or a
    /// built-in pattern slug is unknown.
    pub fn from_toml_str(s: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(s)
            .map_err(|e| BitVanesError::InvalidConfig(format!("Bitvanes.toml: {e}")))?;
        // Eagerly validate pattern slugs so a typo surfaces at load time.
        for slug in &cfg.pii.patterns {
            parse_pattern_slug(slug)?;
        }
        Ok(cfg)
    }

    /// Projects the TOML config into the engine's [`ScrubProfile`].
    ///
    /// # Errors
    ///
    /// Returns [`BitVanesError::InvalidConfig`] if any pattern slug or custom
    /// regex is invalid.
    pub fn to_scrub_profile(&self) -> Result<ScrubProfile> {
        let mut patterns = Vec::with_capacity(self.pii.patterns.len());
        for slug in &self.pii.patterns {
            patterns.push(parse_pattern_slug(slug)?);
        }
        let mut custom = Vec::with_capacity(self.pii.custom.len());
        for rule in &self.pii.custom {
            // Validate the regex compiles now (fail loud at load).
            regex::Regex::new(&rule.regex).map_err(|e| {
                BitVanesError::InvalidConfig(format!("custom regex '{}': {e}", rule.name))
            })?;
            custom.push(CustomPattern {
                regex: rule.regex.clone(),
                replacement: rule.replacement.clone(),
            });
        }
        Ok(ScrubProfile {
            patterns,
            custom,
            anchor_window: self.pii.anchor_window.unwrap_or(7),
            min_confidence: self.pii.min_confidence,
            report_only: self.pii.report_only.clone(),
        })
    }

    /// Projects the TOML config's output section into a [`RedactionPolicy`].
    #[must_use]
    pub fn to_redaction_policy(&self) -> RedactionPolicy {
        match self.output.redaction {
            RedactionStyle::Placeholder => RedactionPolicy::Placeholder,
            RedactionStyle::Mask => RedactionPolicy::Mask {
                mask_char: self.output.mask_char,
            },
            RedactionStyle::Hash => RedactionPolicy::Hash {
                hex_chars: self.output.hash_hex_chars,
            },
        }
    }
}

/// Parses a built-in pattern slug (`"email"` → [`BuiltInPattern::Email`]).
fn parse_pattern_slug(slug: &str) -> Result<BuiltInPattern> {
    serde_json::from_str::<BuiltInPattern>(&format!("\"{}\"", slug.trim()))
        .map_err(|_| BitVanesError::InvalidConfig(format!("unknown pii pattern '{slug}'")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitizer::RedactionPolicy;

    #[test]
    fn parses_full_config() {
        let toml = r##"
[pii]
patterns = ["email", "ssn", "credit_card"]
min_confidence = 0.5
anchor_window = 9
report_only = ["phone"]

[[pii.custom]]
name = "project_id"
regex = "\\bPROJECT-\\d+\\b"
replacement = "[PROJECT-ID]"

[output]
redaction = "mask"
mask_char = "#"
"##;
        let cfg = BitvanesConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.pii.patterns.len(), 3);
        assert!((cfg.pii.min_confidence - 0.5).abs() < 1e-6);
        assert_eq!(cfg.pii.anchor_window, Some(9));
        assert_eq!(cfg.pii.report_only, vec!["phone".to_string()]);
        assert_eq!(cfg.pii.custom.len(), 1);
        assert_eq!(cfg.output.redaction, RedactionStyle::Mask);
        assert_eq!(cfg.output.mask_char, '#');

        let profile = cfg.to_scrub_profile().unwrap();
        assert_eq!(profile.patterns.len(), 3);
        assert_eq!(profile.custom.len(), 1);
        assert_eq!(profile.anchor_window, 9);
        assert!(matches!(
            cfg.to_redaction_policy(),
            RedactionPolicy::Mask { mask_char: '#' }
        ));
    }

    #[test]
    fn defaults_to_placeholder_with_empty_config() {
        let cfg = BitvanesConfig::from_toml_str("").unwrap();
        let profile = cfg.to_scrub_profile().unwrap();
        assert!(profile.patterns.is_empty());
        assert!(profile.custom.is_empty());
        assert!(matches!(
            cfg.to_redaction_policy(),
            RedactionPolicy::Placeholder
        ));
    }

    #[test]
    fn unknown_pattern_slug_is_rejected_at_load() {
        let err = BitvanesConfig::from_toml_str("[pii]\npatterns = [\"nope\"]\n").unwrap_err();
        assert!(matches!(err, BitVanesError::InvalidConfig(_)));
    }

    #[test]
    fn invalid_custom_regex_is_rejected() {
        let toml = "[pii]\n[[pii.custom]]\nname = \"bad\"\nregex = \"[unterminated\"\nreplacement = \"x\"\n";
        let cfg = BitvanesConfig::from_toml_str(toml).unwrap();
        let err = cfg.to_scrub_profile().unwrap_err();
        assert!(matches!(err, BitVanesError::InvalidConfig(_)));
    }

    #[test]
    fn hash_policy_picks_up_hex_chars() {
        let cfg =
            BitvanesConfig::from_toml_str("[output]\nredaction = \"hash\"\nhash_hex_chars = 12\n")
                .unwrap();
        match cfg.to_redaction_policy() {
            RedactionPolicy::Hash { hex_chars } => assert_eq!(hex_chars, 12),
            other => panic!("expected hash, got {other:?}"),
        }
    }
}
