/**
 * Zero-copy bridge between the Rust wasm engine and React.
 *
 * Reads Arrow data directly from the wasm linear memory via the Arrow C
 * Data Interface. No serde, no JSON, no fallback. If this fails, the error
 * is surfaced — that's the point.
 */

import initWasm, {
  process as wasmProcess,
  array_ptr,
  schema_ptr,
  release_batch,
  active_export_count,
  version as wasmVersion,
} from '../wasm/bitvanes_wasm.js';

import { parseRecordBatch } from 'arrow-js-ffi';
import { Table } from 'apache-arrow';

export interface PipelineConfig {
  format: string;
  scrub: { patterns: string[]; custom: { regex: string; replacement: string }[] };
  chunk: { max_tokens: number; overlap_tokens: number; tokenizer: string };
  source_label?: string;
}

export interface ChunkRow {
  chunk_index: number;
  text: string;
  token_count: number;
  source_path: string;
  heading_path: string[];
  section_kind: string;
}

let wasmMemory: WebAssembly.Memory | null = null;

export async function ensureInitialized(): Promise<void> {
  if (wasmMemory) return;
  const exports = await initWasm();
  wasmMemory = (exports as unknown as { memory: WebAssembly.Memory }).memory;
  if (!wasmMemory) throw new Error('WebAssembly.Memory not found');
}

export function getVersion(): string {
  return wasmVersion();
}

export async function processDocument(
  config: PipelineConfig,
  bytes: Uint8Array,
): Promise<{ chunks: ChunkRow[]; slotId: number }> {
  if (!wasmMemory) await ensureInitialized();

  // 1. Run the pipeline.
  const slotId = wasmProcess(config, bytes);

  // 2. Get FFI pointers.
  const arrPtr = array_ptr(slotId);
  const schPtr = schema_ptr(slotId);
  if (arrPtr === 0 || schPtr === 0) {
    throw new Error(`FFI pointers null (array=${arrPtr}, schema=${schPtr})`);
  }

  const batch = parseRecordBatch(wasmMemory!.buffer, arrPtr, schPtr);

  const chunks: ChunkRow[] = [];
  for (let i = 0; i < batch.numRows; i++) {
    chunks.push({
      chunk_index: colGet(batch.getChildAt(0), i, i),
      text: colGet(batch.getChildAt(1), i, ''),
      token_count: colGet(batch.getChildAt(2), i, 0),
      source_path: colGet(batch.getChildAt(3), i, ''),
      heading_path: readHeading(batch.getChildAt(4), i),
      section_kind: readSection(batch.getChildAt(5), i),
    });
  }

  return { chunks, slotId };
}

export function releaseSlot(slotId: number): void {
  release_batch(slotId);
}

/** Returns an Apache Arrow Table by re-reading the FFI pointers. */
export function getArrowTable(slotId: number): Table {
  if (!wasmMemory) throw new Error('wasm not initialized');
  const arrPtr = array_ptr(slotId);
  const schPtr = schema_ptr(slotId);
  if (arrPtr === 0 || schPtr === 0) throw new Error('invalid slot');
  const batch = parseRecordBatch(wasmMemory.buffer, arrPtr, schPtr);
  return new Table(batch);
}

// --- Column helpers ---

function colGet<T>(col: Vectorish | null, i: number, fallback: T): T {
  if (!col) return fallback;
  try {
    return (col.get(i) ?? fallback) as T;
  } catch {
    return fallback;
  }
}

function readHeading(col: Vectorish | null, i: number): string[] {
  if (!col || !col.isValid(i)) return [];
  try {
    const val = col.get(i);
    if (val === null || val === undefined) return [];
    if (Array.isArray(val)) return val.map(String);
    return [String(val)];
  } catch {
    return [];
  }
}

function readSection(col: Vectorish | null, i: number): string {
  if (!col) return 'paragraph';
  try {
    const val = col.get(i);
    if (typeof val === 'string') return val;
    if (typeof val === 'number') return `kind_${val}`;
    return String(val ?? 'paragraph');
  } catch {
    return 'paragraph';
  }
}

interface Vectorish {
  get(i: number): unknown;
  isValid(i: number): boolean;
  length: number;
}
