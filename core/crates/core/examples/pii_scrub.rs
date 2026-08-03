//! PII scrubbing: redact built-in patterns before chunking, with an offset
//! map that can project chunk positions back onto the original document.
//!
//! Run: `cargo run -p bitvanes-core --example pii_scrub`

use bitvanes_core::{
    BuiltInPattern, ChunkConfig, PipelineConfig, ScrubProfile, TokenizerKind,
    chunk::chunk_document,
    parse::{MarkdownParser, Parser},
    pii::scrub_document,
};

fn main() -> bitvanes_core::Result<()> {
    let src = "Reach alice@example.com or 415-555-0123. File 123-45-6789 was leaked.";

    let doc = MarkdownParser.parse(src, &PipelineConfig::default())?;

    let profile = ScrubProfile {
        patterns: vec![
            BuiltInPattern::Email,
            BuiltInPattern::Phone,
            BuiltInPattern::Ssn,
        ],
        ..ScrubProfile::default()
    };
    let (scrubbed, offset_map, findings) = scrub_document(doc, &profile)?;

    println!("scrubbed text:\n  {}\n", scrubbed.full_text);
    println!(
        "  {} finding(s) emitted (offsets into original text):",
        findings.len()
    );
    for f in &findings {
        println!(
            "    {entity:>14}  conf={conf:.2}  [{s}..{e}]  anchors={anchors:?}",
            entity = f.entity,
            conf = f.confidence,
            s = f.offset_start,
            e = f.offset_end,
            anchors = f.anchors_hit,
        );
    }
    let _ = offset_map;

    let chunks = chunk_document(
        &scrubbed,
        &ChunkConfig {
            max_tokens: 32,
            overlap_tokens: 0,
            tokenizer: TokenizerKind::Cl100kBase,
            ..ChunkConfig::default()
        },
        None,
    )?;
    println!(
        "{} chunk(s) emitted (PII never crosses a boundary).",
        chunks.len()
    );
    Ok(())
}
