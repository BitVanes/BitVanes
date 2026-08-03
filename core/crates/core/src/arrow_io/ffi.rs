//! Arrow C Data Interface FFI export: the zero-copy bridge between the
//! Rust engine and JavaScript.
//!
//! # Architecture
//!
//! 1. [`export_batch`] takes a `RecordBatch`, converts it to the Arrow C
//!    Data Interface structs (`FFI_ArrowArray` + `FFI_ArrowSchema`), stores
//!    everything in a process-wide registry, and returns a slot `u32` ID.
//! 2. JS reads the raw pointers (obtained via [`ffi_pointers`]) from the
//!    wasm linear memory using `arrow-js-ffi`.
//! 3. JS calls [`release_batch`] when done, dropping the Rust memory.
//!
//! # Lifetime contract
//!
//! The registry owns the `RecordBatch` and the FFI structs. The pointers
//! returned by [`ffi_pointers`] are valid **only** until [`release_batch`]
//! is called. After release, the memory is freed and the pointers become
//! dangling. JS MUST call `release_batch` via a `FinalizationRegistry` or
//! equivalent to avoid memory leaks.
//!
//! # Thread safety
//!
//! The registry uses `Mutex<HashMap>` with `OnceLock` initialization. On
//! `wasm32-unknown-unknown` (single-threaded), the mutex is a no-op. On
//! native targets, it provides multi-threaded access for the CLI.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use arrow::array::{Array, ArrayData, RecordBatch, StructArray};
use arrow::ffi::{FFI_ArrowArray, FFI_ArrowSchema};

use crate::error::Result;

/// Monotonic ID generator for exported batches.
static NEXT_ID: AtomicU32 = AtomicU32::new(0);

/// Process-wide registry of exported batches. Lives for the duration of
/// the process (or wasm module instance).
fn registry() -> &'static Mutex<HashMap<u32, ExportedBatch>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u32, ExportedBatch>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// An exported batch: the `ArrayData` (owns the buffers) plus the FFI structs
/// that JS reads via raw pointers. All three must be kept alive together.
struct ExportedBatch {
    // Order matters: ffi structs reference into the data, so data must be
    // declared first (dropped last).
    ffi_schema: Box<FFI_ArrowSchema>,
    ffi_array: Box<FFI_ArrowArray>,
    _data: ArrayData,
}

/// Exports a [`RecordBatch`] via the Arrow C Data Interface. Returns a slot
/// ID that JS uses to obtain the raw FFI pointers and, later, to release
/// the memory.
///
/// # Errors
///
/// Returns [`BitVanesError::Arrow`] if the FFI conversion fails.
pub fn export_batch(batch: RecordBatch) -> Result<u32> {
    let schema = batch.schema();

    // Convert the RecordBatch to a single StructArray's ArrayData so it
    // can be exported as one FFI_ArrowArray (struct of columns).
    let struct_array = StructArray::from(batch);
    let data = struct_array.into_data();

    let ffi_schema = Box::new(FFI_ArrowSchema::try_from(schema.as_ref())?);
    let ffi_array = Box::new(FFI_ArrowArray::new(&data));

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut guard = registry().lock().expect("registry mutex poisoned");
    guard.insert(
        id,
        ExportedBatch {
            ffi_schema,
            ffi_array,
            _data: data,
        },
    );
    Ok(id)
}

/// Returns the raw pointers to the `FFI_ArrowArray` and `FFI_ArrowSchema`
/// structs for the given slot ID, as `(array_ptr, schema_ptr)`.
///
/// These are **wasm-linear-memory addresses** — JS reads them via
/// `DataView` on `WebAssembly.Memory.buffer` + `arrow-js-ffi`.
///
/// Returns `None` if the slot ID doesn't exist (already released or never
/// exported).
#[must_use]
pub fn ffi_pointers(slot_id: u32) -> Option<(usize, usize)> {
    let guard = registry().lock().expect("registry mutex poisoned");
    let exported = guard.get(&slot_id)?;
    let array_ptr = core::ptr::from_ref(exported.ffi_array.as_ref()) as usize;
    let schema_ptr = core::ptr::from_ref(exported.ffi_schema.as_ref()) as usize;
    Some((array_ptr, schema_ptr))
}

/// Releases the memory associated with the given slot ID. After this call,
/// the FFI pointers obtained from [`ffi_pointers`] are **dangling** and
/// must not be accessed.
///
/// Returns `true` if the slot was found and released, `false` if it was
/// already released or never existed.
pub fn release_batch(slot_id: u32) -> bool {
    let mut guard = registry().lock().expect("registry mutex poisoned");
    guard.remove(&slot_id).is_some()
}

/// Returns the number of currently-exported (unreleased) batches. Useful
/// for leak detection in tests.
#[must_use]
pub fn active_export_count() -> usize {
    let guard = registry().lock().expect("registry mutex poisoned");
    guard.len()
}

/// Releases all exported batches. Used by tests to reset state.
#[cfg(test)]
pub fn release_all() {
    let mut guard = registry().lock().expect("registry mutex poisoned");
    guard.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_io::batch::chunks_to_batch;
    use crate::schema::{ChunkSpec, SectionKind};
    use std::sync::{Mutex, MutexGuard};

    // Serialize FFI tests since they share a global registry.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().expect("test lock poisoned")
    }

    fn make_batch(n: usize) -> RecordBatch {
        let chunks: Vec<ChunkSpec> = (0..n)
            .map(|i| {
                let idx = u32::try_from(i).expect("test index fits in u32");
                ChunkSpec {
                    chunk_index: idx,
                    chunk_id: format!("id{i}"),
                    text: format!("chunk {i}"),
                    token_count: 2,
                    source_path: "test.md".to_string(),
                    heading_path: vec![],
                    section_kind: SectionKind::Paragraph,
                    char_offset_start: idx * 10,
                    char_offset_end: idx * 10 + 8,
                    pii: vec![],
                }
            })
            .collect();
        chunks_to_batch(&chunks).expect("batch should build")
    }

    #[test]
    fn export_and_release_round_trip() {
        let _lock = lock();
        release_all();
        assert_eq!(active_export_count(), 0);

        let batch = make_batch(5);
        let id = export_batch(batch).expect("export should succeed");
        assert_eq!(active_export_count(), 1);

        let (array_ptr, schema_ptr) = ffi_pointers(id).expect("pointers should exist");
        assert!(array_ptr > 0, "array pointer should be nonzero");
        assert!(schema_ptr > 0, "schema pointer should be nonzero");

        assert!(release_batch(id), "release should find the slot");
        assert_eq!(active_export_count(), 0);

        assert!(
            ffi_pointers(id).is_none(),
            "pointers should be gone after release"
        );
    }

    #[test]
    fn release_unknown_id_returns_false() {
        let _lock = lock();
        assert!(!release_batch(999_999), "unknown ID should return false");
    }

    #[test]
    fn multiple_exports_coexist() {
        let _lock = lock();
        release_all();
        let id1 = export_batch(make_batch(3)).unwrap();
        let id2 = export_batch(make_batch(7)).unwrap();
        let id3 = export_batch(make_batch(1)).unwrap();
        assert_eq!(active_export_count(), 3);

        let (a1, _) = ffi_pointers(id1).unwrap();
        let (a2, _) = ffi_pointers(id2).unwrap();
        let (a3, _) = ffi_pointers(id3).unwrap();
        assert_ne!(a1, a2);
        assert_ne!(a2, a3);
        assert_ne!(a1, a3);

        release_batch(id2);
        assert_eq!(active_export_count(), 2);
        assert!(ffi_pointers(id1).is_some());
        assert!(ffi_pointers(id3).is_some());

        release_batch(id1);
        release_batch(id3);
    }

    #[test]
    fn export_many_batches_then_release_all() {
        let _lock = lock();
        release_all();
        let ids: Vec<u32> = (0..100)
            .map(|_| export_batch(make_batch(10)).unwrap())
            .collect();
        assert_eq!(active_export_count(), 100);

        for id in &ids {
            release_batch(*id);
        }
        assert_eq!(active_export_count(), 0);
    }
}
