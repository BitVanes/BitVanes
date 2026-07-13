//! Minimal: parse markdown into structural chunks and print them.
//!
//! Run: `cargo run -p bitvanes-core --example basic_chunks`

use bitvanes_core::{
    ChunkConfig, PipelineConfig, TokenizerKind,
    chunk::chunk_document,
    parse::{MarkdownParser, Parser},
};

fn main() -> bitvanes_core::Result<()> {
    let src = "# Architecture\n\
               \n\
               The engine has four stages: parse, scrub, chunk, assemble.\n\
               \n\
               ## Storage\n\
               \n\
               Output is an Apache Arrow RecordBatch with nine columns.";

    let doc = MarkdownParser.parse(src, &PipelineConfig::default())?;
    let chunk_cfg = ChunkConfig {
        max_tokens: 16,
        overlap_tokens: 0,
        tokenizer: TokenizerKind::Cl100kBase,
        ..ChunkConfig::default()
    };
    let chunks = chunk_document(&doc, &chunk_cfg, Some("overview.md"))?;

    for c in &chunks {
        let path = if c.heading_path.is_empty() {
            "(root)".to_string()
        } else {
            c.heading_path.join(" › ")
        };
        println!("[#{} | {} tokens | {}]", c.chunk_index, c.token_count, path);
        println!("  {}", c.text.replace('\n', " "));
    }
    Ok(())
}
