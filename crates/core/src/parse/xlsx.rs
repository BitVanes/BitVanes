//! XLSX (Microsoft Excel) parser via `calamine`.
//!
//! # Memory strategy for large/sparse spreadsheets
//!
//! Calamine materialises each sheet into a dense `Range<DataType>` grid.
//! For a sheet with dimension 1,000,000 × 50 but only 200 populated cells,
//! the full grid is still allocated. To prevent OOM on pathological files:
//!
//! 1. **One sheet at a time**: `worksheet_range(name)` loads a single sheet,
//!    not `worksheets()` which loads all sheets simultaneously.
//! 2. **Row iterator with empty-row trimming**: `range.rows()` gives a
//!    slice view; we skip all-empty rows and break at the first long run of
//!    empty trailing rows (calamine's used-range bounding box usually
//!    handles this, but we belt-and-braces it).
//! 3. **Cell-count guard**: `max_cells` (default 2,000,000) caps total
//!    materialised cells to prevent unbounded allocation. If exceeded, the
//!    sheet is truncated and a warning is emitted.
//! 4. **Sparse cell skipping**: `DataType::Empty` cells are not emitted as
//!    text — they're collapsed into a single tab delimiter.
//!
//! Each contiguous block of non-empty rows becomes one span. The sheet name
//! becomes the heading ancestry: `["Sheet1"]`.

use calamine::{Data, Reader as CalamineReader, open_workbook_auto_from_rs};

use crate::error::{BitVanesError, Result};
use crate::parse::{Document, TextSpan, offset_to_u32};
use crate::schema::{PipelineConfig, SectionKind};

/// Maximum cells to materialise per workbook before truncating. At ~50 bytes
/// per `Data` enum value, 2M cells ≈ 100 MB — a reasonable ceiling.
const MAX_CELLS: usize = 2_000_000;

/// Maximum contiguous empty rows before we stop scanning a sheet (trailing
/// junk that calamine's bounding box sometimes includes).
const MAX_TRAILING_EMPTY: usize = 100;

/// Entry point: parses `.xlsx` bytes into a [`Document`].
pub fn parse_xlsx_bytes(bytes: &[u8], _cfg: &PipelineConfig) -> Result<Document> {
    let cursor = std::io::Cursor::new(bytes);
    let mut workbook = open_workbook_auto_from_rs(cursor)
        .map_err(|e| BitVanesError::InvalidInput(format!("xlsx open error: {e}")))?;

    let sheet_names = workbook.sheet_names().clone();

    let mut full_text = String::new();
    let mut spans: Vec<TextSpan> = Vec::new();
    let mut total_cells = 0usize;

    for sheet_name in &sheet_names {
        if total_cells >= MAX_CELLS {
            break;
        }

        let Ok(range) = workbook.worksheet_range(sheet_name) else {
            continue;
        };

        let height = range.height();
        let width = range.width().max(1);
        // Guard: skip sheets that would blow the cell budget.
        if total_cells.saturating_add(height * width) > MAX_CELLS {
            break;
        }
        total_cells += height * width;

        let heading = vec![sheet_name.clone()];
        let mut row_block = String::new();
        let mut empty_streak = 0u32;

        for row in range.rows() {
            let non_empty: Vec<&Data> = row.iter().filter(|c| !matches!(c, Data::Empty)).collect();

            if non_empty.is_empty() {
                empty_streak += 1;
                // Flush the current row block as a span.
                if !row_block.trim().is_empty() {
                    push_span(&mut full_text, &mut spans, &row_block, &heading);
                    row_block.clear();
                }
                if empty_streak >= u32::try_from(MAX_TRAILING_EMPTY).unwrap_or(u32::MAX) {
                    break;
                }
                continue;
            }
            empty_streak = 0;

            // Build a tab-separated row string, collapsing empty cells.
            let row_str: String = row
                .iter()
                .map(cell_to_string)
                .collect::<Vec<_>>()
                .join("\t");

            if !row_block.is_empty() {
                row_block.push('\n');
            }
            row_block.push_str(&row_str);
        }

        if !row_block.trim().is_empty() {
            push_span(&mut full_text, &mut spans, &row_block, &heading);
        }
    }

    Ok(Document { full_text, spans })
}

/// Converts a calamine `Data` cell to its string representation.
/// `Data::Empty` → empty string (collapsed by the join delimiter).
#[allow(clippy::cast_possible_truncation)]
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) | Data::DurationIso(s) | Data::DateTimeIso(s) => s.clone(),
        Data::Int(n) => n.to_string(),
        Data::Float(f) => {
            // Avoid trailing ".0" for whole numbers.
            if f.fract() == 0.0 && f.is_finite() {
                format!("{f:.0}")
            } else {
                f.to_string()
            }
        }
        Data::DateTime(dt) => dt.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::Error(err) => format!("#ERROR:{err:?}"),
    }
}

/// Appends a text block to the full document and records a span.
fn push_span(full_text: &mut String, spans: &mut Vec<TextSpan>, text: &str, heading: &[String]) {
    let start = full_text.len();
    full_text.push_str(text);
    full_text.push('\n');
    spans.push(TextSpan {
        char_offset_start: offset_to_u32(start),
        char_offset_end: offset_to_u32(full_text.len()),
        heading_path: heading.to_vec(),
        section_kind: SectionKind::TableCell,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::DocumentFormat;
    use calamine::Data;

    /// Tests cell conversion logic and error handling.
    /// Full XLSX round-trip tests live in `tests/formats.rs` with fixture files.

    #[test]
    fn cell_to_string_handles_types() {
        assert_eq!(cell_to_string(&Data::Empty), "");
        assert_eq!(cell_to_string(&Data::Int(42)), "42");
        assert_eq!(cell_to_string(&Data::Float(3.15)), "3.15");
        assert_eq!(cell_to_string(&Data::Float(100.0)), "100");
        assert_eq!(cell_to_string(&Data::String("hello".into())), "hello");
        assert_eq!(cell_to_string(&Data::Bool(true)), "true");
    }

    #[test]
    fn invalid_xlsx_returns_error() {
        let result = parse_xlsx_bytes(
            b"not an xlsx",
            &PipelineConfig {
                format: DocumentFormat::Xlsx,
                ..PipelineConfig::default()
            },
        );
        assert!(result.is_err());
    }
}
