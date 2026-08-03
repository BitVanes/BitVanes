//! TUI module: interactive terminal purification viewer using ratatui.
//!
//! Retained from the original product as an interactive file browser + config
//! editor + results viewer. It runs the parse → scrub → chunk pipeline and
//! shows the chunk table with the PII findings that overlap each chunk.

pub mod app;
pub mod event;
pub mod ui;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use serde::Serialize;

use bitvanes_core::arrow_io::batch::chunks_to_batch;
use bitvanes_core::schema::{
    BuiltInPattern, ChunkSpec, DocumentFormat, PipelineConfig, ScrubProfile, TokenizerKind,
};

/// `bitvanes tui` flags.
#[derive(Args, Debug, Clone)]
pub struct TuiArgs {
    /// Input file or directory (optional — the TUI has a file browser).
    #[arg(short, long, value_name = "PATH")]
    pub input: Option<PathBuf>,

    /// Pipeline profile JSON.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Document format (inferred from extension if omitted).
    #[arg(short, long, value_name = "FORMAT")]
    pub format: Option<String>,

    /// BPE tokenizer for chunk boundaries.
    #[arg(short, long, value_name = "NAME")]
    pub tokenizer: Option<String>,

    /// Maximum tokens per chunk.
    #[arg(short = 'm', long, value_name = "N")]
    pub max_tokens: Option<u32>,

    /// PII patterns to scrub (comma-separated).
    #[arg(long, value_name = "PATTERNS")]
    pub scrub: Option<String>,

    /// Output file (.arrow / .csv / .json).
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<String>,
}

/// Builds a `PipelineConfig` from a profile JSON and/or CLI flags.
pub(crate) fn build_config(cli: &TuiArgs) -> Result<PipelineConfig, Box<dyn std::error::Error>> {
    let mut config = if let Some(path) = &cli.config {
        let json = fs::read_to_string(path)
            .map_err(|e| format!("could not read config {}: {e}", path.display()))?;
        serde_json::from_str(&json)?
    } else {
        PipelineConfig::default()
    };
    if let Some(fmt) = &cli.format {
        config.format = parse_format(fmt)?;
    }
    if let Some(tok) = &cli.tokenizer {
        config.chunk.tokenizer = parse_tokenizer(tok)?;
    }
    if let Some(mt) = cli.max_tokens {
        config.chunk.max_tokens = mt;
    }
    if let Some(scrub) = &cli.scrub {
        config.scrub = parse_scrub(scrub);
    }
    Ok(config)
}

/// Overrides format from extension so each file is parsed by the right parser.
pub(crate) fn infer_format(path: &Path, mut cfg: PipelineConfig) -> PipelineConfig {
    cfg.format = crate::shared::infer_format(path);
    cfg
}

/// Writes chunks to the output file in the format implied by the extension.
pub(crate) fn write_output(
    chunks: &[ChunkSpec],
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let ext = Path::new(output)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("json");
    match ext {
        "arrow" => {
            let batch = chunks_to_batch(chunks)?;
            let ipc_bytes = bitvanes_core::arrow_io::ipc::write_ipc_stream(&batch)?;
            if output == "-" {
                use std::io::Write;
                std::io::stdout().write_all(&ipc_bytes)?;
            } else {
                fs::write(output, ipc_bytes)?;
            }
        }
        "csv" => {
            let batch = chunks_to_batch(chunks)?;
            let csv_text = bitvanes_core::arrow_io::csv::write_csv(&batch)?;
            if output == "-" {
                print!("{csv_text}");
            } else {
                fs::write(output, csv_text)?;
            }
        }
        _ => {
            let json_chunks: Vec<JsonChunk> = chunks.iter().map(JsonChunk::from).collect();
            let json = serde_json::to_string_pretty(&json_chunks)?;
            if output == "-" {
                println!("{json}");
            } else {
                fs::write(output, json)?;
            }
        }
    }
    Ok(())
}

fn parse_format(s: &str) -> Result<DocumentFormat, Box<dyn std::error::Error>> {
    match s.to_lowercase().as_str() {
        "markdown" | "md" => Ok(DocumentFormat::Markdown),
        "text" | "txt" => Ok(DocumentFormat::Text),
        "html" | "htm" => Ok(DocumentFormat::Html),
        "json" => Ok(DocumentFormat::Json),
        "pdf" => Ok(DocumentFormat::Pdf),
        other => Err(format!("unknown format: {other}").into()),
    }
}

fn parse_tokenizer(s: &str) -> Result<TokenizerKind, Box<dyn std::error::Error>> {
    serde_json::from_str(&format!("\"{}\"", s.to_lowercase()))
        .map_err(|e| format!("unknown tokenizer '{s}': {e}").into())
}

fn parse_scrub(s: &str) -> ScrubProfile {
    let patterns: Vec<BuiltInPattern> = s
        .split(',')
        .filter_map(|p| serde_json::from_str(&format!("\"{}\"", p.trim())).ok())
        .collect();
    ScrubProfile {
        patterns,
        ..ScrubProfile::default()
    }
}

#[derive(Serialize)]
struct JsonChunk {
    chunk_index: u32,
    text: String,
    token_count: u16,
    source_path: String,
    heading_path: Vec<String>,
    section_kind: String,
}

impl From<&ChunkSpec> for JsonChunk {
    fn from(c: &ChunkSpec) -> Self {
        Self {
            chunk_index: c.chunk_index,
            text: c.text.clone(),
            token_count: c.token_count,
            source_path: c.source_path.clone(),
            heading_path: c.heading_path.clone(),
            section_kind: format!("{:?}", c.section_kind).to_lowercase(),
        }
    }
}

/// Runs the interactive TUI.
pub fn run(cli: &TuiArgs) -> io::Result<()> {
    install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, cli);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        prev(info);
    }));
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, cli: &TuiArgs) -> io::Result<()> {
    let mut app = app::AppState::new(cli);
    loop {
        terminal.draw(|f| ui::draw(f, &app))?;
        app.pump();
        if let Some(key) = event::poll_key(Duration::from_millis(120))? {
            match app.handle_key(key) {
                app::Action::Quit => return Ok(()),
                app::Action::None => {}
            }
        }
    }
}
