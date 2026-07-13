//! Full pipeline orchestration: parse -> scrub -> chunk -> `RecordBatch`.
//!
//! This is the entry point called by both the wasm wrapper and the CLI.
//! It ties the four stages together into a single call that produces an
//! Arrow [`RecordBatch`] ready for FFI export or IPC streaming.

use arrow::array::RecordBatch;

use crate::arrow_io::batch::{chunks_to_batch, chunks_to_batch_with_embeddings};
use crate::chunk::{chunk_document, chunk_document_semantic};
use crate::embed::Embedder;
use crate::error::Result;
use crate::parse::parse_bytes;
use crate::schema::{ChunkStrategy, PipelineConfig};
use crate::scrub::{OffsetMap, PiiFinding, scrub_document};

/// Runs the full ETL pipeline on `bytes` and returns an Arrow
/// [`RecordBatch`] containing the chunked output.
///
/// Stages:
/// 1. Parse raw bytes into a [`Document`] via the configured format.
/// 2. Scrub PII via the configured [`ScrubProfile`] (emits findings + offsets).
/// 3. Chunk the scrubbed text via BPE token boundaries.
/// 4. Attach per-chunk PII findings + deterministic `chunk_id`.
/// 5. Assemble chunks into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Propagates [`crate::error::BitVanesError`] from any stage.
pub fn run_pipeline(bytes: &[u8], cfg: &PipelineConfig) -> Result<RecordBatch> {
    let doc = parse_bytes(bytes, cfg)?;
    let (scrubbed_doc, offset_map, findings) = scrub_document(doc, &cfg.scrub)?;
    let mut chunks = chunk_document(&scrubbed_doc, &cfg.chunk, cfg.source_label.as_deref())?;
    attach_metadata(&mut chunks, &findings, &offset_map);
    let batch = chunks_to_batch(&chunks)?;
    Ok(batch)
}

/// Like [`run_pipeline`] but generates embeddings for each chunk and fills
/// the `embedding` column with real `Float32` vectors.
///
/// The embedder is provided by the caller (typically an [`OrtEmbedder`]
/// loaded from a local model file, or a test stub).
///
/// # Errors
///
/// Propagates [`crate::error::BitVanesError`] from any pipeline stage or
/// the embedder.
///
/// [`OrtEmbedder`]: crate::embed::OrtEmbedder
pub fn run_pipeline_with_embeddings(
    bytes: &[u8],
    cfg: &PipelineConfig,
    embedder: &dyn Embedder,
) -> Result<RecordBatch> {
    let doc = parse_bytes(bytes, cfg)?;
    let (scrubbed_doc, offset_map, findings) = scrub_document(doc, &cfg.scrub)?;
    let mut chunks = chunk_document(&scrubbed_doc, &cfg.chunk, cfg.source_label.as_deref())?;
    attach_metadata(&mut chunks, &findings, &offset_map);

    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    let embeddings = embedder.embed(&texts)?;
    let dim = embedder.dim();

    let batch = chunks_to_batch_with_embeddings(&chunks, &embeddings, dim)?;
    Ok(batch)
}

/// Like [`run_pipeline`] but honours [`ChunkStrategy::Semantic`]: when the
/// config asks for semantic chunking, `embedder` guides where cuts happen.
/// For [`ChunkStrategy::Structural`] the embedder is unused and this is
/// equivalent to [`run_pipeline`].
///
/// # Errors
///
/// Propagates [`crate::error::BitVanesError`] from any stage or the embedder.
pub fn run_pipeline_with_strategy(
    bytes: &[u8],
    cfg: &PipelineConfig,
    embedder: &dyn Embedder,
) -> Result<RecordBatch> {
    let doc = parse_bytes(bytes, cfg)?;
    let (scrubbed_doc, offset_map, findings) = scrub_document(doc, &cfg.scrub)?;
    let mut chunks = match cfg.chunk.strategy {
        ChunkStrategy::Structural => {
            chunk_document(&scrubbed_doc, &cfg.chunk, cfg.source_label.as_deref())?
        }
        ChunkStrategy::Semantic { .. } => chunk_document_semantic(
            &scrubbed_doc,
            &cfg.chunk,
            embedder,
            cfg.source_label.as_deref(),
        )?,
    };
    attach_metadata(&mut chunks, &findings, &offset_map);
    let batch = chunks_to_batch(&chunks)?;
    Ok(batch)
}

/// Runs [`run_pipeline`] over many inputs, in parallel when the `parallel`
/// feature is enabled and sequentially otherwise. Returns one result per
/// input so a single failing document does not abort the batch.
///
/// # Errors
///
/// Each element of the returned `Vec` propagates the per-input error
/// independently; an `Ok` element is a fully assembled `RecordBatch`.
pub fn run_pipeline_batch(inputs: &[&[u8]], cfg: &PipelineConfig) -> Vec<Result<RecordBatch>> {
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        inputs.par_iter().map(|b| run_pipeline(b, cfg)).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        inputs.iter().map(|b| run_pipeline(b, cfg)).collect()
    }
}

/// Computes a deterministic `chunk_id` (blake3 hex) and attaches the PII
/// findings whose original-text ranges overlap each chunk.
///
/// `findings` carry offsets into the **original** (pre-scrub) text; chunk
/// offsets reference the **scrubbed** text. The `offset_map` projects chunk
/// offsets back into original-text space to perform the overlap test.
///
/// Exposed publicly so non-Arrow code paths (e.g. the wasm serde fallback)
/// can attach the same metadata as the primary batch path.
pub fn attach_metadata(
    chunks: &mut [crate::schema::ChunkSpec],
    findings: &[PiiFinding],
    offset_map: &OffsetMap,
) {
    for chunk in chunks {
        // Map chunk's scrubbed-text range back to original-text coordinates.
        let orig_start = offset_map.project_inverse(chunk.char_offset_start as usize);
        let orig_end = offset_map.project_inverse(chunk.char_offset_end as usize);

        chunk.pii = findings
            .iter()
            .filter(|f| {
                (f.offset_start as usize) < orig_end && (f.offset_end as usize) > orig_start
            })
            .cloned()
            .collect();

        // Deterministic content-addressed ID: blake3(text || source || offsets).
        let mut hasher = blake3::Hasher::new();
        hasher.update(chunk.text.as_bytes());
        hasher.update(chunk.source_path.as_bytes());
        hasher.update(&chunk.char_offset_start.to_le_bytes());
        hasher.update(&chunk.char_offset_end.to_le_bytes());
        chunk.chunk_id = hasher.finalize().to_hex().to_string();
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use crate::schema::{ChunkConfig, DocumentFormat};

    #[test]
    fn batch_processes_multiple_inputs() {
        let cfg = PipelineConfig {
            format: DocumentFormat::Text,
            chunk: ChunkConfig {
                max_tokens: 512,
                ..ChunkConfig::default()
            },
            ..PipelineConfig::default()
        };
        let inputs: Vec<&[u8]> = vec![b"first document", b"second document", b"third"];
        let results = run_pipeline_batch(&inputs, &cfg);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.is_ok(), "batch element failed: {:?}", r.as_ref().err());
            assert!(r.as_ref().unwrap().num_rows() > 0);
        }
    }

    #[test]
    fn batch_isolates_per_input_failures() {
        let cfg = PipelineConfig {
            format: DocumentFormat::Pdf, // unresolvable without cli-pdf in tests
            ..PipelineConfig::default()
        };
        let inputs: Vec<&[u8]> = vec![b"not pdf", b"also not pdf"];
        let results = run_pipeline_batch(&inputs, &cfg);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(std::result::Result::is_err));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{
        BuiltInPattern, ChunkConfig, DocumentFormat, PipelineConfig, ScrubProfile,
    };

    #[test]
    fn pipeline_produces_nonempty_batch_from_markdown() {
        let cfg = PipelineConfig {
            format: DocumentFormat::Markdown,
            scrub: ScrubProfile::default(),
            chunk: ChunkConfig {
                max_tokens: 512,
                ..ChunkConfig::default()
            },
            source_label: Some("test.md".to_string()),
            embeddings: None,
        };
        let input = b"# Title\n\nHello world. This is a test.";
        let batch = run_pipeline(input, &cfg).unwrap();
        assert!(batch.num_rows() > 0);
        assert_eq!(batch.num_columns(), 11);
    }

    #[test]
    fn pipeline_with_pii_scrubbing_redacts_email() {
        use arrow::array::{Array, StringArray};

        let cfg = PipelineConfig {
            format: DocumentFormat::Markdown,
            scrub: ScrubProfile {
                patterns: vec![BuiltInPattern::Email],
                ..ScrubProfile::default()
            },
            chunk: ChunkConfig::default(),
            source_label: None,
            embeddings: None,
        };
        let input = b"Contact alice@example.com for info.";
        let batch = run_pipeline(input, &cfg).unwrap();
        assert_eq!(batch.num_rows(), 1);

        let text_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(
            text_col.value(0).contains("[EMAIL]"),
            "email should be redacted"
        );
        assert!(
            !text_col.value(0).contains("alice@example.com"),
            "raw email should not survive"
        );
    }

    #[test]
    fn pipeline_emits_pii_metadata_for_scrubbed_email() {
        use arrow::array::{Array, ListArray};

        let cfg = PipelineConfig {
            format: DocumentFormat::Markdown,
            scrub: ScrubProfile {
                patterns: vec![BuiltInPattern::Email],
                ..ScrubProfile::default()
            },
            chunk: ChunkConfig::default(),
            source_label: None,
            embeddings: None,
        };
        let input = b"Contact alice@example.com for info.";
        let batch = run_pipeline(input, &cfg).unwrap();

        let pii_col = batch
            .column(9)
            .as_any()
            .downcast_ref::<ListArray>()
            .expect("pii_metadata should be List");
        assert!(!pii_col.is_null(0), "row 0 should carry a finding");
    }

    #[test]
    fn pipeline_chunk_id_is_deterministic() {
        use arrow::array::{Array, StringArray};

        let cfg = PipelineConfig {
            format: DocumentFormat::Markdown,
            scrub: ScrubProfile {
                patterns: vec![BuiltInPattern::Email],
                ..ScrubProfile::default()
            },
            chunk: ChunkConfig::default(),
            source_label: None,
            embeddings: None,
        };
        let input = b"Contact alice@example.com for info.";
        let batch1 = run_pipeline(input, &cfg).unwrap();
        let batch2 = run_pipeline(input, &cfg).unwrap();

        let id_col1 = batch1
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let id_col2 = batch2
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(
            id_col1.value(0),
            id_col2.value(0),
            "chunk_id must be stable"
        );
        assert_eq!(id_col1.value(0).len(), 64, "blake3 hex is 64 chars");
    }

    #[test]
    fn pipeline_empty_input_produces_empty_batch() {
        let cfg = PipelineConfig::default();
        let batch = run_pipeline(b"", &cfg).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn pipeline_preserves_heading_path() {
        use arrow::array::{Array, ListArray};

        let cfg = PipelineConfig {
            format: DocumentFormat::Markdown,
            ..PipelineConfig::default()
        };
        let input = b"# Architecture\n\nThe system has layers.\n\n## Storage\n\nWe use Arrow.";
        let batch = run_pipeline(input, &cfg).unwrap();
        assert!(batch.num_rows() >= 1);

        let heading_col = batch
            .column(5)
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let has_heading = (0..heading_col.len()).any(|i| !heading_col.is_null(i));
        assert!(has_heading, "at least one chunk should have heading_path");
    }
}
