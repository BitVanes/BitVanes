/**
 * Web Worker hosting the BitVanes wasm engine.
 *
 * The wasm pipeline + Arrow FFI read happen here so a large document never
 * freezes the UI thread. The main thread receives plain, structured-cloneable
 * results: a `ChunkRow[]` (now including chunk_id + PII findings) and the
 * Arrow IPC bytes for export.
 *
 * Column layout (positional) — 11 columns as of v0.3.0:
 *  0 chunk_index        1 chunk_id         2 text           3 token_count
 *  4 source_path        5 heading_path     6 section_kind   7 char_offset_start
 *  8 char_offset_end    9 pii_metadata    10 embedding
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
import type { PipelineConfig, ChunkRow, PiiFinding } from './wasm-bridge';

type Req =
  | { type: 'init' }
  | { type: 'process'; id: number; config: PipelineConfig; bytes: Uint8Array };

type MsgOut =
  | { type: 'ready'; version: string }
  | { type: 'start'; id: number; byteLength: number }
  | { type: 'result'; id: number; chunks: ChunkRow[]; arrowBytes: Uint8Array; elapsedMs: number; inputBytes: number; findingCount: number }
  | { type: 'error'; id: number; error: string };

let wasmMemory: WebAssembly.Memory | null = null;
let pendingInit: Promise<void> | null = null;

function post(msg: MsgOut, transfer: Transferable[] = []) {
  (self as unknown as Worker).postMessage(msg, transfer);
}

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

interface Vectorish {
  get(i: number): unknown;
  isValid(i: number): boolean;
  length: number;
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

/**
 * Reads the `pii_metadata` List<Struct> column for row `i`.
 *
 * Arrow JS represents a List<Struct> value as an iterable of StructRow
 * objects. StructRow supports bracket access (row['entity']) and is also
 * iterable itself. This helper is defensive: any read failure produces an
 * empty array so the UI degrades gracefully.
 */
function readPiiMetadata(col: Vectorish | null, i: number): PiiFinding[] {
  if (!col || !col.isValid(i)) return [];
  try {
    const listVal = col.get(i);
    if (listVal === null || listVal === undefined) return [];

    let items: unknown[];
    if (Array.isArray(listVal)) {
      items = listVal;
    } else if (typeof (listVal as { [Symbol.iterator]?: () => Iterator<unknown> })[Symbol.iterator] === 'function') {
      items = Array.from(listVal as Iterable<unknown>);
    } else {
      return [];
    }

    const findings: PiiFinding[] = [];
    for (const row of items) {
      if (row === null || row === undefined) continue;
      const get = (k: string): unknown => {
        const r = row as Record<string, unknown>;
        if (k in r) return r[k];
        if (typeof (row as { get?: (k: string) => unknown }).get === 'function') {
          return (row as { get: (k: string) => unknown }).get(k);
        }
        return undefined;
      };
      const anchorsRaw = get('anchors');
      let anchors: string[];
      if (Array.isArray(anchorsRaw)) anchors = anchorsRaw.map(String);
      else if (anchorsRaw && typeof (anchorsRaw as { [Symbol.iterator]?: () => Iterator<unknown> })[Symbol.iterator] === 'function') {
        anchors = Array.from(anchorsRaw as Iterable<unknown>, String);
      } else anchors = [];

      findings.push({
        entity: String(get('entity') ?? 'unknown'),
        confidence: Number(get('confidence') ?? 0),
        offset_start: Number(get('offset_start') ?? 0),
        offset_end: Number(get('offset_end') ?? 0),
        anchors_hit: anchors,
      });
    }
    return findings;
  } catch {
    return [];
  }
}

async function run(
  config: PipelineConfig,
  bytes: Uint8Array,
  id: number,
): Promise<{ chunks: ChunkRow[]; arrowBytes: Uint8Array; findingCount: number }> {
  await ensureReady();
  const t0 = performance.now();
  const slotId = wasmProcess(config, bytes);
  const arrPtr = array_ptr(slotId);
  const schPtr = schema_ptr(slotId);
  if (arrPtr === 0 || schPtr === 0) {
    release_batch(slotId);
    throw new Error(`FFI pointers null (array=${arrPtr}, schema=${schPtr})`);
  }

  const batch = parseRecordBatch(wasmMemory!.buffer, arrPtr, schPtr);
  release_batch(slotId);

  const elapsedMs = performance.now() - t0;

  const chunks: ChunkRow[] = [];
  let findingCount = 0;
  for (let i = 0; i < batch.numRows; i++) {
    const pii = readPiiMetadata(batch.getChildAt(9) as Vectorish | null, i);
    findingCount += pii.length;
    chunks.push({
      chunk_index: colGet(batch.getChildAt(0) as Vectorish | null, i, i),
      chunk_id: colGet(batch.getChildAt(1) as Vectorish | null, i, ''),
      text: colGet(batch.getChildAt(2) as Vectorish | null, i, ''),
      token_count: colGet(batch.getChildAt(3) as Vectorish | null, i, 0),
      source_path: colGet(batch.getChildAt(4) as Vectorish | null, i, ''),
      heading_path: readHeading(batch.getChildAt(5) as Vectorish | null, i),
      section_kind: readSection(batch.getChildAt(6) as Vectorish | null, i),
      char_offset_start: colGet(batch.getChildAt(7) as Vectorish | null, i, 0),
      char_offset_end: colGet(batch.getChildAt(8) as Vectorish | null, i, 0),
      pii,
    });
  }

  const writer = new RecordBatchStreamWriter();
  writer.write(batch);
  writer.finish();
  const arrowBytes = writer.toUint8Array(true);
  return { chunks, arrowBytes, findingCount };
}

self.onmessage = async (e: MessageEvent<Req>) => {
  const msg = e.data;
  if (msg.type === 'init') {
    try {
      await ensureReady();
      post({ type: 'ready', version: wasmVersion() });
    } catch (err) {
      post({ type: 'error', id: -1, error: String(err) });
    }
    return;
  }
  if (msg.type === 'process') {
    post({ type: 'start', id: msg.id, byteLength: msg.bytes.length });
    try {
      const t0 = performance.now();
      const { chunks, arrowBytes, findingCount } = await run(msg.config, msg.bytes, msg.id);
      const elapsedMs = performance.now() - t0;
      post(
        { type: 'result', id: msg.id, chunks, arrowBytes, elapsedMs, inputBytes: msg.bytes.length, findingCount },
        [arrowBytes.buffer],
      );
    } catch (err) {
      post({ type: 'error', id: msg.id, error: String(err) });
    }
  }
};
