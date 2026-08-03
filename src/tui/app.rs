//! Application state and key handling for the TUI.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use crossterm::event::{KeyCode, KeyEvent};

use bitvanes_core::{
    ChunkSpec, PipelineConfig, chunk::chunk_document, parse::parse_bytes, pii::scrub_document,
};

use super::TuiArgs;

/// Which screen is currently displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    FileBrowser,
    Config,
    Results,
    Help,
}

/// Actions returned by key handling.
pub enum Action {
    Quit,
    None,
}

/// A status message shown to the user, coloured by kind.
#[derive(Debug, Clone)]
pub enum Status {
    Success(String),
    Error(String),
}

/// Result delivered by the background processing thread.
struct ProcessOutcome {
    chunks: Vec<ChunkSpec>,
    error: Option<String>,
}

/// Top-level TUI application state.
pub struct AppState {
    pub screen: Screen,
    prev_screen: Screen,
    pub current_dir: PathBuf,
    pub dir_entries: Vec<PathBuf>,
    pub cursor: usize,
    pub selected_files: Vec<PathBuf>,
    pub config: PipelineConfig,
    pub chunks: Vec<ChunkSpec>,
    pub status: Option<Status>,
    pub scroll: usize,
    pub processing: bool,
    pub output_path: String,
    pub editing_path: bool,
    /// Spinner frame counter, advanced each render tick.
    pub tick: usize,
    result_rx: Option<Receiver<ProcessOutcome>>,
    saved_path: String,
}

impl AppState {
    pub fn new(cli: &TuiArgs) -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Wire CLI flags / config file into the pipeline config so the TUI
        // honours `--format`, `--tokenizer`, `--max-tokens`, `--scrub`,
        // and `--config` just like headless mode.
        let (config, status) = match super::build_config(cli) {
            Ok(c) => (c, None),
            Err(e) => (
                PipelineConfig::default(),
                Some(Status::Error(format!(
                    "config load failed ({e}); using defaults"
                ))),
            ),
        };

        let output_path = cli
            .output
            .clone()
            .unwrap_or_else(|| "output.json".to_string());

        let mut state = Self {
            screen: Screen::FileBrowser,
            prev_screen: Screen::FileBrowser,
            dir_entries: Vec::new(),
            cursor: 0,
            selected_files: Vec::new(),
            config,
            chunks: Vec::new(),
            status,
            scroll: 0,
            processing: false,
            output_path: output_path.clone(),
            editing_path: false,
            tick: 0,
            result_rx: None,
            saved_path: output_path,
            current_dir,
        };
        state.refresh_dir();
        state
    }

    /// Reload directory entries from `current_dir`.
    pub fn refresh_dir(&mut self) {
        let mut entries: Vec<PathBuf> = fs::read_dir(&self.current_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();

        // Sort: directories first, then alphabetical.
        entries.sort_by(|a, b| {
            let a_dir = a.is_dir();
            let b_dir = b.is_dir();
            b_dir
                .cmp(&a_dir)
                .then_with(|| a.file_name().cmp(&b.file_name()))
        });

        // Prepend parent dir entry if we're not at root.
        if self.current_dir.parent().is_some() {
            entries.insert(0, self.current_dir.join(".."));
        }

        self.dir_entries = entries;
        self.cursor = 0;
    }

    /// Returns the path currently under the cursor, if any.
    fn current_entry(&self) -> Option<&PathBuf> {
        self.dir_entries.get(self.cursor)
    }

    /// Returns true if the file at `path` is in the selected list.
    pub fn is_selected(&self, path: &PathBuf) -> bool {
        self.selected_files.contains(path)
    }

    /// Adds or removes `path` from the selection list.
    fn toggle_selection(&mut self, path: &PathBuf) {
        if let Some(pos) = self.selected_files.iter().position(|p| p == path) {
            self.selected_files.remove(pos);
        } else {
            self.selected_files.push(path.clone());
        }
    }

    /// Processes all selected files on a background thread so the UI stays
    /// responsive. Results arrive asynchronously via [`pump`].
    pub fn start_processing(&mut self) {
        if self.processing {
            return;
        }
        let paths: Vec<PathBuf> = self
            .selected_files
            .iter()
            .filter(|p| p.is_file())
            .cloned()
            .collect();
        if paths.is_empty() {
            self.status = Some(Status::Error(
                "no files selected — use Space in the file browser first".to_string(),
            ));
            return;
        }
        self.processing = true;
        self.chunks.clear();
        self.status = None;
        self.scroll = 0;
        self.screen = Screen::Results;

        let config = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.result_rx = Some(rx);
        std::thread::spawn(move || {
            let outcome = run_pipeline_for_files(&paths, &config);
            let _ = tx.send(outcome);
        });
    }

    /// Absorbs a completed background result, if one has arrived. Called
    /// once per render tick by the main loop.
    pub fn pump(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        let Some(rx) = &self.result_rx else { return };
        match rx.try_recv() {
            Ok(outcome) => {
                let count = outcome.chunks.len();
                self.processing = false;
                self.result_rx = None;
                self.chunks = outcome.chunks;
                self.scroll = 0;
                self.status = Some(match outcome.error {
                    Some(e) => Status::Error(e),
                    None => Status::Success(format!("Processed {count} chunks")),
                });
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.processing = false;
                self.result_rx = None;
                self.status = Some(Status::Error(
                    "processing thread terminated unexpectedly".to_string(),
                ));
            }
        }
    }

    /// Handle a key press. Returns an action for the main loop.
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Path-edit mode swallows everything until confirmed/cancelled.
        if self.editing_path {
            return self.handle_path_edit_key(key);
        }
        // The Help overlay is global and closes on any key.
        if key.code == KeyCode::Char('?') {
            self.toggle_help();
            return Action::None;
        }
        if self.screen == Screen::Help {
            self.screen = self.prev_screen;
            return Action::None;
        }
        match self.screen {
            Screen::FileBrowser => self.handle_browser_key(key),
            Screen::Config => self.handle_config_key(key),
            Screen::Results => self.handle_results_key(key),
            Screen::Help => Action::None, // handled above; unreachable
        }
    }

    fn toggle_help(&mut self) {
        if self.screen == Screen::Help {
            self.screen = self.prev_screen;
        } else {
            self.prev_screen = self.screen;
            self.screen = Screen::Help;
        }
    }

    fn handle_path_edit_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Enter => {
                self.editing_path = false;
                self.status = None;
            }
            KeyCode::Esc => {
                self.editing_path = false;
                self.output_path = self.saved_path.clone();
                self.status = None;
            }
            KeyCode::Backspace => {
                self.output_path.pop();
            }
            KeyCode::Char(c) => self.output_path.push(c),
            _ => {}
        }
        Action::None
    }

    fn handle_browser_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(self.dir_entries.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                if let Some(path) = self.current_entry() {
                    let path = path.clone();
                    if path.is_dir() || path.ends_with("..") {
                        if path.ends_with("..") {
                            if let Some(parent) = self.current_dir.parent() {
                                self.current_dir = parent.to_path_buf();
                            }
                        } else {
                            self.current_dir = path;
                        }
                        self.refresh_dir();
                    } else if path.is_file() {
                        self.toggle_selection(&path);
                    }
                }
            }
            KeyCode::Char(' ') => {
                if let Some(path) = self.current_entry() {
                    let path = path.clone();
                    if path.is_file() {
                        self.toggle_selection(&path);
                    }
                }
            }
            KeyCode::Tab => self.screen = Screen::Config,
            _ => {}
        }
        Action::None
    }

    fn handle_config_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::BackTab | KeyCode::Char('b') => self.screen = Screen::FileBrowser,
            KeyCode::Tab => self.screen = Screen::Results,
            KeyCode::Char('m') => {
                let mt = self.config.chunk.max_tokens;
                self.config.chunk.max_tokens = match mt {
                    0..=128 => 256,
                    129..=256 => 512,
                    257..=512 => 1024,
                    _ => 128,
                };
            }
            KeyCode::Char('t') => {
                self.config.chunk.tokenizer = match self.config.chunk.tokenizer {
                    bitvanes_core::TokenizerKind::Cl100kBase => {
                        bitvanes_core::TokenizerKind::O200kBase
                    }
                    bitvanes_core::TokenizerKind::O200kBase => {
                        bitvanes_core::TokenizerKind::R50kBase
                    }
                    bitvanes_core::TokenizerKind::R50kBase => {
                        bitvanes_core::TokenizerKind::P50kBase
                    }
                    bitvanes_core::TokenizerKind::P50kBase => {
                        bitvanes_core::TokenizerKind::P50kEdit
                    }
                    bitvanes_core::TokenizerKind::P50kEdit => {
                        bitvanes_core::TokenizerKind::O200kHarmony
                    }
                    bitvanes_core::TokenizerKind::O200kHarmony => {
                        bitvanes_core::TokenizerKind::Cl100kBase
                    }
                };
            }
            KeyCode::Char('e') => {
                toggle_pattern(
                    &mut self.config.scrub.patterns,
                    bitvanes_core::BuiltInPattern::Email,
                );
            }
            KeyCode::Char('s') => {
                toggle_pattern(
                    &mut self.config.scrub.patterns,
                    bitvanes_core::BuiltInPattern::Ssn,
                );
            }
            KeyCode::Char('a') => {
                toggle_pattern(
                    &mut self.config.scrub.patterns,
                    bitvanes_core::BuiltInPattern::AwsKey,
                );
            }
            KeyCode::Enter if !self.selected_files.is_empty() => self.start_processing(),
            _ => {}
        }
        Action::None
    }

    fn handle_results_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = (self.scroll + 1).min(self.chunks.len().saturating_sub(1));
            }
            KeyCode::Tab => self.screen = Screen::Config,
            KeyCode::BackTab | KeyCode::Char('b') => self.screen = Screen::FileBrowser,
            KeyCode::Char('e') if !self.processing => {
                self.editing_path = true;
                self.saved_path = self.output_path.clone();
                self.status = None;
            }
            KeyCode::Char('s') if !self.processing => self.save(),
            _ => {}
        }
        Action::None
    }

    fn save(&mut self) {
        if self.chunks.is_empty() {
            self.status = Some(Status::Error("nothing to save — no chunks".to_string()));
            return;
        }
        match super::write_output(&self.chunks, &self.output_path) {
            Ok(()) => {
                self.status = Some(Status::Success(format!(
                    "Saved {} chunks to {}",
                    self.chunks.len(),
                    self.output_path
                )))
            }
            Err(e) => self.status = Some(Status::Error(format!("save failed: {e}"))),
        }
    }

    /// Inferred output format from the current output path extension.
    pub fn output_format_label(&self) -> &'static str {
        match std::path::Path::new(&self.output_path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref()
        {
            Some("arrow") => "Arrow IPC",
            Some("csv") => "CSV",
            _ => "JSON",
        }
    }
}

fn toggle_pattern(
    patterns: &mut Vec<bitvanes_core::BuiltInPattern>,
    pat: bitvanes_core::BuiltInPattern,
) {
    if let Some(pos) = patterns.iter().position(|&p| p == pat) {
        patterns.remove(pos);
    } else {
        patterns.push(pat);
    }
}

/// Runs the pipeline over the selected files. Runs on a worker thread.
fn run_pipeline_for_files(paths: &[PathBuf], base_config: &PipelineConfig) -> ProcessOutcome {
    let mut chunks = Vec::new();
    let mut error: Option<String> = None;

    for path in paths {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                error = Some(format!("could not read {}: {e}", path.display()));
                continue;
            }
        };

        let cfg = PipelineConfig {
            source_label: Some(path.display().to_string()),
            ..base_config.clone()
        };
        let cfg = super::infer_format(path, cfg);

        match parse_bytes(&bytes, &cfg)
            .and_then(|doc| scrub_document(doc, &cfg.scrub).map(|(d, _, _)| d))
            .and_then(|doc| chunk_document(&doc, &cfg.chunk, cfg.source_label.as_deref()))
        {
            Ok(c) => chunks.extend(c),
            Err(e) => error = Some(format!("failed {}: {e}", path.display())),
        }
    }

    // Renumber chunk indices sequentially across all files.
    for (i, c) in chunks.iter_mut().enumerate() {
        c.chunk_index = i as u32;
    }

    ProcessOutcome { chunks, error }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn cli_with(args: &[&str]) -> TuiArgs {
        let mut full = vec!["bitvanes", "tui"];
        full.extend_from_slice(args);
        match crate::Cli::parse_from(full).command {
            Some(crate::Command::Tui(a)) => a,
            other => panic!("expected `tui` subcommand, got {other:?}"),
        }
    }

    fn write_temp_file(content: &str, ext: &str) -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = PathBuf::from(format!("/tmp/bitvanes-test-{id}.{ext}"));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn output_format_label_infers_from_extension() {
        let mut app = AppState::new(&cli_with(&[]));
        for (path, expected) in [
            ("out.json", "JSON"),
            ("out.csv", "CSV"),
            ("out.arrow", "Arrow IPC"),
            ("out", "JSON"),
            ("out.txt", "JSON"),
        ] {
            app.output_path = path.to_string();
            assert_eq!(app.output_format_label(), expected, "for path {path}");
        }
    }

    #[test]
    fn flags_wire_into_tui_config() {
        let app = AppState::new(&cli_with(&[
            "--format",
            "text",
            "--max-tokens",
            "128",
            "--tokenizer",
            "o200k_base",
        ]));
        assert_eq!(app.config.format, bitvanes_core::DocumentFormat::Text);
        assert_eq!(app.config.chunk.max_tokens, 128);
        assert_eq!(
            app.config.chunk.tokenizer,
            bitvanes_core::TokenizerKind::O200kBase
        );
    }

    #[test]
    fn run_pipeline_for_files_processes_markdown() {
        let path = write_temp_file("# Title\n\nHello world document.\n", "md");
        let cfg = PipelineConfig::default();
        let outcome = run_pipeline_for_files(&[path], &cfg);
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert!(!outcome.chunks.is_empty());
        assert!(outcome.chunks[0].text.contains("Hello world"));
    }

    #[test]
    fn save_writes_json_and_reports_success() {
        let mut app = AppState::new(&cli_with(&[]));
        app.chunks = vec![ChunkSpec {
            chunk_index: 0,
            chunk_id: String::new(),
            text: "sample".to_string(),
            token_count: 1,
            source_path: "t.md".to_string(),
            heading_path: vec![],
            section_kind: bitvanes_core::SectionKind::Paragraph,
            char_offset_start: 0,
            char_offset_end: 6,
            pii: vec![],
        }];
        let out = format!(
            "/tmp/bitvanes-save-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        app.output_path = out.clone();
        app.save();
        match &app.status {
            Some(Status::Success(m)) => assert!(m.contains("Saved 1 chunks"), "{m}"),
            other => panic!("expected success, got {other:?}"),
        }
        assert!(std::fs::read_to_string(&out).unwrap().contains("sample"));
    }
}
