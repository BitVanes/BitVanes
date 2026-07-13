//! Bring-your-own embedder: implement the `Embedder` trait and fill the
//! `embedding` column with real vectors — no ONNX Runtime required.
//!
//! Run: `cargo run -p bitvanes-core --example custom_embedder`

use bitvanes_core::{DocumentFormat, Embedder, PipelineConfig, run_pipeline_with_embeddings};

/// A toy embedder: derives a small deterministic vector from each text by
/// hashing its bytes. Real users plug in `OrtEmbedder` (with the `embeddings`
/// feature) or their own backend.
struct HashEmbedder {
    dim: usize,
}

impl Embedder for HashEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, texts: &[&str]) -> bitvanes_core::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut state: u32 = 0x811C_9DC5;
                for b in t.bytes() {
                    state ^= u32::from(b);
                    state = state.wrapping_mul(0x0100_0193);
                }
                (0..self.dim)
                    .map(|i| {
                        // Value masked to 0..=255; the cast to f32 is lossless.
                        #[allow(clippy::cast_precision_loss)]
                        let byte = ((state >> i) & 0xFF) as f32;
                        byte / 255.0
                    })
                    .collect()
            })
            .collect())
    }
}

fn main() -> bitvanes_core::Result<()> {
    let cfg = PipelineConfig {
        format: DocumentFormat::Text,
        ..PipelineConfig::default()
    };
    let embedder = HashEmbedder { dim: 8 };

    let batch =
        run_pipeline_with_embeddings(b"Hello world. Second sentence here.", &cfg, &embedder)?;

    println!("{} rows, {} columns", batch.num_rows(), batch.num_columns());
    println!(
        "embedding column has {} non-null value(s)",
        batch.column(8).null_count()
    );
    // null_count() returns the number of NULLs; with embeddings filled it is 0.
    assert_eq!(batch.column(8).null_count(), 0);
    Ok(())
}
