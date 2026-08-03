//! `bitvanes` — zero-trust local PII purification.
//!
//! Subcommands:
//! - `scrub`  : batch-sanitize a file or directory (detect/redact PII + stats).
//! - `filter` : stream stdin → stdout, redacting PII in flight.
//! - `daemon` : local HTTP daemon (127.0.0.1 only) for the dashboard / API.
//! - `tui`    : interactive terminal UI.

mod daemon;
mod entitlement;
mod filter;
mod scrub;
mod shared;
mod tui;

use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

/// Zero-trust local PII purification. Direct, filter, and purify data streams
/// before they reach downstream systems — 100% on-premise, zero cloud uploads.
#[derive(Parser)]
#[command(
    name = "bitvanes",
    version,
    propagate_version = true,
    long_about = None,
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Command {
    /// Batch-sanitize a file or directory: detect/redact PII, write clean
    /// output, and print a categorized stats summary.
    Scrub(scrub::ScrubArgs),
    /// Filter a byte stream on stdin → stdout (e.g. `cat f.json | bitvanes filter`).
    Filter(filter::FilterArgs),
    /// Run a local HTTP daemon on 127.0.0.1 for the dashboard / API.
    Daemon(daemon::DaemonArgs),
    /// Interactive terminal UI (file browser + config editor + results).
    Tui(tui::TuiArgs),
    /// Manage local configuration (install a license key, etc.).
    Config(ConfigArgs),
}

/// `bitvanes config --key <BV-…>` — install / verify a license key.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ConfigArgs {
    /// A `BV-{TIER}-{jwt}` license key (from bitvanes.com). Verified offline
    /// against the embedded public key and persisted to `~/.bitvanes/license`.
    #[arg(long, value_name = "KEY")]
    pub key: Option<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Scrub(args)) => exit(scrub::run(args)),
        Some(Command::Filter(args)) => exit(filter::run(args)),
        Some(Command::Daemon(args)) => exit(daemon::run(args)),
        Some(Command::Tui(args)) => exit(tui::run(&args).map_err(Box::from)),
        Some(Command::Config(args)) => exit(config_run(args)),
        None => {
            // No subcommand: print help and exit success.
            let _ = Cli::command().print_help();
            println!();
            ExitCode::SUCCESS
        }
    }
}

fn exit(result: Result<(), Box<dyn std::error::Error>>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `bitvanes config` handler.
fn config_run(args: ConfigArgs) -> Result<(), Box<dyn std::error::Error>> {
    match args.key {
        Some(k) => {
            let msg = entitlement::install_key(&k).map_err(Box::<dyn std::error::Error>::from)?;
            println!("{msg}");
            Ok(())
        }
        None => {
            let checker = entitlement::resolve_checker(None);
            println!("{}", checker.status().label());
            Ok(())
        }
    }
}
