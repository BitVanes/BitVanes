//! Shared helpers used by the `scrub`, `filter`, and `daemon` subcommands:
//! loading a `Bitvanes.toml` into a [`Scrubber`] + [`RedactionPolicy`], format
//! inference, and output routing.

use std::fs;
use std::path::{Path, PathBuf};

use bitvanes_core::BitvanesConfig;
use bitvanes_core::pii::Scrubber;
use bitvanes_core::sanitizer::RedactionPolicy;
use bitvanes_core::schema::{BuiltInPattern, DocumentFormat, ScrubProfile};
use clap::Args;

/// Shared flags that select the built-in rule set when no `Bitvanes.toml` is given.
#[derive(Args, Debug, Clone, Default)]
pub struct RulesArg {
    /// Comma-separated built-in PII slugs (e.g. `email,ssn,credit_card`).
    #[arg(long, value_name = "SLUGS")]
    pub rules: Option<String>,
}

/// Shared config-source flag.
#[derive(Args, Debug, Clone, Default)]
pub struct ConfigArg {
    /// Path to a `Bitvanes.toml` config file.
    #[arg(short, long, value_name = "FILE", global = true)]
    pub config: Option<PathBuf>,
}

/// A resolved PII configuration: a compiled [`Scrubber`] and the output policy.
pub struct ResolvedConfig {
    pub scrubber: Scrubber,
    pub policy: RedactionPolicy,
    pub profile: ScrubProfile,
}

/// Resolves a PII config from, in priority order:
/// 1. an explicit `Bitvanes.toml` path (`config_path`),
/// 2. a comma-separated `--rules` list (built-in slugs),
/// 3. the built-in default ruleset (email, ssn, phone, credit_card).
///
/// # Errors
///
/// Returns an error if the config file cannot be read/parsed or a rule slug is
/// unknown.
pub fn resolve_config(
    config_path: Option<&Path>,
    rules: Option<&str>,
) -> Result<ResolvedConfig, Box<dyn std::error::Error>> {
    let cfg = if let Some(path) = config_path {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("could not read config {}: {e}", path.display()))?;
        Some(BitvanesConfig::from_toml_str(&text)?)
    } else {
        None
    };

    let (profile, policy) = if let Some(cfg) = &cfg {
        (cfg.to_scrub_profile()?, cfg.to_redaction_policy())
    } else {
        let patterns = match rules {
            Some(r) => parse_rules(Some(r)),
            None => default_patterns(),
        };
        (
            ScrubProfile {
                patterns,
                ..ScrubProfile::default()
            },
            RedactionPolicy::default(),
        )
    };

    let scrubber = Scrubber::from_profile(&profile)?;
    Ok(ResolvedConfig {
        scrubber,
        policy,
        profile,
    })
}

/// The default ruleset applied when no `--rules` and no `Bitvanes.toml` are
/// given: the "standard PII ruleset" from the product spec (email, SSN, phone,
/// credit card). `street_address` is intentionally opt-in (higher FP rate).
#[must_use]
pub fn default_patterns() -> Vec<BuiltInPattern> {
    vec![
        BuiltInPattern::Email,
        BuiltInPattern::Ssn,
        BuiltInPattern::Phone,
        BuiltInPattern::CreditCard,
    ]
}

/// Parses a comma-separated list of built-in slugs into patterns. Unknown
/// slugs are silently skipped (the engine's serde does the validation).
pub fn parse_rules(rules: Option<&str>) -> Vec<BuiltInPattern> {
    let Some(rules) = rules else {
        return Vec::new();
    };
    rules
        .split(',')
        .filter_map(|s| serde_json::from_str::<BuiltInPattern>(&format!("\"{}\"", s.trim())).ok())
        .collect()
}

/// Infers a [`DocumentFormat`] from a file extension.
#[must_use]
pub fn infer_format(path: &Path) -> DocumentFormat {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("md") | Some("markdown") => DocumentFormat::Markdown,
        Some("txt") | Some("csv") | Some("jsonl") | Some("log") => DocumentFormat::Text,
        Some("html") | Some("htm") => DocumentFormat::Html,
        Some("pdf") => DocumentFormat::Pdf,
        Some("json") => DocumentFormat::Json,
        Some("docx") => DocumentFormat::Docx,
        Some("pptx") => DocumentFormat::Pptx,
        Some("xlsx") => DocumentFormat::Xlsx,
        Some("epub") => DocumentFormat::Epub,
        Some("rtf") => DocumentFormat::Rtf,
        _ => DocumentFormat::Markdown,
    }
}

/// Returns the file extensions to collect for a directory walk, given the
/// configured format (or all supported extensions if `None`).
#[must_use]
pub fn extensions_for(format: Option<DocumentFormat>) -> Vec<&'static str> {
    match format {
        Some(DocumentFormat::Markdown) => vec!["md", "markdown"],
        Some(DocumentFormat::Text) => vec!["txt", "csv", "jsonl", "log"],
        Some(DocumentFormat::Html) => vec!["html", "htm"],
        Some(DocumentFormat::Pdf) => vec!["pdf"],
        Some(DocumentFormat::Json) => vec!["json"],
        Some(DocumentFormat::Docx) => vec!["docx"],
        Some(DocumentFormat::Pptx) => vec!["pptx"],
        Some(DocumentFormat::Xlsx) => vec!["xlsx"],
        Some(DocumentFormat::Epub) => vec!["epub"],
        Some(DocumentFormat::Rtf) => vec!["rtf"],
        Some(_) => Vec::new(),
        None => vec![
            "md", "markdown", "txt", "csv", "jsonl", "log", "html", "htm", "pdf", "json", "docx",
            "pptx", "xlsx", "epub", "rtf",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rules_skips_unknown_and_keeps_known() {
        let p = parse_rules(Some("email,ssn, credit_card ,, unknown"));
        assert_eq!(p.len(), 3);
        assert!(p.contains(&BuiltInPattern::Email));
        assert!(p.contains(&BuiltInPattern::Ssn));
        assert!(p.contains(&BuiltInPattern::CreditCard));
    }

    #[test]
    fn infer_format_covers_every_extension() {
        assert_eq!(infer_format(Path::new("/x/a.docx")), DocumentFormat::Docx);
        assert_eq!(infer_format(Path::new("/x/a.XLSX")), DocumentFormat::Xlsx);
        assert_eq!(
            infer_format(Path::new("/x/a.unknown")),
            DocumentFormat::Markdown
        );
    }

    #[test]
    fn resolve_config_with_rules_only_builds_scrubber() {
        let r = resolve_config(None, Some("email,ssn")).unwrap();
        assert!(!r.scrubber.is_empty());
        assert_eq!(r.profile.patterns.len(), 2);
        assert!(matches!(r.policy, RedactionPolicy::Placeholder));
    }
}
