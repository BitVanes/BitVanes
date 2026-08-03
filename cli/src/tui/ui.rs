//! Ratatui rendering for each screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table};

use bitvanes_core::BuiltInPattern;

use super::app::{AppState, Screen, Status};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Main draw entry point — dispatches to the current screen.
pub fn draw(f: &mut Frame, app: &AppState) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title bar
            Constraint::Min(1),    // content
            Constraint::Length(1), // status bar
        ])
        .split(area);

    draw_title_bar(f, chunks[0]);
    match app.screen {
        Screen::FileBrowser => draw_file_browser(f, chunks[1], app),
        Screen::Config => draw_config(f, chunks[1], app),
        Screen::Results => draw_results(f, chunks[1], app),
        Screen::Help => draw_help(f, chunks[1]),
    }
    draw_status_bar(f, chunks[2], app);
}

// -----------------------------------------------------------------------
// Title + Status bars
// -----------------------------------------------------------------------

fn draw_title_bar(f: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " BitVanes ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("│ "),
        Span::styled(
            "Zero-Trust PII Purification",
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(Block::default().borders(Borders::TOP));
    f.render_widget(title, area);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &AppState) {
    // Surface status messages on the browser/config screens in the status bar.
    if !matches!(app.screen, Screen::Results | Screen::Help) {
        if let Some(status) = &app.status {
            let (msg, color) = status_parts(status);
            f.render_widget(
                Paragraph::new(format!(" {msg}")).style(Style::default().fg(color)),
                area,
            );
            return;
        }
    }

    let hints = match app.screen {
        Screen::FileBrowser => {
            "↑↓/jk navigate · Enter open/select · Space select · Tab config · ? help · q quit"
        }
        Screen::Config => {
            "m tokens · t tokenizer · e email · s ssn · a aws · Enter process · Tab results · ? help · q quit"
        }
        Screen::Results => {
            "↑↓/jk scroll · s save · e edit output · Tab config · b browser · ? help · q quit"
        }
        Screen::Help => "any key closes help · q quit",
    };

    let bar = Paragraph::new(format!(" {hints}")).style(Style::default().fg(Color::DarkGray));
    f.render_widget(bar, area);
}

// -----------------------------------------------------------------------
// File Browser
// -----------------------------------------------------------------------

fn draw_file_browser(f: &mut Frame, area: Rect, app: &AppState) {
    let entries_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);

    let path_text = format!(" 📁 {}", app.current_dir.display());
    let header = Paragraph::new(path_text).style(Style::default().fg(Color::Yellow));
    f.render_widget(header, entries_area[0]);

    let items: Vec<ListItem> = app
        .dir_entries
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

            let is_dir = path.is_dir() || name == "..";
            let is_selected = app.is_selected(path);

            let prefix = if is_selected { "✓ " } else { "  " };
            let icon = if name == ".." {
                "↑ .."
            } else if is_dir {
                "📁"
            } else {
                "📄"
            };

            let label = format!("{prefix}{icon} {name}");

            let style = if i == app.cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if is_dir {
                Style::default().fg(Color::Blue)
            } else if is_selected {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(Span::styled(label, style)))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Files ({} selected) ", app.selected_files.len())),
    );
    f.render_widget(list, entries_area[1]);
}

// -----------------------------------------------------------------------
// Config Editor
// -----------------------------------------------------------------------

fn draw_config(f: &mut Frame, area: Rect, app: &AppState) {
    let lines = vec![
        Line::from(""),
        styled_line("Pipeline Configuration", Color::Yellow, true),
        Line::from(""),
        kv_line("Format", format!("{:?}", app.config.format)),
        kv_line("Tokenizer", format!("{:?}", app.config.chunk.tokenizer)),
        kv_line("Max Tokens", app.config.chunk.max_tokens.to_string()),
        Line::from(""),
        styled_line("PII Scrubbing", Color::Yellow, true),
        pattern_line(
            "Email",
            app.config.scrub.patterns.contains(&BuiltInPattern::Email),
        ),
        pattern_line(
            "SSN",
            app.config.scrub.patterns.contains(&BuiltInPattern::Ssn),
        ),
        pattern_line(
            "AWS Key",
            app.config.scrub.patterns.contains(&BuiltInPattern::AwsKey),
        ),
        Line::from(""),
        styled_line("Selected Files", Color::Yellow, true),
        Line::from(format!(
            "  {} files ready to process",
            app.selected_files.len()
        )),
        Line::from(""),
        if app.selected_files.is_empty() {
            Line::from(Span::styled(
                "  ⚠ No files selected — go back with 'b' and press Space to select files",
                Style::default().fg(Color::Red),
            ))
        } else {
            Line::from(Span::styled(
                "  ► Press Enter to process ◄",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ))
        },
    ];

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Configuration "),
    );
    f.render_widget(para, area);
}

// -----------------------------------------------------------------------
// Results
// -----------------------------------------------------------------------

fn draw_results(f: &mut Frame, area: Rect, app: &AppState) {
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // stats
            Constraint::Min(1),    // table / spinner
            Constraint::Length(4), // output + status footer
        ])
        .split(area);

    // Stats bar.
    let total_tokens: u64 = app.chunks.iter().map(|c| c.token_count as u64).sum();
    let avg = if app.chunks.is_empty() {
        0
    } else {
        total_tokens / app.chunks.len() as u64
    };
    let stats = format!(
        " {} chunks · {} tokens · {} avg tokens/chunk ",
        app.chunks.len(),
        total_tokens,
        avg,
    );
    let stats_widget = Paragraph::new(stats).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(stats_widget, content[0]);

    if app.processing {
        let frame = SPINNER[app.tick % SPINNER.len()];
        let n = app.selected_files.iter().filter(|p| p.is_file()).count();
        let para = Paragraph::new(format!("\n {frame}  Processing {n} file(s)…"))
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(para, content[1]);
    } else {
        draw_chunk_table(f, content[1], app);
    }

    draw_output_footer(f, content[2], app);
}

fn draw_chunk_table(f: &mut Frame, area: Rect, app: &AppState) {
    let rows: Vec<Row> = app
        .chunks
        .iter()
        .skip(app.scroll)
        .take(area.height.saturating_sub(3) as usize)
        .map(|c| {
            let text = truncate_chars(&c.text, 60);
            let heading = if c.heading_path.is_empty() {
                "—".to_string()
            } else {
                c.heading_path.join(" › ")
            };
            Row::new(vec![
                Cell::from(c.chunk_index.to_string()),
                Cell::from(text),
                Cell::from(c.token_count.to_string()),
                Cell::from(truncate_chars(&heading, 30)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Min(20),
            Constraint::Length(8),
            Constraint::Length(30),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("#"),
            Cell::from("Text"),
            Cell::from("Tokens"),
            Cell::from("Heading"),
        ])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title(format!(
        " Chunks (showing {}/{}) ",
        app.scroll.min(app.chunks.len()),
        app.chunks.len()
    )));

    f.render_widget(table, area);
}

fn draw_output_footer(f: &mut Frame, area: Rect, app: &AppState) {
    let footer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(2)])
        .split(area);

    // Output path + detected format (with an edit-mode hint).
    let path_line = if app.editing_path {
        Line::from(vec![
            Span::styled(" Save to: ", Style::default().fg(Color::DarkGray)),
            Span::raw(app.output_path.clone()),
            Span::styled("▏", Style::default().fg(Color::Cyan)),
            Span::styled(
                "  ✏ Enter to confirm · Esc to cancel",
                Style::default().fg(Color::Cyan),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Save to: ", Style::default().fg(Color::DarkGray)),
            Span::raw(app.output_path.clone()),
            Span::styled(
                format!("  [{}]", app.output_format_label()),
                Style::default().fg(Color::Blue),
            ),
            Span::styled(
                "  (press 'e' to edit)",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };
    f.render_widget(Paragraph::new(path_line), footer[0]);

    // Status message, coloured by kind (never an overlay on the table).
    if let Some(status) = &app.status {
        draw_status_line(f, footer[1], status);
    }
}

// -----------------------------------------------------------------------
// Help
// -----------------------------------------------------------------------

fn draw_help(f: &mut Frame, area: Rect) {
    let lines = vec![
        styled_line("Help — Keybindings", Color::Yellow, true),
        Line::from(""),
        styled_line("File Browser", Color::Cyan, false),
        Line::from("   ↑↓ / j k   move cursor"),
        Line::from("   Enter      open directory / toggle file selection"),
        Line::from("   Space      toggle file selection"),
        Line::from("   Tab        go to Configuration"),
        Line::from(""),
        styled_line("Configuration", Color::Cyan, false),
        Line::from("   m          cycle max tokens (128→256→512→1024)"),
        Line::from("   t          cycle tokenizer"),
        Line::from("   e / s / a  toggle Email / SSN / AWS-key scrubbing"),
        Line::from("   Enter      process selected files"),
        Line::from("   Tab        go to Results    b  back to browser"),
        Line::from(""),
        styled_line("Results", Color::Cyan, false),
        Line::from("   ↑↓ / j k   scroll chunks"),
        Line::from("   s          save to the output path (format from extension)"),
        Line::from("   e          edit the output path"),
        Line::from("   Tab        go to Configuration    b  back to browser"),
        Line::from(""),
        styled_line("Anywhere", Color::Cyan, false),
        Line::from("   ?          toggle this help    q / Esc  quit"),
        Line::from(""),
        Line::from(Span::styled(
            " Output format is chosen by the path extension: .json (default), .csv, .arrow",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Help — press any key to close "),
    );
    f.render_widget(para, area);
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Truncates `s` to at most `max_chars` Unicode chars and appends an
/// ellipsis. Char-based, so it never panics on multi-byte boundaries.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// Returns the `(text, color)` for a status message.
fn status_parts(status: &Status) -> (String, Color) {
    match status {
        Status::Success(m) => (format!("✓ {m}"), Color::Green),
        Status::Error(m) => (format!("⚠ {m}"), Color::Red),
    }
}

/// Renders a status message in its own sub-area, coloured by kind.
fn draw_status_line(f: &mut Frame, area: Rect, status: &Status) {
    let (text, color) = status_parts(status);
    f.render_widget(Paragraph::new(text).style(Style::default().fg(color)), area);
}

fn styled_line(text: &str, color: Color, bold: bool) -> Line<'_> {
    let mut style = Style::default().fg(color);
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    Line::from(Span::styled(format!(" {text}"), style))
}

fn kv_line(key: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {key:<16} "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(value),
    ])
}

fn pattern_line(name: &str, enabled: bool) -> Line<'_> {
    let check = if enabled { "✓" } else { "✗" };
    let color = if enabled {
        Color::Green
    } else {
        Color::DarkGray
    };
    Line::from(Span::styled(
        format!("  [{check}] {name}"),
        Style::default().fg(color),
    ))
}
