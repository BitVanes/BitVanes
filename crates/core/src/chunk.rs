//! Structural-boundary-aware BPE chunker.
//!
//! Takes a scrubbed [`Document`] and a [`ChunkConfig`] and produces a
//! [`Vec<ChunkSpec>`] by greedily packing [`TextSpan`]s into chunks up to
//! `max_tokens`, splitting over-long spans at exact BPE token boundaries.
//!
//! # Algorithm
//!
//! 1. Walk spans in order.
//! 2. For each span, compute its BPE token count.
//! 3. If the span fits in the current chunk (current + span <= `max_tokens`),
//!    add it.
//! 4. If it doesn't fit, flush the current chunk and start a new one with
//!    this span.
//! 5. If a single span exceeds `max_tokens`, split it into sub-chunks at
//!    token boundaries (each sub-chunk respects `max_tokens`).
//! 6. Every chunk inherits the `heading_path` and `section_kind` of its
//!    first span — this is how structural context is preserved into the
//!    output `RecordBatch`.
//!
//! # Overlap
//!
//! `overlap_tokens > 0` repeats the last `overlap_tokens` of each chunk at
//! the start of the next, including across oversized spans (spans larger
//! than `max_tokens`): the tail of the chunk before an oversized span is
//! prepended to the oversized span's first sub-chunk, and each oversized
//! sub-chunk overlaps the next. `overlap_tokens` must be strictly less
//! than `max_tokens`.

use crate::embed::Embedder;
use crate::error::{BitVanesError, Result};
use crate::parse::{Document, offset_to_u32};
use crate::schema::{ChunkConfig, ChunkSpec, ChunkStrategy, SectionKind};
use crate::tokenize::Tokenizer;

/// Chunks a scrubbed [`Document`] into [`ChunkSpec`]s according to the
/// given [`ChunkConfig`].
///
/// `source_label` is copied verbatim into every chunk's `source_path`
/// field (typically from [`crate::schema::PipelineConfig::source_label`]).
///
/// # Errors
///
/// Returns [`BitVanesError::InvalidConfig`] if the config is inconsistent
/// (e.g. `overlap` >= `max_tokens`, or `max_tokens` == 0).
pub fn chunk_document(
    doc: &Document,
    cfg: &ChunkConfig,
    source_label: Option<&str>,
) -> Result<Vec<ChunkSpec>> {
    validate_config(cfg)?;

    if doc.spans.is_empty() {
        return Ok(Vec::new());
    }

    let tokenizer = Tokenizer::new(cfg.tokenizer)?;
    let max_tokens = cfg.max_tokens as usize;
    let source = source_label.unwrap_or("").to_string();

    let mut chunks = Vec::new();
    let mut accum = ChunkAccum::default();
    let overlap = cfg.overlap_tokens as usize;

    for span in &doc.spans {
        let span_text = span.text(doc);
        let span_tokens = tokenizer.count(span_text);

        if span_tokens > max_tokens {
            // Carry the previous chunk's overlap tail into the first
            // oversized sub-chunk so the overlap chain is not broken.
            let prefix = if overlap > 0 && accum.is_nonempty() {
                let records = accum.take_overlap(overlap);
                overlap_prefix_from_records(doc, &records)
            } else {
                OverlapPrefix::default()
            };
            if accum.is_nonempty() {
                chunks.push(accum.finish(doc, &source, &tokenizer));
            }
            accum = ChunkAccum::default();
            let ctx = SplitCtx {
                doc,
                tokenizer: &tokenizer,
                max_tokens,
                overlap,
                source: &source,
            };
            split_oversized_span(&ctx, span, &prefix, &mut chunks)?;
        } else if accum.try_add(span, span_tokens, max_tokens) {
        } else {
            let overlap_spans = accum.take_overlap(overlap);
            chunks.push(accum.finish(doc, &source, &tokenizer));
            accum = ChunkAccum::from_overlap(&overlap_spans);
            if !accum.try_add(span, span_tokens, max_tokens) {
                if accum.is_nonempty() {
                    chunks.push(accum.finish(doc, &source, &tokenizer));
                }
                accum = ChunkAccum::default();
                accum.try_add(span, span_tokens, max_tokens);
            }
        }
    }

    if accum.is_nonempty() {
        chunks.push(accum.finish(doc, &source, &tokenizer));
    }

    // Assign sequential chunk_index values.
    for (i, chunk) in chunks.iter_mut().enumerate() {
        chunk.chunk_index = offset_to_u32(i);
    }

    Ok(chunks)
}

/// Embedding-guided chunking.
///
/// Behaves like [`chunk_document`] but, under [`ChunkStrategy::Semantic`],
/// only merges an adjacent span into the current chunk when its embedding is
/// within `similarity_threshold` (cosine) of the chunk's running centroid
/// *and* `max_tokens` is respected. A topic drift therefore opens a new
/// chunk even when there is token budget left — producing topically
/// coherent chunks.
///
/// Overlap is not supported under the semantic strategy (`overlap_tokens`
/// must be 0). If `cfg.strategy` is [`ChunkStrategy::Structural`], this
/// delegates to [`chunk_document`].
///
/// # Errors
///
/// Returns [`BitVanesError::InvalidConfig`] for the usual config violations,
/// or if `overlap_tokens > 0` under the semantic strategy. Returns
/// [`BitVanesError::InvalidInput`] if the embedder returns the wrong number
/// of vectors.
pub fn chunk_document_semantic(
    doc: &Document,
    cfg: &ChunkConfig,
    embedder: &dyn Embedder,
    source_label: Option<&str>,
) -> Result<Vec<ChunkSpec>> {
    validate_config(cfg)?;
    if !matches!(cfg.strategy, ChunkStrategy::Semantic { .. }) {
        return chunk_document(doc, cfg, source_label);
    }
    if cfg.overlap_tokens != 0 {
        return Err(BitVanesError::InvalidConfig(
            "semantic chunking does not support overlap_tokens > 0 (use Structural)".to_string(),
        ));
    }
    if doc.spans.is_empty() {
        return Ok(Vec::new());
    }

    let similarity_threshold = match cfg.strategy {
        ChunkStrategy::Semantic {
            similarity_threshold,
        } => similarity_threshold,
        ChunkStrategy::Structural => unreachable!("guarded above"),
    };
    let tokenizer = Tokenizer::new(cfg.tokenizer)?;
    let max_tokens = cfg.max_tokens as usize;
    let source = source_label.unwrap_or("").to_string();

    // Embed every span once; similarity is span-vs-running-chunk-centroid.
    let span_texts: Vec<&str> = doc.spans.iter().map(|s| s.text(doc)).collect();
    let embeddings = embedder.embed(&span_texts)?;
    if embeddings.len() != doc.spans.len() {
        return Err(BitVanesError::InvalidInput(format!(
            "embedder returned {} vectors for {} spans",
            embeddings.len(),
            doc.spans.len()
        )));
    }
    let dim = embedder.dim();

    let mut chunks: Vec<ChunkSpec> = Vec::new();
    let mut accum = ChunkAccum::default();
    let mut centroid_sum = vec![0.0f32; dim];
    let mut centroid_n: u32 = 0;

    for (i, span) in doc.spans.iter().enumerate() {
        let span_tokens = tokenizer.count(span_texts[i]);

        if span_tokens > max_tokens {
            // Oversized span: flush, then fall back to token-boundary split.
            if accum.is_nonempty() {
                chunks.push(accum.finish(doc, &source, &tokenizer));
            }
            accum = ChunkAccum::default();
            centroid_sum = vec![0.0; dim];
            centroid_n = 0;
            let ctx = SplitCtx {
                doc,
                tokenizer: &tokenizer,
                max_tokens,
                overlap: 0,
                source: &source,
            };
            split_oversized_span(&ctx, span, &OverlapPrefix::default(), &mut chunks)?;
            continue;
        }

        let similar = !accum.is_nonempty()
            || cosine(&embeddings[i], &centroid_avg(&centroid_sum, centroid_n))
                >= similarity_threshold;

        if similar && accum.try_add(span, span_tokens, max_tokens) {
            for (c, v) in centroid_sum.iter_mut().zip(&embeddings[i]) {
                *c += *v;
            }
            centroid_n += 1;
        } else {
            // Flush and start a fresh chunk with this span.
            if accum.is_nonempty() {
                chunks.push(accum.finish(doc, &source, &tokenizer));
            }
            accum = ChunkAccum::default();
            centroid_sum = vec![0.0; dim];
            accum.try_add(span, span_tokens, max_tokens);
            for (c, v) in centroid_sum.iter_mut().zip(&embeddings[i]) {
                *c += *v;
            }
            centroid_n = 1;
        }
    }

    if accum.is_nonempty() {
        chunks.push(accum.finish(doc, &source, &tokenizer));
    }

    for (i, chunk) in chunks.iter_mut().enumerate() {
        chunk.chunk_index = offset_to_u32(i);
    }
    Ok(chunks)
}

/// Cosine similarity, or `0.0` if either vector is zero-norm.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom > 0.0 { dot / denom } else { 0.0 }
}

/// Elementwise mean of an embedding sum, or all-zeros when `n == 0`.
#[allow(clippy::cast_precision_loss)]
fn centroid_avg(sum: &[f32], n: u32) -> Vec<f32> {
    if n == 0 {
        return vec![0.0; sum.len()];
    }
    let n = n as f32;
    sum.iter().map(|v| v / n).collect()
}

/// Validates that the chunk config is self-consistent.
fn validate_config(cfg: &ChunkConfig) -> Result<()> {
    if cfg.max_tokens == 0 {
        return Err(BitVanesError::InvalidConfig(
            "max_tokens must be greater than zero".to_string(),
        ));
    }
    if cfg.max_tokens > u32::from(u16::MAX) {
        // Chunk token counts are emitted into a UInt16 column, so cap
        // max_tokens to keep every chunk's count representable.
        return Err(BitVanesError::InvalidConfig(format!(
            "max_tokens ({}) must not exceed {} (token_count is a u16 column)",
            cfg.max_tokens,
            u16::MAX
        )));
    }
    if cfg.overlap_tokens >= cfg.max_tokens {
        return Err(BitVanesError::InvalidConfig(format!(
            "overlap_tokens ({}) must be less than max_tokens ({})",
            cfg.overlap_tokens, cfg.max_tokens
        )));
    }
    Ok(())
}

/// Text carried from the end of one chunk to the start of the next so that
/// adjacent chunks overlap by `overlap_tokens`.
#[derive(Default, Clone)]
struct OverlapPrefix {
    text: String,
    tokens: usize,
    /// Absolute offset in `doc.full_text` where `text` begins.
    abs_start: usize,
}

/// Builds an [`OverlapPrefix`] from accumulated span records by stitching
/// their text out of the document buffer.
fn overlap_prefix_from_records(doc: &Document, records: &[SpanRecord]) -> OverlapPrefix {
    if records.is_empty() {
        return OverlapPrefix::default();
    }
    let mut text = String::new();
    for record in records {
        text.push_str(&doc.full_text[record.start..record.end]);
    }
    let tokens: usize = records.iter().map(|r| r.tokens).sum();
    OverlapPrefix {
        text,
        tokens,
        abs_start: records[0].start,
    }
}

/// Bundled arguments for [`split_oversized_span`] to keep its arity low.
struct SplitCtx<'a> {
    doc: &'a Document,
    tokenizer: &'a Tokenizer,
    max_tokens: usize,
    overlap: usize,
    source: &'a str,
}

/// Splits an over-long span into sub-chunks, each respecting `max_tokens`.
///
/// `initial_prefix` is the overlap tail of the preceding chunk (possibly
/// empty); it is prepended to the first emitted sub-chunk. When `overlap`
/// is non-zero, every sub-chunk after the first begins with the previous
/// sub-chunk's tail. Overlap tails are always derived from span-internal
/// slices so that absolute document offsets remain contiguous.
fn split_oversized_span(
    ctx: &SplitCtx,
    span: &crate::parse::TextSpan,
    initial_prefix: &OverlapPrefix,
    out: &mut Vec<ChunkSpec>,
) -> Result<()> {
    let doc = ctx.doc;
    let tokenizer = ctx.tokenizer;
    let max_tokens = ctx.max_tokens;
    let overlap = ctx.overlap;

    let full_text = span.text(doc);
    let abs_base = span.char_offset_start as usize;
    let mut cursor = 0usize;
    let mut prefix = initial_prefix.clone();

    while cursor < full_text.len() {
        let remaining = &full_text[cursor..];
        let budget = max_tokens.saturating_sub(prefix.tokens).max(1);
        let (split_at, _piece_tokens) = tokenizer.split_at_token_boundary(remaining, budget)?;

        if split_at == 0 {
            return Err(BitVanesError::InvalidInput(format!(
                "tokenizer produced a zero-length split at byte {cursor} in a span of {} bytes",
                full_text.len()
            )));
        }

        let piece = &remaining[..split_at];
        let piece_start = abs_base + cursor;

        let mut text = String::with_capacity(prefix.text.len() + piece.len());
        let start_off = if prefix.tokens > 0 {
            text.push_str(&prefix.text);
            prefix.abs_start
        } else {
            piece_start
        };
        text.push_str(piece);
        let end_off = piece_start + split_at;
        // Re-count the actual (prefix + piece) text for an accurate count.
        let total_tokens = tokenizer.count(&text);

        out.push(ChunkSpec {
            chunk_index: 0,          // re-indexed by caller
            chunk_id: String::new(), // filled by pipeline's attach_metadata
            text,
            token_count: u16::try_from(total_tokens).map_err(|_| {
                BitVanesError::InvalidInput("token count overflowed u16".to_string())
            })?,
            source_path: ctx.source.to_string(),
            heading_path: span.heading_path.clone(),
            section_kind: span.section_kind,
            char_offset_start: offset_to_u32(start_off),
            char_offset_end: offset_to_u32(end_off),
            pii: Vec::new(), // filled by pipeline's attach_metadata
        });

        cursor += split_at;

        // Carry this piece's tail into the next sub-chunk. Derived from the
        // span-internal slice so offsets stay contiguous in the document.
        if overlap > 0 && cursor < full_text.len() {
            let (suffix_offset, suffix_tokens) = tokenizer.suffix_tokens(piece, overlap)?;
            prefix = OverlapPrefix {
                text: piece[suffix_offset..].to_string(),
                tokens: suffix_tokens,
                abs_start: piece_start + suffix_offset,
            };
        } else {
            prefix = OverlapPrefix::default();
        }
    }

    Ok(())
}

/// Mutable accumulator for the current chunk being built.
#[derive(Default)]
struct ChunkAccum {
    start_offset: usize,
    end_offset: usize,
    token_count: usize,
    heading_path: Vec<String>,
    section_kind: Option<SectionKind>,
    span_records: Vec<SpanRecord>,
}

#[derive(Clone)]
struct SpanRecord {
    start: usize,
    end: usize,
    tokens: usize,
    heading_path: Vec<String>,
    section_kind: SectionKind,
}

impl ChunkAccum {
    fn try_add(
        &mut self,
        span: &crate::parse::TextSpan,
        span_tokens: usize,
        max_tokens: usize,
    ) -> bool {
        if self.token_count + span_tokens > max_tokens {
            return false;
        }
        if self.section_kind.is_none() {
            self.start_offset = span.char_offset_start as usize;
            self.heading_path.clone_from(&span.heading_path);
            self.section_kind = Some(span.section_kind);
        }
        self.end_offset = span.char_offset_end as usize;
        self.token_count += span_tokens;
        self.span_records.push(SpanRecord {
            start: span.char_offset_start as usize,
            end: span.char_offset_end as usize,
            tokens: span_tokens,
            heading_path: span.heading_path.clone(),
            section_kind: span.section_kind,
        });
        true
    }

    const fn is_nonempty(&self) -> bool {
        self.section_kind.is_some()
    }

    fn take_overlap(&self, overlap_tokens: usize) -> Vec<SpanRecord> {
        if overlap_tokens == 0 || self.span_records.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut accumulated = 0usize;
        for record in self.span_records.iter().rev() {
            if accumulated >= overlap_tokens {
                break;
            }
            result.push(record.clone());
            accumulated += record.tokens;
        }
        result.reverse();
        result
    }

    fn from_overlap(records: &[SpanRecord]) -> Self {
        if records.is_empty() {
            return Self::default();
        }
        let first = &records[0];
        let last = &records[records.len() - 1];
        let token_count: usize = records.iter().map(|r| r.tokens).sum();
        Self {
            start_offset: first.start,
            end_offset: last.end,
            token_count,
            heading_path: first.heading_path.clone(),
            section_kind: Some(first.section_kind),
            span_records: records.to_vec(),
        }
    }

    fn finish(self, doc: &Document, source: &str, tokenizer: &Tokenizer) -> ChunkSpec {
        let text = doc.full_text[self.start_offset..self.end_offset].to_string();
        // Re-count the actual concatenated chunk text rather than trusting
        // the sum of per-span counts: BPE junction effects at span boundaries
        // can make the true count differ by ±1 from the sum.
        let actual_tokens = tokenizer.count(&text);
        ChunkSpec {
            chunk_index: 0,
            chunk_id: String::new(), // filled by pipeline's attach_metadata
            text,
            token_count: u16::try_from(actual_tokens).unwrap_or(u16::MAX),
            source_path: source.to_string(),
            heading_path: self.heading_path,
            section_kind: self.section_kind.unwrap_or(SectionKind::Paragraph),
            char_offset_start: offset_to_u32(self.start_offset),
            char_offset_end: offset_to_u32(self.end_offset),
            pii: Vec::new(), // filled by pipeline's attach_metadata
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::Parser;
    use crate::parse::{Document, TextSpan};
    use crate::schema::{ChunkConfig, DocumentFormat, PipelineConfig, SectionKind, TokenizerKind};

    fn make_doc(spans: &[(&str, SectionKind)]) -> Document {
        let mut full_text = String::new();
        let mut span_vec = Vec::new();
        for (text, kind) in spans {
            let start = full_text.len();
            full_text.push_str(text);
            let end = full_text.len();
            span_vec.push(TextSpan {
                char_offset_start: offset_to_u32(start),
                char_offset_end: offset_to_u32(end),
                heading_path: Vec::new(),
                section_kind: *kind,
            });
        }
        Document {
            full_text,
            spans: span_vec,
        }
    }

    fn cfg(max_tokens: u32) -> ChunkConfig {
        ChunkConfig {
            max_tokens,
            overlap_tokens: 0,
            tokenizer: TokenizerKind::Cl100kBase,
            strategy: ChunkStrategy::default(),
        }
    }

    #[test]
    fn token_count_is_exact_for_packed_chunks() {
        // The reported token_count must equal the BPE count of the actual
        // chunk text, not a sum of per-span counts (which can drift at span
        // junctions). Pack several spans into one chunk to exercise this.
        let doc = make_doc(&[
            ("The quick brown fox ", SectionKind::Paragraph),
            ("jumps over the lazy dog ", SectionKind::Paragraph),
            ("while a cat watches quietly ", SectionKind::Paragraph),
            ("from the wooden windowsill.", SectionKind::Paragraph),
        ]);
        let chunks = chunk_document(&doc, &cfg(512), None).unwrap();
        let tokenizer = crate::tokenize::Tokenizer::new(TokenizerKind::Cl100kBase).unwrap();
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert_eq!(
                c.token_count as usize,
                tokenizer.count(&c.text),
                "reported token_count must match actual count of chunk text {:?}",
                c.text
            );
        }
    }

    #[test]
    fn empty_document_produces_no_chunks() {
        let doc = Document::default();
        assert!(chunk_document(&doc, &cfg(512), None).unwrap().is_empty());
    }

    #[test]
    fn single_small_span_produces_one_chunk() {
        let doc = make_doc(&[("Hello world.", SectionKind::Paragraph)]);
        let chunks = chunk_document(&doc, &cfg(512), Some("test.md")).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "Hello world.");
        assert!(chunks[0].token_count > 0);
        assert_eq!(chunks[0].source_path, "test.md");
        assert_eq!(chunks[0].section_kind, SectionKind::Paragraph);
    }

    #[test]
    fn multiple_small_spans_pack_into_one_chunk() {
        let doc = make_doc(&[
            ("First sentence. ", SectionKind::Paragraph),
            ("Second sentence. ", SectionKind::Paragraph),
            ("Third.", SectionKind::Paragraph),
        ]);
        let chunks = chunk_document(&doc, &cfg(512), None).unwrap();
        assert_eq!(chunks.len(), 1, "all three spans should fit in one chunk");
        assert!(chunks[0].text.contains("First"));
        assert!(chunks[0].text.contains("Third"));
    }

    #[test]
    fn spans_split_across_chunks_when_exceeding_max_tokens() {
        // Each span is ~3-4 tokens. With max_tokens=5, we should get
        // multiple chunks.
        let doc = make_doc(&[
            ("The quick brown ", SectionKind::Paragraph),
            ("fox jumps over ", SectionKind::Paragraph),
            ("the lazy dog ", SectionKind::Paragraph),
            ("while a cat watches.", SectionKind::Paragraph),
        ]);
        let chunks = chunk_document(&doc, &cfg(5), None).unwrap();
        assert!(
            chunks.len() > 1,
            "should produce multiple chunks with tight token limit"
        );

        // Every chunk must respect max_tokens.
        for chunk in &chunks {
            assert!(
                chunk.token_count <= 5,
                "chunk {} has {} tokens, max is 5",
                chunk.chunk_index,
                chunk.token_count
            );
        }

        // Chunk indices must be sequential.
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index as usize, i);
        }
    }

    #[test]
    fn oversized_span_is_split_at_token_boundaries() {
        // A single very long span that far exceeds max_tokens.
        let long_text = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let doc = make_doc(&[(&long_text, SectionKind::Paragraph)]);
        let chunks = chunk_document(&doc, &cfg(10), None).unwrap();

        assert!(
            chunks.len() > 1,
            "oversized span should be split into multiple chunks"
        );
        for chunk in &chunks {
            assert!(
                chunk.token_count <= 10,
                "sub-chunk has {} tokens, max is 10",
                chunk.token_count
            );
        }
    }

    #[test]
    fn heading_path_inherited_from_first_span() {
        let doc = {
            let mut full_text = String::new();
            let mut spans = Vec::new();
            // First span under "Architecture" heading.
            let s1_start = full_text.len();
            full_text.push_str("Content under architecture.");
            spans.push(TextSpan {
                char_offset_start: offset_to_u32(s1_start),
                char_offset_end: offset_to_u32(full_text.len()),
                heading_path: vec!["Architecture".to_string()],
                section_kind: SectionKind::Paragraph,
            });
            // Second span under "Architecture > Storage".
            let s2_start = full_text.len();
            full_text.push_str("Storage details here.");
            spans.push(TextSpan {
                char_offset_start: offset_to_u32(s2_start),
                char_offset_end: offset_to_u32(full_text.len()),
                heading_path: vec!["Architecture".to_string(), "Storage".to_string()],
                section_kind: SectionKind::Paragraph,
            });
            Document { full_text, spans }
        };

        // Use max_tokens=5 so each span fits individually (~4 tokens each)
        // but both don't fit together (~8 > 5), producing two chunks.
        let chunks = chunk_document(&doc, &cfg(5), None).unwrap();
        assert!(chunks.len() >= 2, "should produce at least 2 chunks");

        // Find the chunk containing "Storage" and verify its heading_path.
        let storage_chunk = chunks
            .iter()
            .find(|c| c.text.contains("Storage"))
            .expect("should find a chunk containing Storage text");
        assert_eq!(
            storage_chunk.heading_path,
            vec!["Architecture".to_string(), "Storage".to_string()]
        );

        // Find the chunk containing "Content under" and verify its heading_path.
        let arch_chunk = chunks
            .iter()
            .find(|c| c.text.contains("Content under"))
            .expect("should find a chunk with Content text");
        assert_eq!(arch_chunk.heading_path, vec!["Architecture".to_string()]);
    }

    #[test]
    fn section_kind_inherited_from_first_span() {
        let doc = make_doc(&[
            ("Some code here.", SectionKind::Code),
            ("More code.", SectionKind::Code),
        ]);
        let chunks = chunk_document(&doc, &cfg(512), None).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].section_kind, SectionKind::Code);
    }

    #[test]
    fn max_tokens_zero_is_rejected() {
        let doc = make_doc(&[("hello", SectionKind::Paragraph)]);
        let err = chunk_document(&doc, &cfg(0), None).unwrap_err();
        assert!(matches!(err, BitVanesError::InvalidConfig(_)));
    }

    #[test]
    fn overlap_creates_overlapping_text() {
        let doc = make_doc(&[
            ("The quick brown fox ", SectionKind::Paragraph),
            ("jumps over the lazy dog ", SectionKind::Paragraph),
            ("while a cat watches ", SectionKind::Paragraph),
            ("from the windowsill.", SectionKind::Paragraph),
        ]);
        let cfg_overlap = ChunkConfig {
            max_tokens: 8,
            overlap_tokens: 3,
            tokenizer: TokenizerKind::Cl100kBase,
            strategy: ChunkStrategy::default(),
        };
        let chunks = chunk_document(&doc, &cfg_overlap, None).unwrap();
        assert!(
            chunks.len() >= 2,
            "should produce multiple chunks with tight limit"
        );
        for chunk in &chunks {
            assert!(
                chunk.token_count <= 8,
                "chunk has {} tokens, max 8",
                chunk.token_count
            );
        }
    }

    #[test]
    fn chunk_text_round_trips_through_parser() {
        // End-to-end: parse markdown → chunk → verify chunk text is
        // a substring of the document text.
        let input = "# Title\n\nFirst paragraph here.\n\nSecond paragraph there.";
        let parse_cfg = PipelineConfig {
            format: DocumentFormat::Markdown,
            ..PipelineConfig::default()
        };
        let doc = crate::parse::MarkdownParser
            .parse(input, &parse_cfg)
            .expect("parse");
        let chunks = chunk_document(&doc, &cfg(5), None).unwrap();
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(
                doc.full_text.contains(&chunk.text),
                "chunk text must be a substring of the document"
            );
        }
    }

    #[test]
    fn overlap_carries_across_oversized_span() {
        // A normal span followed by a span large enough to require splitting.
        let normal = "alpha beta gamma delta epsilon zeta ";
        let oversized = "the quick brown fox jumps over the lazy dog repeatedly ".repeat(10);
        let doc = make_doc(&[
            (normal, SectionKind::Paragraph),
            (&oversized, SectionKind::Paragraph),
        ]);
        let cfg_overlap = ChunkConfig {
            max_tokens: 12,
            overlap_tokens: 3,
            tokenizer: TokenizerKind::Cl100kBase,
            strategy: ChunkStrategy::default(),
        };
        let chunks = chunk_document(&doc, &cfg_overlap, None).unwrap();
        assert!(chunks.len() >= 3, "expected multiple chunks");

        // Every adjacent pair must overlap: chunk[i+1] must begin with a
        // non-empty char-aligned suffix of chunk[i]. This is the property
        // that previously broke at the oversized-span boundary.
        for w in chunks.windows(2) {
            let (prev, next) = (&w[0].text, &w[1].text);
            let mut best = 0;
            let mut cut = 1;
            while cut <= prev.len().min(next.len()) {
                if prev.is_char_boundary(prev.len() - cut)
                    && next.is_char_boundary(cut)
                    && next.starts_with(&prev[prev.len() - cut..])
                {
                    best = cut;
                }
                cut += 1;
            }
            assert!(
                best > 0,
                "adjacent chunks must overlap\nprev: {prev:?}\nnext: {next:?}"
            );
        }
    }

    #[test]
    fn max_tokens_above_u16_max_is_rejected() {
        let doc = make_doc(&[("hello", SectionKind::Paragraph)]);
        let big = ChunkConfig {
            max_tokens: u32::from(u16::MAX) + 1,
            overlap_tokens: 0,
            tokenizer: TokenizerKind::Cl100kBase,
            strategy: ChunkStrategy::default(),
        };
        let err = chunk_document(&doc, &big, None).unwrap_err();
        assert!(matches!(err, BitVanesError::InvalidConfig(_)));
    }

    // --- Semantic (embedding-guided) chunking ---

    /// Maps text to one of two orthogonal topic vectors so similarity is
    /// deterministic: "cat" → [1,0], "dog" → [0,1], else the diagonal.
    struct TopicEmbedder;
    impl Embedder for TopicEmbedder {
        fn dim(&self) -> usize {
            2
        }
        fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    if t.contains("cat") {
                        vec![1.0, 0.0]
                    } else if t.contains("dog") {
                        vec![0.0, 1.0]
                    } else {
                        vec![0.707_106_77, 0.707_106_77]
                    }
                })
                .collect())
        }
    }

    fn semantic_cfg(threshold: f32) -> ChunkConfig {
        ChunkConfig {
            max_tokens: 512,
            overlap_tokens: 0,
            tokenizer: TokenizerKind::Cl100kBase,
            strategy: ChunkStrategy::Semantic {
                similarity_threshold: threshold,
            },
        }
    }

    fn topic_doc() -> Document {
        make_doc(&[
            ("cat one ", SectionKind::Paragraph),
            ("cat two ", SectionKind::Paragraph),
            ("dog three ", SectionKind::Paragraph),
            ("dog four ", SectionKind::Paragraph),
        ])
    }

    #[test]
    fn semantic_splits_on_topic_drift() {
        // cosine(cat, dog) = 0 < 0.5, so the cat-run and dog-run separate.
        let doc = topic_doc();
        let chunks =
            chunk_document_semantic(&doc, &semantic_cfg(0.5), &TopicEmbedder, None).unwrap();
        assert_eq!(
            chunks.len(),
            2,
            "cats and dogs should form separate chunks: {:?}",
            chunks.iter().map(|c| c.text.as_str()).collect::<Vec<_>>()
        );
        assert!(chunks[0].text.contains("cat") && !chunks[0].text.contains("dog"));
        assert!(chunks[1].text.contains("dog"));
    }

    #[test]
    fn semantic_merges_when_threshold_is_zero() {
        // cosine 0 >= 0 threshold → everything merges (token budget allows it).
        let doc = topic_doc();
        let chunks =
            chunk_document_semantic(&doc, &semantic_cfg(0.0), &TopicEmbedder, None).unwrap();
        assert_eq!(chunks.len(), 1, "threshold 0 should merge all spans");
    }

    #[test]
    fn semantic_still_respects_max_tokens() {
        // Tight token budget forces a split even within the same topic.
        let doc = make_doc(&[
            ("cat one two three four ", SectionKind::Paragraph),
            ("cat five six seven eight ", SectionKind::Paragraph),
        ]);
        let mut c = semantic_cfg(0.0);
        c.max_tokens = 5;
        let chunks = chunk_document_semantic(&doc, &c, &TopicEmbedder, None).unwrap();
        assert!(chunks.len() > 1, "max_tokens must still cap chunk size");
        for ch in &chunks {
            assert!(ch.token_count <= 5);
        }
    }

    #[test]
    fn semantic_rejects_overlap() {
        let doc = topic_doc();
        let mut c = semantic_cfg(0.5);
        c.overlap_tokens = 4;
        let err = chunk_document_semantic(&doc, &c, &TopicEmbedder, None).unwrap_err();
        assert!(matches!(err, BitVanesError::InvalidConfig(_)));
    }

    #[test]
    fn semantic_delegates_to_structural_for_structural_strategy() {
        // A Structural strategy must ignore the embedder entirely and match
        // the plain structural chunker exactly.
        struct PanicEmbedder;
        impl Embedder for PanicEmbedder {
            fn dim(&self) -> usize {
                2
            }
            fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>> {
                unreachable!("structural strategy must not call the embedder")
            }
        }
        let doc = topic_doc();
        let cfg = ChunkConfig {
            max_tokens: 5,
            overlap_tokens: 0,
            tokenizer: TokenizerKind::Cl100kBase,
            strategy: ChunkStrategy::Structural,
        };
        let via_strategy = chunk_document_semantic(&doc, &cfg, &PanicEmbedder, None).unwrap();
        let structural = chunk_document(&doc, &cfg, None).unwrap();
        assert_eq!(via_strategy, structural);
    }
}
