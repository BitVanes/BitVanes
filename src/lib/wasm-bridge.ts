/**
 * Main-thread bridge to the wasm engine, which runs inside a Web Worker
 * (see `engine.worker.ts`) so large documents never block the UI. Results
 * cross the worker boundary as plain, structured-cloneable values: a
 * `ChunkRow[]` and the Arrow IPC `Uint8Array` for export.
 */

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

type Res =
  | { type: 'ready'; version: string }
  | { type: 'result'; id: number; chunks: ChunkRow[]; arrowBytes: Uint8Array }
  | { type: 'error'; id: number; error: string };

let worker: Worker | null = null;
let readyPromise: Promise<void> | null = null;
let cachedVersion = '0.0.0';
let nextId = 1;
const pending = new Map<number, { resolve: (v: { chunks: ChunkRow[]; arrowBytes: Uint8Array }) => void; reject: (e: Error) => void }>();

function getWorker(): Worker {
  if (!worker) {
    worker = new Worker(new URL('./engine.worker.ts', import.meta.url), { type: 'module' });
    worker.onmessage = (e: MessageEvent<Res>) => {
      const msg = e.data;
      if (msg.type === 'ready') {
        cachedVersion = msg.version;
        return;
      }
      const entry = pending.get(msg.id);
      if (!entry) return;
      pending.delete(msg.id);
      if (msg.type === 'result') entry.resolve({ chunks: msg.chunks, arrowBytes: msg.arrowBytes });
      else entry.reject(new Error(msg.error));
    };
    worker.onerror = (e) => {
      // Surface worker startup failures to anyone waiting on init.
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
): Promise<{ chunks: ChunkRow[]; arrowBytes: Uint8Array }> {
  await ensureInitialized();
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    // Transfer the input bytes (no copy) into the worker.
    getWorker().postMessage({ type: 'process', id, config, bytes }, [bytes]);
  });
}
