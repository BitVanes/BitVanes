//! `bitvanes scrub` — batch-sanitize a file or directory.
//!
//! For each input file: parse → detect PII → redact per the configured policy
//! → write the sanitized text. Prints a categorized stats summary on stderr.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use walkdir::WalkDir;

use bitvanes_core::parse::parse_bytes;
use bitvanes_core::sanitizer::pdfium::PdfRedactMode;
use bitvanes_core::sanitizer::{RedactionPolicy, SanitizationStats, redact_pdf, sanitize_text};
use bitvanes_core::schema::{DocumentFormat, PipelineConfig};

use crate::shared::{
    ConfigArg, ResolvedConfig, RulesArg, extensions_for, infer_format, resolve_config,
};

/// `bitvanes scrub <INPUT> [--out <OUT>] [--rules ...] [--config ...]`.
#[derive(Args, Debug, Clone)]
pub struct ScrubArgs {
    /// Input file or directory (recursive).
    pub input: PathBuf,

    /// Output file or directory. Use `-` for stdout (single input file only).
    #[arg(short, long, value_name = "OUT")]
    pub out: Option<String>,

    #[command(flatten)]
    pub rules: RulesArg,

    #[command(flatten)]
    pub config: ConfigArg,

    /// Override output style (`mask` | `hash` | `placeholder`) for text output.
    #[arg(long, value_name = "STYLE")]
    pub redact: Option<String>,

    /// PDF redaction mode: `redact` (destructive object removal, default),
    /// `flatten` (rasterize to image, drop text layer), or `text-only`
    /// (extract text → redact → emit `.txt`). Requires a runtime `libpdfium`
    /// for `redact`/`flatten`; fails closed if unavailable.
    #[arg(long, value_name = "MODE", default_value = "redact")]
    pub pdf_mode: String,
}

/// Entry point for `bitvanes scrub`.
pub fn run(args: ScrubArgs) -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_config(args.config.config.as_deref(), args.rules.rules.as_deref())?;
    let policy = override_policy(&resolved.policy, args.redact.as_deref())?;
    // `text-only` → None (route to the text-layer path). Unknown → hard error.
    let pdf_mode = PdfRedactMode::parse(&args.pdf_mode)
        .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;

    let files = collect_files(&args.input)?;
    if files.is_empty() {
        return Err(format!("no supported files under {}", args.input.display()).into());
    }

    let out_target = args.out.as_deref().unwrap_or("-");
    let mirror = args.input.is_dir() && out_target != "-";
    if mirror {
        fs::create_dir_all(out_target)?;
    }

    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::with_template("{wide_bar} {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("█░"),
    );

    let mut stats = SanitizationStats::new();
    let mut failures: Vec<(std::path::PathBuf, String)> = Vec::new();
    let start = Instant::now();
    let cfg = PipelineConfig::default();

    for path in &files {
        pb.inc(1);
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                failures.push((path.clone(), format!("read: {e}")));
                continue;
            }
        };
        match sanitize_bytes(&bytes, path, &cfg, &resolved, &policy, pdf_mode) {
            Ok(Sanitized::Text(text, findings)) => {
                stats.record_file(bytes.len() as u64, &findings);
                write_output(
                    &Sanitized::Text(text, findings),
                    path,
                    &args.input,
                    out_target,
                    mirror,
                )?;
            }
            Ok(Sanitized::Pdf(pdf_bytes, matches_scrubbed)) => {
                stats.record_pdf_file(bytes.len() as u64, matches_scrubbed);
                write_output(
                    &Sanitized::Pdf(pdf_bytes, matches_scrubbed),
                    path,
                    &args.input,
                    out_target,
                    mirror,
                )?;
            }
            Err(e) => {
                // Sanitization failure (e.g. corrupt/unsupported file): do NOT
                // write anything for this file (fail closed — never emit
                // unscrubbed bytes). Record the failure and continue the batch.
                failures.push((path.clone(), e.to_string()));
            }
        }
    }
    pb.finish_and_clear();
    stats.finalize(start.elapsed());

    print_stats(&stats);
    if !failures.is_empty() {
        eprintln!(
            "  {} file(s) failed (no output written for them):",
            failures.len()
        );
        for (p, e) in &failures {
            eprintln!("    ⚠ {}: {e}", p.display());
        }
        // Non-zero exit so CI / scripts can detect that not every file scrubbed.
        let summary = if stats.files_processed() == 0 {
            format!("no files could be scrubbed ({} failed)", failures.len())
        } else {
            format!(
                "{} scrubbed, {} failed",
                stats.files_processed(),
                failures.len()
            )
        };
        return Err(summary.into());
    }
    Ok(())
}

/// Sanitized output for a single file (either redacted text or a re-encoded PDF).
enum Sanitized {
    Text(String, Vec<bitvanes_core::pii::PiiFinding>),
    /// Destructively-redacted PDF bytes + the number of PII spans scrubbed.
    Pdf(Vec<u8>, usize),
}

/// Sanitizes a single file's bytes.
///
/// PDFs take the destructive `--pdf-mode` path (object removal / flatten) and
/// return re-encoded PDF bytes; every other format returns redacted text.
fn sanitize_bytes(
    bytes: &[u8],
    path: &Path,
    cfg: &PipelineConfig,
    resolved: &ResolvedConfig,
    policy: &RedactionPolicy,
    pdf_mode: Option<PdfRedactMode>,
) -> Result<Sanitized, Box<dyn std::error::Error>> {
    let format = infer_format(path);
    if format == DocumentFormat::Pdf {
        if let Some(mode) = pdf_mode {
            let (out, result) = redact_pdf(bytes, &resolved.scrubber, mode)
                .map_err(|e| format!("pdf redact failed: {e}"))?;
            return Ok(Sanitized::Pdf(out, result.pii_matches_scrubbed));
        }
    }

    let mut file_cfg = cfg.clone();
    file_cfg.format = format;
    let doc = parse_bytes(bytes, &file_cfg)?;
    // Detect on the ORIGINAL extracted text so finding offsets align with the
    // text we hand to `sanitize_text`.
    let (_engine_redacted, _map, findings) = resolved.scrubber.scrub(&doc.full_text);
    let text =
        sanitize_text(&doc.full_text, &findings, policy).map_err(|e| format!("sanitize: {e}"))?;
    Ok(Sanitized::Text(text, findings))
}

/// Writes sanitized output (text or PDF bytes) to the resolved target.
fn write_output(
    sanitized: &Sanitized,
    src: &Path,
    input_root: &Path,
    out_target: &str,
    mirror: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (data, extension): (Vec<u8>, &str) = match sanitized {
        Sanitized::Text(text, _) => (text.as_bytes().to_vec(), "txt"),
        Sanitized::Pdf(bytes, _) => (bytes.clone(), "pdf"),
    };
    if out_target == "-" {
        use std::io::Write;
        std::io::stdout().write_all(&data)?;
        return Ok(());
    }
    let dest = if mirror {
        let rel = src.strip_prefix(input_root).unwrap_or(src);
        Path::new(out_target).join(rel).with_extension(extension)
    } else {
        Path::new(out_target).to_path_buf()
    };
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, data)?;
    Ok(())
}

/// Collects supported files under `input` (single file or recursive dir walk).
fn collect_files(input: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if input.is_file() {
        return Ok(vec![input.canonicalize()?]);
    }
    if !input.is_dir() {
        return Err(format!("{} is not a file or directory", input.display()).into());
    }
    let exts = extensions_for(None);
    let files: Vec<PathBuf> = WalkDir::new(input)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| exts.contains(&x.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect();
    Ok(files)
}

/// Overrides the resolved policy when `--redact` is given on the command line.
fn override_policy(
    base: &RedactionPolicy,
    style: Option<&str>,
) -> Result<RedactionPolicy, Box<dyn std::error::Error>> {
    match style {
        None => Ok(base.clone()),
        Some("placeholder") => Ok(RedactionPolicy::Placeholder),
        Some("mask") => Ok(RedactionPolicy::Mask { mask_char: '*' }),
        Some("hash") => Ok(RedactionPolicy::Hash { hex_chars: 8 }),
        Some(other) => {
            Err(format!("unknown --redact style '{other}' (mask|hash|placeholder)").into())
        }
    }
}

/// Prints the categorized stats summary to stderr.
fn print_stats(stats: &SanitizationStats) {
    eprintln!();
    eprintln!("BitVanes sanitization complete");
    eprintln!("  Files processed:    {}", stats.files_processed());
    eprintln!("  Total bytes:        {}", stats.bytes_sanitized());
    eprintln!("  PII instances:      {}", stats.total_pii());
    if !stats.categories().is_empty() {
        eprintln!("  By category:");
        for c in stats.categories() {
            eprintln!("    {:>16}: {}", c.entity, c.count);
        }
    }
    eprintln!(
        "  Throughput:         {:.2} MiB/s",
        stats.throughput_mib_s()
    );
}
