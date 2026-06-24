/**
 * Web Worker hosting the BitVanes wasm engine.
 *
 * The wasm pipeline + Arrow FFI read happen here so a large document never
 * freezes the UI thread. The main thread receives plain, structured-cloneable
 * results: a `ChunkRow[]` and the Arrow IPC bytes for export.
 */

import init, {
  process as wasmProcess,
  array_ptr,
  schema_ptr,
  release_batch,
  version as wasmVersion,
} from '../wasm/bitvanes_wasm.js';
import { parseRecordBatch } from 'arrow-js-ffi';
import { RecordBatchStreamWriter } from 'apache-arrow';
import type { PipelineConfig, ChunkRow } from './wasm-bridge';

type Req =
  | { type: 'init' }
  | { type: 'process'; id: number; config: PipelineConfig; bytes: Uint8Array };

let wasmMemory: WebAssembly.Memory | null = null;
let pendingInit: Promise<void> | null = null;

function ensureReady(): Promise<void> {
  if (wasmMemory) return Promise.resolve();
  if (!pendingInit) {
    pendingInit = (async () => {
      const exports = await init();
      const memory = (exports as unknown as { memory: WebAssembly.Memory }).memory;
      if (!memory) throw new Error('WebAssembly.Memory not found');
      wasmMemory = memory;
    })().catch((e) => {
      pendingInit = null;
      throw e;
    });
  }
  return pendingInit;
}

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
    if (typeof (val as { toArray?: () => unknown }).toArray === 'function') {
      return Array.from((val as { toArray: () => Iterable<unknown> }).toArray(), String);
    }
    if (typeof (val as { [Symbol.iterator]?: () => Iterator<unknown> })[Symbol.iterator] === 'function') {
      return Array.from(val as Iterable<unknown>, String);
    }
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

async function run(config: PipelineConfig, bytes: Uint8Array): Promise<{ chunks: ChunkRow[]; arrowBytes: Uint8Array }> {
  await ensureReady();
  const slotId = wasmProcess(config, bytes);
  const arrPtr = array_ptr(slotId);
  const schPtr = schema_ptr(slotId);
  if (arrPtr === 0 || schPtr === 0) {
    release_batch(slotId);
    throw new Error(`FFI pointers null (array=${arrPtr}, schema=${schPtr})`);
  }

  const batch = parseRecordBatch(wasmMemory!.buffer, arrPtr, schPtr);
  release_batch(slotId);

  const chunks: ChunkRow[] = [];
  for (let i = 0; i < batch.numRows; i++) {
    chunks.push({
      chunk_index: colGet(batch.getChildAt(0), i, i),
      text: colGet(batch.getChildAt(1), i, ''),
      token_count: colGet(batch.getChildAt(2), i, 0),
      source_path: colGet(batch.getChildAt(3), i, ''),
      heading_path: readHeading(batch.getChildAt(4), i),
      section_kind: readSection(batch.getChildAt(5), i),
      char_offset_start: colGet(batch.getChildAt(6), i, 0),
      char_offset_end: colGet(batch.getChildAt(7), i, 0),
    });
  }

  const writer = new RecordBatchStreamWriter();
  writer.write(batch);
  writer.finish();
  const arrowBytes = writer.toUint8Array(true);
  return { chunks, arrowBytes };
}

self.onmessage = async (e: MessageEvent<Req>) => {
  const msg = e.data;
  if (msg.type === 'init') {
    try {
      await ensureReady();
      (self as unknown as Worker).postMessage({ type: 'ready', version: wasmVersion() });
    } catch (err) {
      (self as unknown as Worker).postMessage({ type: 'error', id: -1, error: String(err) });
    }
    return;
  }
  if (msg.type === 'process') {
    try {
      const { chunks, arrowBytes } = await run(msg.config, msg.bytes);
      (self as unknown as Worker).postMessage(
        { type: 'result', id: msg.id, chunks, arrowBytes },
        [arrowBytes.buffer],
      );
    } catch (err) {
      (self as unknown as Worker).postMessage({ type: 'error', id: msg.id, error: String(err) });
    }
  }
};
