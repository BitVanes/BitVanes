//! `bitvanes filter` — stream stdin → stdout, redacting PII in flight using the
//! bounded-memory [`StreamSanitizer`].

use std::io;

use clap::Args;
use tokio::io::{AsyncWriteExt, stdin, stdout};

use bitvanes_core::pii::Scrubber;
use bitvanes_core::sanitizer::{DEFAULT_MAX_MATCH_LEN, RedactionPolicy, StreamSanitizer};

use crate::shared::{ConfigArg, RulesArg, resolve_config};

/// `bitvanes filter [--mask-char <C>] [--rules ...] [--config ...]`.
#[derive(Args, Debug, Clone)]
pub struct FilterArgs {
    #[command(flatten)]
    pub rules: RulesArg,

    #[command(flatten)]
    pub config: ConfigArg,

    /// Mask character (sets the redaction policy to `mask`).
    #[arg(long, value_name = "CHAR", default_value = "*")]
    pub mask_char: char,

    /// Override the match window (bytes). Defaults to {DEFAULT}.
    #[arg(long, value_name = "N")]
    pub max_match_len: Option<usize>,
}

/// Entry point for `bitvanes filter`.
pub fn run(args: FilterArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Drive the async sanitizer on a current-thread runtime. The stream path is
    // I/O-bound + regex CPU work; a single worker suffices for a pipe.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run_async(args))
}

async fn run_async(args: FilterArgs) -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_config(args.config.config.as_deref(), args.rules.rules.as_deref())?;
    // `filter` defaults to the mask policy (the spec's `--mask-char` UX).
    let policy = RedactionPolicy::Mask {
        mask_char: args.mask_char,
    };
    // Rebuild the scrubber from the resolved profile (resolve_config already
    // compiled one, but we need it by value into the sanitizer).
    let scrubber = Scrubber::from_profile(&resolved.profile)?;
    let window = args.max_match_len.unwrap_or(DEFAULT_MAX_MATCH_LEN);
    let mut sanitizer = StreamSanitizer::with_max_match_len(scrubber, policy, window);

    let mut stdin = stdin();
    let mut stdout = stdout();
    let findings = sanitizer
        .sanitize_reader(&mut stdin, &mut stdout)
        .await
        .map_err(|e| io::Error::other(format!("sanitize: {e}")))?;
    stdout.flush().await?;

    // Stats go to stderr so stdout stays a clean data pipe.
    eprintln!(
        "\nBitVanes filter: {} PII instance(s) redacted",
        findings.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_window_is_reasonable() {
        // The default match window must be large enough to cover a credit-card
        // or SSN straddling a chunk boundary.
        let window = DEFAULT_MAX_MATCH_LEN;
        assert!(window >= 256);
    }
}
