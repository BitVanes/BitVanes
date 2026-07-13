/**
 * Main-thread bridge to the wasm engine, which runs inside a Web Worker
 * (see `engine.worker.ts`) so large documents never block the UI. Results
 * cross the worker boundary as plain, structured-cloneable values: a
 * `ChunkRow[]`, the Arrow IPC `Uint8Array` for export, and coarse timing.
 */

export interface PiiFinding {
  entity: string;
  offset_start: number;
  offset_end: number;
  confidence: number;
  anchors_hit: string[];
}

export interface PipelineConfig {
  format: string;
  scrub: {
    patterns: string[];
    custom: { regex: string; replacement: string }[];
    anchor_window?: number;
    min_confidence?: number;
  };
  chunk: {
    max_tokens: number;
    overlap_tokens: number;
    tokenizer: string;
    strategy?: { structural?: unknown } | { semantic: { similarity_threshold: number } };
  };
  source_label?: string;
}

export interface ChunkRow {
  chunk_index: number;
  chunk_id: string;
  text: string;
  token_count: number;
  source_path: string;
  heading_path: string[];
  section_kind: string;
  /** Half-open [start, end) char offsets into the scrubbed document text. */
  char_offset_start: number;
  char_offset_end: number;
  /** PII findings overlapping this chunk (offsets into original text). */
  pii: PiiFinding[];
}

export interface ProcessResult {
  chunks: ChunkRow[];
  arrowBytes: Uint8Array;
  elapsedMs: number;
  inputBytes: number;
  findingCount: number;
}

export interface ProgressInfo {
  phase: 'start' | 'done';
  id: number;
  byteLength?: number;
}

type Res =
  | { type: 'ready'; version: string }
  | { type: 'start'; id: number; byteLength: number }
  | { type: 'result'; id: number; chunks: ChunkRow[]; arrowBytes: Uint8Array; elapsedMs: number; inputBytes: number; findingCount: number }
  | { type: 'error'; id: number; error: string };

let worker: Worker | null = null;
let readyPromise: Promise<void> | null = null;
let cachedVersion = '0.0.0';
let nextId = 1;
const pending = new Map<
  number,
  { resolve: (v: ProcessResult) => void; reject: (e: Error) => void; onStart?: (byteLength: number) => void }
>();

function getWorker(): Worker {
  if (!worker) {
    worker = new Worker(new URL('./engine.worker.ts', import.meta.url), { type: 'module' });
    worker.onmessage = (e: MessageEvent<Res>) => {
      const msg = e.data;
      if (msg.type === 'ready') {
        cachedVersion = msg.version;
        return;
      }
      if (msg.type === 'start') {
        const entry = pending.get(msg.id);
        entry?.onStart?.(msg.byteLength);
        return;
      }
      const entry = pending.get(msg.id);
      if (!entry) return;
      pending.delete(msg.id);
      if (msg.type === 'result') {
        entry.resolve({
          chunks: msg.chunks,
          arrowBytes: msg.arrowBytes,
          elapsedMs: msg.elapsedMs,
          inputBytes: msg.inputBytes,
          findingCount: msg.findingCount,
        });
      } else {
        entry.reject(new Error(msg.error));
      }
    };
    worker.onerror = (e) => {
      const err = new Error(`engine worker error: ${e.message}`);
      for (const entry of pending.values()) entry.reject(err);
      pending.clear();
      readyPromise = null;
    };
  }
  return worker;
}

export function ensureInitialized(): Promise<void> {
  if (readyPromise) return readyPromise;
  readyPromise = new Promise<void>((resolve, reject) => {
    const w = getWorker();
    const onReady = (e: MessageEvent<Res>) => {
      if (e.data.type === 'ready') {
        cachedVersion = e.data.version;
        w.removeEventListener('message', onReady);
        resolve();
      } else if (e.data.type === 'error' && e.data.id === -1) {
        w.removeEventListener('message', onReady);
        reject(new Error(e.data.error));
      }
    };
    w.addEventListener('message', onReady);
    w.postMessage({ type: 'init' });
  });
  return readyPromise;
}

export function getVersion(): string {
  return cachedVersion;
}

export async function processDocument(
  config: PipelineConfig,
  bytes: Uint8Array,
  onStart?: (byteLength: number) => void,
): Promise<ProcessResult> {
  await ensureInitialized();
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject, onStart });
    getWorker().postMessage({ type: 'process', id, config, bytes }, [bytes.buffer]);
  });
}
