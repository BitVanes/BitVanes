//! The ONNX inference boundary — the only place that touches the model +
//! ONNX Runtime. Loads the bundled Int8 BERT-NER once (lazy, cached),
//! tokenizes with offset + word-id tracking, runs the model, argmaxes
//! per-token logits, and hands the raw subword outputs to the pure
//! [`crate::ner`] pipeline (`merge_subwords` → `aggregate`).
//!
//! All offset-sensitive logic lives in [`crate::ner`] and is unit-tested
//! without a model; this module is the thin I/O glue over `ort` + `tokenizers`.
//!
//! # Runtime requirements (only when built with `--features model`)
//!
//! - `libonnxruntime` on the host, located via `ORT_DYLIB_PATH` (ort's
//!   convention for `load-dynamic`). Absent ⇒ fail-closed `unavailable`.
//! - The model artifact: an ONNX file at `$BITVANES_NER_MODEL` (default
//!   `models/bert-base-ner-int8.onnx` next to the binary) and a matching
//!   tokenizer at `$BITVANES_NER_TOKENIZER` (default `models/tokenizer.json`).
//!   Produced by `scripts/export_ner_model.py` (forthcoming). Absent ⇒
//!   fail-closed `unavailable`.
//!
//! # Verification status
//!
//! Compile-verified (type-correct against ort 2.0.0-rc.13 + tokenizers 0.21).
//! Runtime behavior — that the model's output head matches [`CONLL_LABELS`],
//! that the tokenizer's offset mapping is byte-accurate — is gated on the
//! model artifact + libonnxruntime being present, and is exercised by a
//! runtime test once those ship. The pure post-processing it feeds is fully
//! unit-tested in [`crate::ner`].

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use ort::value::TensorRef;

use crate::NerFinding;
use crate::ner::{CONLL_LABELS, RawToken, aggregate, merge_subwords};

/// Confidence floor for emitted findings. BERT-NER softmax is overconfident;
/// this trims the long tail of low-recall guesses.
const MIN_CONFIDENCE: f32 = 0.70;

/// A lazily-loaded model + tokenizer. Held in a `OnceLock` so the (expensive)
/// load happens once per process. The session is behind a `Mutex` because
/// `ort::session::Session::run` takes `&mut self`; this also serializes
/// concurrent inferences (one model, one CPU).
struct LoadedModel {
    session: Mutex<ort::session::Session>,
    tokenizer: tokenizers::Tokenizer,
}

static MODEL: OnceLock<Option<LoadedModel>> = OnceLock::new();

/// Resolve the ONNX model path from `BITVANES_NER_MODEL` or the default.
fn model_path() -> Option<PathBuf> {
    std::env::var_os("BITVANES_NER_MODEL")
        .map(PathBuf::from)
        .or_else(|| default_artifact("model_quantized.onnx"))
}

/// Resolve the tokenizer path from `BITVANES_NER_TOKENIZER` or the default.
fn tokenizer_path() -> Option<PathBuf> {
    std::env::var_os("BITVANES_NER_TOKENIZER")
        .map(PathBuf::from)
        .or_else(|| default_artifact("tokenizer.json"))
}

/// Default artifact location: `models/<name>` next to the running binary.
fn default_artifact(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("models").join(name))
}

/// Load (once) and cache the model + tokenizer. Returns `Ok(Some(_))` on
/// success, `Ok(None)` if a required artifact is absent.
fn load() -> Option<LoadedModel> {
    let model_path = model_path()?;
    let tok_path = tokenizer_path()?;
    if !model_path.exists() || !tok_path.exists() {
        return None;
    }

    // ort init (load-dynamic reads ORT_DYLIB_PATH). `commit()` returns bool;
    // a false usually means libonnxruntime is not on the host.
    if !ort::init().with_name("bitvanes-nerd").commit() {
        return None;
    }

    let session = ort::session::Session::builder()
        .ok()?
        .commit_from_file(&model_path)
        .ok()?;
    let tokenizer = tokenizers::Tokenizer::from_file(&tok_path).ok()?;
    Some(LoadedModel {
        session: Mutex::new(session),
        tokenizer,
    })
}

/// Run NER over `text`, returning the engine-bound findings.
///
/// # Errors
///
/// All failures map to the wire contract's `Err` codes:
/// - `"unavailable"` — no model artifact, or libonnxruntime absent (fail-closed:
///   the engine refuses to emit text the detector would have scrubbed).
/// - `"internal"` — an unexpected tokenize/inference/extract fault.
pub fn run(text: &str) -> Result<Vec<NerFinding>, (&'static str, String)> {
    let model = match MODEL.get_or_init(load) {
        Some(m) => m,
        None => {
            return Err((
                "unavailable",
                "no model artifact (set BITVANES_NER_MODEL + BITVANES_NER_TOKENIZER)".into(),
            ));
        }
    };

    // 1. Tokenize with offset + word-id tracking (add special tokens [CLS]…[SEP]).
    let encoding = model
        .tokenizer
        .encode(text.to_string(), true)
        .map_err(|e| ("internal", format!("tokenize: {e}")))?;

    let ids = encoding.get_ids();
    let attn = encoding.get_attention_mask();
    let offsets = encoding.get_offsets();
    let word_ids = encoding.get_word_ids();
    let seq = ids.len();
    if seq == 0 {
        return Ok(Vec::new());
    }

    // 2. Build [1, seq] i64 input tensors (BERT expects int64).
    let ids_arr = ndarray::Array2::from_shape_vec(
        (1, seq),
        ids.iter().map(|&v| v as i64).collect(),
    )
    .map_err(|e| ("internal", format!("shape ids: {e}")))?;
    let attn_arr = ndarray::Array2::from_shape_vec(
        (1, seq),
        attn.iter().map(|&v| v as i64).collect(),
    )
    .map_err(|e| ("internal", format!("shape attn: {e}")))?;
    let tt_arr = ndarray::Array2::<i64>::zeros((1, seq));

    // 3. Run the model. Input names follow the standard BERT export.
    //    `run` takes &mut self; the Mutex serializes concurrent inferences.
    let mut session = model
        .session
        .lock()
        .map_err(|_| ("internal", "session lock poisoned".to_string()))?;
    let outputs = session
        .run(ort::inputs![
            "input_ids" => TensorRef::from_array_view(ids_arr.view()).map_err(|e| ("internal", format!("ids tensor: {e}")))?,
            "attention_mask" => TensorRef::from_array_view(attn_arr.view()).map_err(|e| ("internal", format!("attn tensor: {e}")))?,
            "token_type_ids" => TensorRef::from_array_view(tt_arr.view()).map_err(|e| ("internal", format!("tt tensor: {e}")))?,
        ])
        .map_err(|e| ("internal", format!("inference: {e}")))?;

    // 4. Extract logits: [1, seq, num_labels] as a flat row-major slice.
    let (_shape, logits) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| ("internal", format!("extract logits: {e}")))?;
    // batch is 1, so total = seq * num_labels. Derive the label count.
    let n_labels = logits.len() / seq;
    if n_labels != CONLL_LABELS.len() {
        return Err((
            "internal",
            format!(
                "model emits {n_labels} labels; expected {} (CONLL_LABELS mismatch — check the export's id2label)",
                CONLL_LABELS.len()
            ),
        ));
    }

    // 5. Argmax + softmax-max per token → RawTokens (offsets + word ids).
    let mut raws = Vec::with_capacity(seq);
    for i in 0..seq {
        let row = &logits[i * n_labels..(i + 1) * n_labels];
        let (best_id, conf) = softmax_argmax(row.iter().copied());
        let off = offsets
            .get(i)
            .copied()
            .map(|(s, e)| (s as u32, e as u32))
            .unwrap_or((0, 0));
        let word = word_ids.get(i).copied().flatten();
        raws.push(RawToken {
            label_id: best_id,
            confidence: conf,
            offset: off,
            word_id: word,
        });
    }

    // 6. Pure-Rust post-processing (unit-tested in ner.rs).
    let merged = merge_subwords(&raws);
    Ok(aggregate(text, &merged, MIN_CONFIDENCE))
}

/// Argmax over a slice of logits, returning `(best_index, softmax_prob_of_best)`.
/// softmax-max is a proper `[0,1]` confidence for the predicted label.
fn softmax_argmax(logits: impl IntoIterator<Item = f32>) -> (usize, f32) {
    let ls: Vec<f32> = logits.into_iter().collect();
    if ls.is_empty() {
        return (0, 0.0);
    }
    let max = ls.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let denom: f32 = ls.iter().map(|&v| (v - max).exp()).sum();
    let (best_id, &best_logit) = ls
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("non-empty");
    let conf = if denom > 0.0 {
        (best_logit - max).exp() / denom
    } else {
        0.0
    };
    (best_id, conf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softmax_argmax_picks_the_max_with_probability() {
        // logits where index 3 dominates → high confidence on id 3.
        let (id, conf) =
            softmax_argmax([1.0, 1.0, 1.0, 8.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(id, 3);
        assert!(conf > 0.99, "dominant logit should be ~certain: {conf}");
    }

    #[test]
    fn softmax_argmax_flat_distribution_is_low_confidence() {
        let (id, conf) = softmax_argmax([0.0, 0.0, 0.0, 0.0]);
        assert!(conf < 0.30, "flat logits should be ~uniform: {conf}");
        // `Iterator::max_by` returns the last element on ties; tie direction
        // is irrelevant for real (non-identical) logits.
        assert_eq!(id, 3);
    }

    #[test]
    fn softmax_argmax_empty_is_safe() {
        let (id, conf) = softmax_argmax(std::iter::empty::<f32>());
        assert_eq!(id, 0);
        assert_eq!(conf, 0.0);
    }

    /// End-to-end runtime test: runs the REAL model against the REAL ONNX
    /// Runtime, asserting a person name is detected. Skipped unless the model
    /// artifact is pointed at via `BITVANES_NER_MODEL` + `BITVANES_NER_TOKENIZER`
    /// (and libonnxruntime is loadable via `ORT_DYLIB_PATH`). Run with:
    ///   cargo test --features model inference_finds_a_person -- --ignored --nocapture
    #[test]
    #[ignore = "needs the model artifact + libonnxruntime"]
    fn inference_finds_a_person() {
        let (Ok(model_path), Ok(tok_path)) = (
            std::env::var("BITVANES_NER_MODEL"),
            std::env::var("BITVANES_NER_TOKENIZER"),
        ) else {
            eprintln!(
                "skipped: BITVANES_NER_MODEL / BITVANES_NER_TOKENIZER not set"
            );
            return;
        };
        if !std::path::Path::new(&model_path).exists() {
            eprintln!("skipped: model not found at {model_path}");
            return;
        }

        let text = "Please reach Alice Smith about the contract.";
        let findings =
            run(text).unwrap_or_else(|e| panic!("inference failed: {e:?}"));
        eprintln!("findings: {findings:?}");
        let has_person = findings.iter().any(|f| f.entity == "person_name");
        assert!(
            has_person,
            "expected a person_name finding; got {findings:?}"
        );

        // The person finding's byte span must point at "Alice" (or cover it)
        // in the original text — the offset invariant, verified for real.
        let person = findings
            .iter()
            .find(|f| f.entity == "person_name")
            .expect("a person finding");
        let s = usize::try_from(person.start).unwrap();
        let e = usize::try_from(person.end).unwrap();
        let span = &text[s..e];
        assert!(
            span.contains("Alice") || span.contains("Smith"),
            "person span must cover the name; got {span:?}"
        );
        eprintln!("OK: person span = {span:?} at [{s}..{e}]");
        let _ = tok_path;
    }
}
