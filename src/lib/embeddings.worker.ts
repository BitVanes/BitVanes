/**
 * Embeddings Web Worker: hosts @xenova/transformers OFF the main thread.
 *
 * The model (all-MiniLM-L6-v2, ~22 MB ONNX) loads and runs here so the UI
 * never freezes during download or inference. The main thread sends text
 * arrays and receives float-vector arrays back via structured clone.
 */

import { pipeline, env, Tensor } from '@xenova/transformers';

env.allowLocalModels = false;

const MODEL_ID = 'Xenova/all-MiniLM-L6-v2';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let extractor: any = null;

type MsgIn =
  | { type: 'init' }
  | { type: 'embed'; id: number; texts: string[] };

type MsgOut =
  | { type: 'downloading'; progress: number; detail: string }
  | { type: 'ready' }
  | { type: 'embedding_progress'; id: number; done: number; total: number }
  | { type: 'result'; id: number; embeddings: number[][] }
  | { type: 'error'; message: string };

function post(msg: MsgOut) {
  (self as unknown as Worker).postMessage(msg);
}

async function ensureModel() {
  if (extractor) return;
  post({ type: 'downloading', progress: 0, detail: 'Starting download…' });
  extractor = await pipeline('feature-extraction', MODEL_ID, {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    progress_callback: (data: any) => {
      if (data.status === 'progress') {
        post({
          type: 'downloading',
          progress: Math.round(data.progress ?? 0),
          detail: `Downloading ${data.file ?? 'model files'}…`,
        });
      }
    },
  });
  post({ type: 'ready' });
}

async function runEmbed(id: number, texts: string[]) {
  const results: number[][] = [];
  for (let i = 0; i < texts.length; i++) {
    const output = await extractor(texts[i], { pooling: 'mean', normalize: true });
    const tensor = output as Tensor;
    results.push(Array.from(tensor.data as Float32Array));
    post({ type: 'embedding_progress', id, done: i + 1, total: texts.length });
  }
  post({ type: 'result', id, embeddings: results });
}

self.onmessage = async (e: MessageEvent<MsgIn>) => {
  const msg = e.data;
  try {
    if (msg.type === 'init') {
      await ensureModel();
    } else if (msg.type === 'embed') {
      if (!extractor) await ensureModel();
      await runEmbed(msg.id, msg.texts);
    }
  } catch (err) {
    post({ type: 'error', message: String(err) });
  }
};
