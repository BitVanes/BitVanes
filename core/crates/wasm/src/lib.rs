//! # bitvanes-wasm
//!
//! Thin WebAssembly bindings for `bitvanes-core`. Compiles to
//! `wasm32-unknown-unknown` via `wasm-pack build --target web`.
//!
//! ## API
//!
//! - [`version`] - returns the engine's semver version.
//! - [`process`] - runs the full ETL pipeline (parse, scrub, chunk, Arrow).
//!   Returns a slot ID for retrieving the FFI pointers.
//! - [`array_ptr`] / [`schema_ptr`] - raw pointers into the wasm linear heap
//!   for the Arrow C Data Interface structs. Read via `arrow-js-ffi`.
//! - [`release_batch`] - frees the memory for an exported batch.
//!
//! ## Data path contract
//!
//! Configuration travels via `serde-wasm-bindgen` (small JSON payload).
//! Document *data* flows in as raw `&[u8]` and out as zero-copy Arrow FFI
//! pointers — no `JSON.stringify` on the data path, ever.

#![deny(unsafe_code)]

use wasm_bindgen::prelude::*;

/// Initializes the wasm module. Installs a panic hook so that Rust panics
/// produce readable stack traces in the browser console. Called
/// automatically by `wasm-bindgen`'s `start` mechanism.
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Returns the semver version of the underlying `bitvanes-core` engine.
#[wasm_bindgen]
pub fn version() -> String {
    bitvanes_core::version().to_owned()
}

/// Runs the full ETL pipeline on the given document bytes and returns a
/// slot ID for retrieving the zero-copy Arrow FFI pointers.
///
/// # Parameters
///
/// - `config_js`: A JS object matching the `PipelineConfig` JSON schema.
///   Deserialized via `serde-wasm-bindgen` (the config bridge, not data).
/// - `bytes`: Raw document bytes (UTF-8 encoded).
///
/// # Returns
///
/// A `u32` slot ID. Use [`array_ptr`] and [`schema_ptr`] to obtain the
/// Arrow C Data Interface pointers, then call [`release_batch`] when done.
///
/// # Throws
///
/// Throws a JS `Error` if the config is invalid or the pipeline fails.
#[wasm_bindgen]
pub fn process(config_js: JsValue, bytes: &[u8]) -> Result<u32, JsValue> {
    let cfg: bitvanes_core::PipelineConfig = serde_wasm_bindgen::from_value(config_js)
        .map_err(|e| JsValue::from_str(&format!("config deserialization failed: {e}")))?;

    let batch = bitvanes_core::pipeline::run_pipeline(bytes, &cfg)
        .map_err(|e| JsValue::from_str(&format!("pipeline failed: {e}")))?;

    let slot_id = bitvanes_core::arrow_io::ffi::export_batch(batch)
        .map_err(|e| JsValue::from_str(&format!("ffi export failed: {e}")))?;

    Ok(slot_id)
}

/// Returns the raw `FFI_ArrowArray` pointer (wasm linear memory address)
/// for the given slot ID. Returns 0 if the slot doesn't exist.
///
/// JS reads this pointer via `DataView` on `WebAssembly.Memory.buffer`
/// + `arrow-js-ffi`'s `parseRecordBatch`.
#[wasm_bindgen]
#[allow(clippy::cast_possible_truncation)]
pub fn array_ptr(slot_id: u32) -> u32 {
    bitvanes_core::arrow_io::ffi::ffi_pointers(slot_id).map_or(0, |(array, _)| array as u32)
}

/// Returns the raw `FFI_ArrowSchema` pointer (wasm linear memory address)
/// for the given slot ID. Returns 0 if the slot doesn't exist.
#[wasm_bindgen]
#[allow(clippy::cast_possible_truncation)]
pub fn schema_ptr(slot_id: u32) -> u32 {
    bitvanes_core::arrow_io::ffi::ffi_pointers(slot_id).map_or(0, |(_, schema)| schema as u32)
}

/// Releases the memory for an exported batch. Must be called after JS is
/// done reading the FFI pointers (typically via `FinalizationRegistry`).
///
/// After this call, the pointers from [`array_ptr`] / [`schema_ptr`] are
/// **dangling** and must not be accessed.
#[wasm_bindgen]
pub fn release_batch(slot_id: u32) {
    bitvanes_core::arrow_io::ffi::release_batch(slot_id);
}

/// Returns the number of currently-exported (unreleased) batches. Useful
/// for leak detection from JS.
#[wasm_bindgen]
#[allow(clippy::cast_possible_truncation)]
pub fn active_export_count() -> u32 {
    bitvanes_core::arrow_io::ffi::active_export_count() as u32
}

/// Processes a document and returns chunk data as a JS array (via serde,
/// NOT zero-copy Arrow FFI). Fallback for when `parseRecordBatch` fails.
///
/// Each element carries the chunk text, token count, heading ancestry,
/// section kind, deterministic `chunk_id`, and the PII findings whose
/// original-text ranges overlap the chunk.
#[wasm_bindgen]
pub fn process_chunks(config_js: JsValue, bytes: &[u8]) -> Result<JsValue, JsValue> {
    let cfg: bitvanes_core::PipelineConfig = serde_wasm_bindgen::from_value(config_js)
        .map_err(|e| JsValue::from_str(&format!("config error: {e}")))?;

    let doc = bitvanes_core::parse::parse_bytes(bytes, &cfg)
        .map_err(|e| JsValue::from_str(&format!("parse: {e}")))?;
    let (scrubbed, offset_map, findings) = bitvanes_core::pii::scrub_document(doc, &cfg.scrub)
        .map_err(|e| JsValue::from_str(&format!("scrub: {e}")))?;
    let mut chunks =
        bitvanes_core::chunk::chunk_document(&scrubbed, &cfg.chunk, cfg.source_label.as_deref())
            .map_err(|e| JsValue::from_str(&format!("chunk: {e}")))?;
    bitvanes_core::pipeline::attach_metadata(&mut chunks, &findings, &offset_map);

    let result: Vec<_> = chunks
        .iter()
        .map(|c| SimpleChunk {
            chunk_index: c.chunk_index,
            chunk_id: c.chunk_id.clone(),
            text: c.text.clone(),
            token_count: c.token_count,
            heading_path: c.heading_path.clone(),
            section_kind: format!("{:?}", c.section_kind).to_lowercase(),
            pii: c
                .pii
                .iter()
                .map(|f| SimpleFinding {
                    entity: f.entity.clone(),
                    offset_start: f.offset_start,
                    offset_end: f.offset_end,
                    confidence: f.confidence,
                    anchors_hit: f.anchors_hit.clone(),
                })
                .collect(),
        })
        .collect();

    Ok(serde_wasm_bindgen::to_value(&result)?)
}

#[derive(serde::Serialize)]
struct SimpleChunk {
    chunk_index: u32,
    chunk_id: String,
    text: String,
    token_count: u16,
    heading_path: Vec<String>,
    section_kind: String,
    pii: Vec<SimpleFinding>,
}

#[derive(serde::Serialize)]
struct SimpleFinding {
    entity: String,
    offset_start: u32,
    offset_end: u32,
    confidence: f32,
    anchors_hit: Vec<String>,
}
