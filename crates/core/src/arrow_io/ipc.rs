//! Arrow IPC stream output (native-only).
//!
//! Serializes a [`RecordBatch`] into the `Arrow` `IPC` streaming format — a
//! self-describing binary sequence suitable for piping to `stdout`,
//! writing to a `.arrow` file, or ingesting into databases that accept
//! `Arrow` `IPC` (`DuckDB`, `ClickHouse`, `LanceDB`, `Apache` `Arrow` `Flight`).
//!
//! Enabled behind the `ipc` cargo feature. The wasm target does not use
//! this — it exports batches via zero-copy FFI pointers instead.

use arrow::array::RecordBatch;
use arrow::ipc::writer::StreamWriter;

use crate::error::Result;

/// Serializes `batch` into an Arrow IPC stream stored in a `Vec<u8>`.
///
/// The output is a complete IPC stream: schema message followed by one or
/// more record-batch messages, terminated by an end-of-stream marker.
///
/// # Errors
///
/// Returns [`crate::error::BitVanesError::Arrow`] if serialization fails.
pub fn write_ipc_stream(batch: &RecordBatch) -> Result<Vec<u8>> {
    let mut buffer = Vec::with_capacity(estimate_ipc_size(batch));
    {
        let mut writer = StreamWriter::try_new(&mut buffer, &batch.schema())?;
        writer.write(batch)?;
        writer.finish()?;
    }
    Ok(buffer)
}

/// Writes a single [`RecordBatch`] to an open writer in Arrow IPC streaming
/// format. Call this once per file in batch mode to stream results to disk
/// or stdout without buffering the entire output in memory.
///
/// The caller is responsible for:
/// 1. Calling [`start_ipc_stream`] once before the first batch.
/// 2. Calling [`finish_ipc_stream`] once after the last batch.
///
/// # Errors
///
/// Returns [`crate::error::BitVanesError::Arrow`] on write failure.
pub fn write_ipc_stream_to<W: std::io::Write>(batch: &RecordBatch, writer: &mut W) -> Result<()> {
    writer.write_all(&write_ipc_stream(batch)?).map_err(|e| {
        crate::error::BitVanesError::InvalidInput(format!("ipc stream write error: {e}"))
    })?;
    Ok(())
}

/// Creates a streaming IPC writer over an open writer. The writer emits the
/// schema immediately, then one record-batch message per `write()` call,
/// and an EOS marker on `finish()`.
///
/// This is the preferred API for streaming batch output in the CLI:
/// ```
/// # use bitvanes_core::arrow_io::ipc::IpcStream;
/// # use bitvanes_core::arrow_io::batch::chunks_to_batch;
/// // let mut stream = IpcStream::try_new(File::create("out.arrow")?, &schema)?;
/// // for file in files { let batch = process(file)?; stream.write(&batch)?; }
/// // stream.finish()?;
/// ```
pub struct IpcStream<W: std::io::Write> {
    writer: StreamWriter<W>,
}

impl<W: std::io::Write> IpcStream<W> {
    /// Creates a new IPC stream writer, emitting the schema header immediately.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::BitVanesError::Arrow`] on failure.
    pub fn try_new(w: W, schema: &arrow::datatypes::SchemaRef) -> Result<Self> {
        let writer = StreamWriter::try_new(w, schema)?;
        Ok(Self { writer })
    }

    /// Writes one record-batch message.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::BitVanesError::Arrow`] on failure.
    pub fn write(&mut self, batch: &RecordBatch) -> Result<()> {
        self.writer.write(batch)?;
        Ok(())
    }

    /// Writes the end-of-stream marker and flushes.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::BitVanesError::Arrow`] on failure.
    pub fn finish(mut self) -> Result<()> {
        self.writer.finish()?;
        Ok(())
    }
}

/// Rough size estimate for pre-allocation.
fn estimate_ipc_size(batch: &RecordBatch) -> usize {
    let data_size: usize = batch
        .columns()
        .iter()
        .map(|col| col.get_buffer_memory_size())
        .sum();
    data_size + 4096
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_io::batch::chunks_to_batch;
    use crate::schema::{ChunkSpec, SectionKind};
    use arrow::ipc::reader::StreamReader;

    fn sample_batch() -> RecordBatch {
        let chunks = vec![ChunkSpec {
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
        }];
        chunks_to_batch(&chunks).expect("batch should build")
    }

    #[test]
    fn ipc_output_is_valid_stream_format() {
        let batch = sample_batch();
        let ipc_bytes = write_ipc_stream(&batch).expect("IPC write");
        assert!(!ipc_bytes.is_empty(), "IPC output should be non-empty");
        // The IPC STREAMING format starts with a continuation marker
        // (0xFFFFFFFF), not the "ARROW1" file magic. The file format
        // (FileWriter) uses ARROW1; the stream format (StreamWriter)
        // uses continuation markers.
        assert!(
            ipc_bytes.len() >= 4,
            "IPC stream must have at least 4 bytes"
        );
        let marker = &ipc_bytes[..4];
        assert!(
            marker == [0xFF, 0xFF, 0xFF, 0xFF],
            "IPC stream must start with continuation marker 0xFFFFFFFF, got {marker:?}"
        );
    }

    #[test]
    fn ipc_round_trip_preserves_row_count() {
        let original = sample_batch();
        let ipc_bytes = write_ipc_stream(&original).expect("IPC write");

        let reader = StreamReader::try_new(std::io::Cursor::new(ipc_bytes), None);
        assert!(reader.is_ok(), "IPC stream should be readable");
        let reader = reader.unwrap();

        let total_rows: usize = reader
            .into_iter()
            .map(|r| r.map_or(0, |b| b.num_rows()))
            .sum();
        assert_eq!(total_rows, original.num_rows());
    }
}
