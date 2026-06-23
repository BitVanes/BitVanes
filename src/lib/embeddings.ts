/**
 * Client-side embedding generation via @xenova/transformers.
 *
 * Loads all-MiniLM-L6-v2 (384-dim, 22MB ONNX) directly in the browser.
 * No data leaves the client — the model runs via ONNX Runtime Web.
 */

import { pipeline, env, Tensor } from '@xenova/transformers';

env.allowLocalModels = false;

const MODEL_ID = 'Xenova/all-MiniLM-L6-v2';
const EMBEDDING_DIM = 384;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let extractor: any = null;
let loadingPromise: Promise<void> | null = null;

export type EmbeddingStatus =
  | { phase: 'idle' }
  | { phase: 'downloading'; progress: number; detail: string }
  | { phase: 'embedding'; done: number; total: number }
  | { phase: 'ready' }
  | { phase: 'error'; message: string };

/** Lazy-loads the embedding model. Downloads ~22MB on first call (cached by browser). */
export async function ensureEmbedder(
  onStatus?: (s: EmbeddingStatus) => void,
): Promise<void> {
  if (extractor) {
    onStatus?.({ phase: 'ready' });
    return;
  }
  if (loadingPromise) return loadingPromise;

  onStatus?.({ phase: 'downloading', progress: 0, detail: 'Starting download…' });

  loadingPromise = (async () => {
    extractor = await pipeline('feature-extraction', MODEL_ID, {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      progress_callback: (data: any) => {
        if (data.status === 'progress' && onStatus) {
          onStatus({
            phase: 'downloading',
            progress: Math.round(data.progress ?? 0),
            detail: `Downloading ${data.file ?? 'model files'}…`,
          });
        }
      },
    });
    onStatus?.({ phase: 'ready' });
  })();

  try {
    await loadingPromise;
  } catch (e) {
    loadingPromise = null;
    onStatus?.({ phase: 'error', message: String(e) });
    throw e;
  }
}

/** Generates 384-dim normalized embeddings for each text. */
export async function embed(
  texts: string[],
  onProgress?: (done: number, total: number) => void,
): Promise<number[][]> {
  if (!extractor) throw new Error('Embedder not loaded');

  const results: number[][] = [];

  for (let i = 0; i < texts.length; i++) {
    const output = await extractor(texts[i], { pooling: 'mean', normalize: true });
    const tensor = output as Tensor;
    results.push(Array.from(tensor.data as Float32Array));
    onProgress?.(i + 1, texts.length);
  }

  return results;
}

export function getEmbeddingDim(): number {
  return EMBEDDING_DIM;
}
