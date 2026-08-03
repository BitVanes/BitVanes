//! Arrow CSV output (native-only).
//!
//! Serializes a [`RecordBatch`] into CSV text for human inspection, data
//! export, or ingestion into tools that prefer flat tabular data.
//!
//! **Limitation:** Arrow's CSV writer does not support nested types
//! (`List`, `FixedSizeList`). The `heading_path` and `pii_metadata` columns
//! are dropped from CSV output. Use IPC format ([`crate::arrow_io::ipc`])
//! for a complete column-preserving serialization.
//!
//! Enabled behind the `csv` cargo feature.

use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::csv::WriterBuilder;
use arrow::datatypes::{DataType, Schema};

use crate::error::{BitVanesError, Result};

/// Serializes `batch` into CSV text (header row + data rows).
///
/// Nested columns (`heading_path`, `pii_metadata`) are excluded — see the
/// module-level limitation note.
///
/// # Errors
///
/// Returns [`crate::error::BitVanesError::Arrow`] if serialization fails.
pub fn write_csv(batch: &RecordBatch) -> Result<String> {
    let flat = flatten_for_csv(batch)?;
    let mut buffer = Vec::new();
    {
        let mut writer = WriterBuilder::new().build(&mut buffer);
        writer.write(&flat)?;
    }
    String::from_utf8(buffer)
        .map_err(|e| BitVanesError::InvalidInput(format!("CSV output is not valid UTF-8: {e}")))
}

/// Serializes `batch` into CSV text WITHOUT a header row.
///
/// # Errors
///
/// See [`write_csv`].
pub fn write_csv_no_header(batch: &RecordBatch) -> Result<String> {
    let flat = flatten_for_csv(batch)?;
    let mut buffer = Vec::new();
    {
        let mut writer = WriterBuilder::new().with_header(false).build(&mut buffer);
        writer.write(&flat)?;
    }
    String::from_utf8(buffer)
        .map_err(|e| BitVanesError::InvalidInput(format!("CSV output is not valid UTF-8: {e}")))
}

/// Builds a CSV-compatible `RecordBatch` by excluding nested-type columns
/// that Arrow's CSV writer cannot handle (`List`, `FixedSizeList`).
fn flatten_for_csv(batch: &RecordBatch) -> Result<RecordBatch> {
    let schema = batch.schema();
    let mut fields = Vec::new();
    let mut columns = Vec::new();

    for (i, field) in schema.fields().iter().enumerate() {
        if is_csv_compatible(field.data_type()) {
            fields.push(field.clone());
            columns.push(batch.column(i).clone());
        }
    }

    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(fields)),
        columns,
    )?)
}

/// Returns `true` if Arrow's CSV writer can serialize this data type.
const fn is_csv_compatible(dt: &DataType) -> bool {
    !matches!(
        dt,
        DataType::List(_) | DataType::FixedSizeList(_, _) | DataType::LargeList(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_io::batch::chunks_to_batch;
    use crate::schema::{ChunkSpec, SectionKind};

    fn sample_batch() -> RecordBatch {
        let chunks = vec![
            ChunkSpec {
                chunk_index: 0,
                chunk_id: "a".to_string(),
                text: "Hello world.".to_string(),
                token_count: 3,
                source_path: "test.md".to_string(),
                heading_path: vec!["Title".to_string()],
                section_kind: SectionKind::Paragraph,
                char_offset_start: 0,
                char_offset_end: 12,
                pii: vec![],
            },
            ChunkSpec {
                chunk_index: 1,
                chunk_id: "b".to_string(),
                text: "Second chunk.".to_string(),
                token_count: 2,
                source_path: "test.md".to_string(),
                heading_path: vec![],
                section_kind: SectionKind::Code,
                char_offset_start: 12,
                char_offset_end: 25,
                pii: vec![],
            },
        ];
        chunks_to_batch(&chunks).expect("batch should build")
    }

    #[test]
    fn csv_output_has_header_and_data() {
        let batch = sample_batch();
        let csv = write_csv(&batch).expect("CSV write");
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines.len() >= 3, "expected header + 2 data rows");

        // Header must contain our column names.
        let header = lines[0];
        assert!(header.contains("chunk_index"), "header: {header}");
        assert!(header.contains("text"), "header: {header}");
        assert!(header.contains("token_count"), "header: {header}");
    }

    #[test]
    fn csv_data_rows_contain_text_values() {
        let batch = sample_batch();
        let csv = write_csv(&batch).expect("CSV write");
        assert!(csv.contains("Hello world."), "CSV must contain chunk text");
        assert!(csv.contains("Second chunk."), "CSV must contain chunk text");
        assert!(csv.contains("test.md"), "CSV must contain source_path");
    }

    #[test]
    fn csv_no_header_omits_first_line() {
        let batch = sample_batch();
        let with_header = write_csv(&batch).unwrap();
        let without_header = write_csv_no_header(&batch).unwrap();

        let header_lines = with_header.lines().count();
        let no_header_lines = without_header.lines().count();
        assert_eq!(
            header_lines,
            no_header_lines + 1,
            "no-header version should have exactly one fewer line"
        );
    }
}
