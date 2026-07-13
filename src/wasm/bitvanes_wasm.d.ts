/* tslint:disable */
/* eslint-disable */

/**
 * Returns the number of currently-exported (unreleased) batches. Useful
 * for leak detection from JS.
 */
export function active_export_count(): number;

/**
 * Returns the raw `FFI_ArrowArray` pointer (wasm linear memory address)
 * for the given slot ID. Returns 0 if the slot doesn't exist.
 *
 * JS reads this pointer via `DataView` on `WebAssembly.Memory.buffer`
 * + `arrow-js-ffi`'s `parseRecordBatch`.
 */
export function array_ptr(slot_id: number): number;

/**
 * Initializes the wasm module. Installs a panic hook so that Rust panics
 * produce readable stack traces in the browser console. Called
 * automatically by `wasm-bindgen`'s `start` mechanism.
 */
export function init(): void;

/**
 * Runs the full ETL pipeline on the given document bytes and returns a
 * slot ID for retrieving the zero-copy Arrow FFI pointers.
 *
 * # Parameters
 *
 * - `config_js`: A JS object matching the `PipelineConfig` JSON schema.
 *   Deserialized via `serde-wasm-bindgen` (the config bridge, not data).
 * - `bytes`: Raw document bytes (UTF-8 encoded).
 *
 * # Returns
 *
 * A `u32` slot ID. Use [`array_ptr`] and [`schema_ptr`] to obtain the
 * Arrow C Data Interface pointers, then call [`release_batch`] when done.
 *
 * # Throws
 *
 * Throws a JS `Error` if the config is invalid or the pipeline fails.
 */
export function process(config_js: any, bytes: Uint8Array): number;

/**
 * Processes a document and returns chunk data as a JS array (via serde,
 * NOT zero-copy Arrow FFI). Fallback for when `parseRecordBatch` fails.
 *
 * Each element carries the chunk text, token count, heading ancestry,
 * section kind, deterministic `chunk_id`, and the PII findings whose
 * original-text ranges overlap the chunk.
 */
export function process_chunks(config_js: any, bytes: Uint8Array): any;

/**
 * Releases the memory for an exported batch. Must be called after JS is
 * done reading the FFI pointers (typically via `FinalizationRegistry`).
 *
 * After this call, the pointers from [`array_ptr`] / [`schema_ptr`] are
 * **dangling** and must not be accessed.
 */
export function release_batch(slot_id: number): void;

/**
 * Returns the raw `FFI_ArrowSchema` pointer (wasm linear memory address)
 * for the given slot ID. Returns 0 if the slot doesn't exist.
 */
export function schema_ptr(slot_id: number): number;

/**
 * Returns the semver version of the underlying `bitvanes-core` engine.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly array_ptr: (a: number) => number;
    readonly process: (a: any, b: number, c: number) => [number, number, number];
    readonly process_chunks: (a: any, b: number, c: number) => [number, number, number];
    readonly schema_ptr: (a: number) => number;
    readonly version: () => [number, number];
    readonly init: () => void;
    readonly release_batch: (a: number) => void;
    readonly active_export_count: () => number;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
