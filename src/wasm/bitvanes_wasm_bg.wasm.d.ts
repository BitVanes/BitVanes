/* tslint:disable */
/* eslint-disable */
export const memory: WebAssembly.Memory;
export const array_ptr: (a: number) => number;
export const process: (a: any, b: number, c: number) => [number, number, number];
export const process_chunks: (a: any, b: number, c: number) => [number, number, number];
export const schema_ptr: (a: number) => number;
export const version: () => [number, number];
export const init: () => void;
export const release_batch: (a: number) => void;
export const active_export_count: () => number;
export const __wbindgen_malloc: (a: number, b: number) => number;
export const __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
export const __wbindgen_exn_store: (a: number) => void;
export const __externref_table_alloc: () => number;
export const __wbindgen_externrefs: WebAssembly.Table;
export const __wbindgen_free: (a: number, b: number, c: number) => void;
export const __externref_table_dealloc: (a: number) => void;
export const __wbindgen_start: () => void;
