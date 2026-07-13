/**
 * Client-side embedding generation via a Web Worker (off-main-thread).
 *
 * The worker hosts @xenova/transformers and the all-MiniLM-L6-v2 model.
 * This module is the main-thread bridge: it lazily creates the worker,
 * sends init/embed messages, and resolves promises when the worker responds.
 *
 * Zero data leaves the client — the model runs via ONNX Runtime Web inside
 * the worker.
 */

export type EmbeddingStatus =
  | { phase: 'idle' }
  | { phase: 'downloading'; progress: number; detail: string }
  | { phase: 'embedding'; done: number; total: number }
  | { phase: 'ready' }
  | { phase: 'error'; message: string };

const EMBEDDING_DIM = 384;

type MsgOut =
  | { type: 'downloading'; progress: number; detail: string }
  | { type: 'ready' }
  | { type: 'embedding_progress'; id: number; done: number; total: number }
  | { type: 'result'; id: number; embeddings: number[][] }
  | { type: 'error'; message: string };

let worker: Worker | null = null;
let ready = false;
let loadingPromise: Promise<void> | null = null;
let nextId = 1;
const pending = new Map<number, { resolve: (v: number[][]) => void; reject: (e: Error) => void }>();
let statusCallback: ((s: EmbeddingStatus) => void) | null = null;

function getWorker(): Worker {
  if (!worker) {
    worker = new Worker(new URL('./embeddings.worker.ts', import.meta.url), { type: 'module' });
    worker.onmessage = (e: MessageEvent<MsgOut>) => {
      const msg = e.data;
      if (msg.type === 'downloading') {
        statusCallback?.({ phase: 'downloading', progress: msg.progress, detail: msg.detail });
      } else if (msg.type === 'ready') {
        ready = true;
        statusCallback?.({ phase: 'ready' });
      } else if (msg.type === 'embedding_progress') {
        const entry = pending.get(msg.id);
        if (entry) {
          statusCallback?.({ phase: 'embedding', done: msg.done, total: msg.total });
        }
      } else if (msg.type === 'result') {
        const entry = pending.get(msg.id);
        if (entry) {
          pending.delete(msg.id);
          entry.resolve(msg.embeddings);
        }
      } else if (msg.type === 'error') {
        // Reject all pending on fatal error.
        for (const entry of pending.values()) entry.reject(new Error(msg.message));
        pending.clear();
        statusCallback?.({ phase: 'error', message: msg.message });
      }
    };
    worker.onerror = (e) => {
      for (const entry of pending.values()) entry.reject(new Error(e.message));
      pending.clear();
      statusCallback?.({ phase: 'error', message: e.message });
    };
  }
  return worker;
}

/** Lazy-loads the embedding model in the worker. Downloads ~22MB on first call. */
export async function ensureEmbedder(
  onStatus?: (s: EmbeddingStatus) => void,
): Promise<void> {
  statusCallback = onStatus ?? null;
  if (ready) {
    onStatus?.({ phase: 'ready' });
    return;
  }
  if (loadingPromise) return loadingPromise;

  loadingPromise = new Promise<void>((resolve, reject) => {
    const w = getWorker();
    const onReady = (e: MessageEvent<MsgOut>) => {
      if (e.data.type === 'ready') {
        w.removeEventListener('message', onReady);
        resolve();
      } else if (e.data.type === 'error') {
        w.removeEventListener('message', onReady);
        loadingPromise = null;
        reject(new Error(e.data.message));
      }
    };
    w.addEventListener('message', onReady);
    w.postMessage({ type: 'init' });
  });

  try {
    await loadingPromise;
  } catch (e) {
    loadingPromise = null;
    throw e;
  }
}

/** Generates 384-dim normalized embeddings for each text via the worker. */
export async function embed(
  texts: string[],
  onProgress?: (done: number, total: number) => void,
): Promise<number[][]> {
  if (!ready && !loadingPromise) {
    throw new Error('Embedder not loaded — call ensureEmbedder first');
  }
  if (loadingPromise) await loadingPromise;

  const id = nextId++;
  const savedCallback = statusCallback;
  if (onProgress) {
    statusCallback = (s) => {
      if (s.phase === 'embedding') onProgress(s.done, s.total);
    };
  }

  return new Promise<number[][]>((resolve, reject) => {
    pending.set(id, {
      resolve: (v) => {
        statusCallback = savedCallback;
        resolve(v);
      },
      reject: (e) => {
        statusCallback = savedCallback;
        reject(e);
      },
    });
    getWorker().postMessage({ type: 'embed', id, texts });
  });
}

export function getEmbeddingDim(): number {
  return EMBEDDING_DIM;
}
