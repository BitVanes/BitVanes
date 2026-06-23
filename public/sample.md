# BitVanes Architecture

## Overview

BitVanes is a zero-trust ETL pipeline designed for AI and RAG workloads.
It processes documents entirely in the browser using WebAssembly, ensuring
no data ever leaves the client's machine during parsing, sanitization, and
chunking.

### Key Features

- Zero-copy Arrow data transfer via the C Data Interface
- BPE-aware chunking with structural context preservation
- PII redaction before tokenization

## Security Model

All document processing happens inside the browser sandbox. The engine
embeds BPE vocabulary files at compile time, so no network calls occur
during tokenization. Contact alice@example.com for security inquiries.

### PII Patterns

The engine redacts the following PII types:
- Email addresses (alice@example.com → [EMAIL])
- US Social Security Numbers (123-45-6789 → [SSN])
- Credit card numbers (validated via Luhn checksum)
- API keys (AKIAIOSFODNN7EXAMPLE → [AWS_KEY])

## Architecture

```rust
pub fn run_pipeline(bytes: &[u8], cfg: &PipelineConfig) -> Result<RecordBatch> {
    let doc = parse_bytes(bytes, cfg)?;
    let (scrubbed_doc, _map) = scrub_document(doc, &cfg.scrub)?;
    let chunks = chunk_document(&scrubbed_doc, &cfg.chunk, ...)?;
    let batch = chunks_to_batch(&chunks)?;
    Ok(batch)
}
```

The pipeline runs in four stages: parse, scrub, chunk, and assemble.

## Tokenizers

The engine supports six OpenAI tokenizers:

1. `cl100k_base` — GPT-4 / GPT-3.5
2. `o200k_base` — GPT-4o / GPT-4.1
3. `r50k_base` — GPT-3 / davinci
4. `p50k_base` — Code models
5. `p50k_edit` — Edit models
6. `o200k_harmony` — gpt-oss models

Each tokenizer's vocabulary is embedded at compile time via `include_str!`.

## Conclusion

BitVanes delivers production-grade document chunking with zero telemetry.
The entire pipeline runs client-side, making it ideal for privacy-sensitive
RAG applications in healthcare, finance, and legal domains.
