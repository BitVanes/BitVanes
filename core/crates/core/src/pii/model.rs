//! Tier-2 pluggable PII detection: an extension point for ML/NLP-based
//! named-entity recognisers (NER) that complement the Tier-1 regex +
//! checksum engine in [`crate::pii::detect`].
//!
//! # Design
//!
//! The Tier-1 engine (regex + Luhn/ABA + anchor windows) is fast,
//! deterministic, and has no external dependencies. Some PII — personal
//! names, physical addresses, dates of birth, medical record numbers —
//! resists structural patterns and requires a statistical model. The
//! [`PiiDetector`] trait is the plug-in point for such models.
//!
//! A detector runs **after** Tier-1 and appends its findings to the same
//! `Vec<PiiFinding>`. Overlap resolution then merges Tier-1 and Tier-2
//! detections. This keeps the pipeline contract unchanged: one finding list,
//! one offset map, one `pii_metadata` column.
//!
//! # Current status
//!
//! [`ModelDetector`] is a **stub**: it exists so the trait object can be
//! wired through the pipeline and config today, but its `detect` impl is
//! `unimplemented!()` behind the `pii-model` cargo feature. A future pass
//! will provide an ONNX-backed implementation (BERT-NER or similar).
//!
//! # Future: ONNX integration
//!
//! ```ignore
//! let detector = ModelDetector::new("models/bert-ner.onnx", "models/tokenizer.json")?;
//! // detector.detect(&text, &mut findings)?;
//! ```

use crate::error::Result;
use crate::pii::detect::PiiFinding;

/// A pluggable Tier-2 PII detector (NER model, gazetteer, ...).
///
/// Implementations append findings to the caller-provided `Vec`; the
/// pipeline merges them with Tier-1 results and resolves overlaps.
///
/// `Send + Sync` is required so the detector can run on a rayon thread pool
/// in the CLI's batch path.
pub trait PiiDetector: Send + Sync {
    /// Scans `text` and appends any detected findings to `findings`.
    ///
    /// Findings MUST carry offsets into the same `text` (original-text
    /// coordinate space, before scrubbing).
    ///
    /// # Errors
    ///
    /// Implementations may return [`crate::error::BitVanesError`] on model
    /// load failure, inference error, or invalid input.
    fn detect(&self, text: &str, findings: &mut Vec<PiiFinding>) -> Result<()>;
}

/// Placeholder Tier-2 detector backed by an (unimplemented) ONNX NER model.
///
/// Fields store the model + tokenizer paths so a future implementation can
/// lazy-load them. Constructed via [`ModelDetector::new`].
///
/// **Not yet functional** — `detect()` panics with `unimplemented!()`.
/// Gated behind the `pii-model` cargo feature.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ModelDetector {
    model_path: String,
    tokenizer_path: String,
    /// Confidence floor for model-emitted findings. Model logits are often
    /// noisy; findings below this floor are dropped.
    min_confidence: f32,
}

impl ModelDetector {
    /// Creates a detector handle for the given model + tokenizer paths.
    ///
    /// Does NOT load the model (lazy-load happens on first `detect` call in
    /// the future implementation).
    #[must_use]
    pub fn new(model_path: impl Into<String>, tokenizer_path: impl Into<String>) -> Self {
        Self {
            model_path: model_path.into(),
            tokenizer_path: tokenizer_path.into(),
            min_confidence: 0.70,
        }
    }

    /// Sets the minimum confidence for model-emitted findings.
    #[must_use]
    pub const fn with_min_confidence(mut self, floor: f32) -> Self {
        self.min_confidence = floor;
        self
    }
}

#[cfg(feature = "pii-model")]
impl PiiDetector for ModelDetector {
    fn detect(&self, _text: &str, _findings: &mut Vec<PiiFinding>) -> Result<()> {
        // Future work: load ONNX model + tokenizer, run inference, map
        // token-level NER labels back to character offsets, filter by
        // min_confidence, and append PiiFindings.
        unimplemented!("ModelDetector is a stub; ONNX integration lands in a future release")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_detector_stores_paths() {
        let d = ModelDetector::new("a.onnx", "b.json");
        assert_eq!(d.model_path, "a.onnx");
        assert_eq!(d.tokenizer_path, "b.json");
        assert!((d.min_confidence - 0.70).abs() < 1e-6);
    }

    #[test]
    fn model_detector_builder_sets_confidence() {
        let d = ModelDetector::new("a.onnx", "b.json").with_min_confidence(0.85);
        assert!((d.min_confidence - 0.85).abs() < 1e-6);
    }

    #[test]
    fn noop_detector_impl_works() {
        // A trivial no-op detector proves the trait is object-safe and
        // composable with the pipeline without needing the ONNX feature.
        struct Noop;
        impl PiiDetector for Noop {
            fn detect(&self, _text: &str, findings: &mut Vec<PiiFinding>) -> Result<()> {
                findings.clear(); // explicitly empty
                Ok(())
            }
        }
        let mut v = vec![PiiFinding {
            entity: "email".to_string(),
            offset_start: 0,
            offset_end: 5,
            confidence: 0.9,
            anchors_hit: vec![],
        }];
        Noop.detect("hello", &mut v).unwrap();
        assert!(v.is_empty());
    }
}
