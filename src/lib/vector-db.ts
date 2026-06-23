/**
 * Vector database sync: batch-upsert chunks + embeddings to Qdrant or Pinecone.
 *
 * All requests go directly from the browser to the DB API. No proxy server.
 * API keys are held in memory only — never persisted to localStorage.
 */

export interface VectorDBConfig {
  provider: 'qdrant' | 'pinecone';
  endpoint: string;
  apiKey: string;
  collection: string;
}

export interface SyncProgress {
  synced: number;
  total: number;
  failed: number;
  message?: string;
}

interface ChunkPayload {
  chunk_index: number;
  text: string;
  token_count: number;
  heading_path: string[];
  section_kind: string;
  source_path: string;
}

const BATCH_SIZE = 50;

/** Upserts chunks + embeddings to the configured vector database. */
export async function syncToVectorDB(
  chunks: ChunkPayload[],
  embeddings: number[][],
  config: VectorDBConfig,
  onProgress?: (p: SyncProgress) => void,
): Promise<void> {
  if (chunks.length !== embeddings.length) {
    throw new Error(
      `Chunk count (${chunks.length}) does not match embedding count (${embeddings.length})`,
    );
  }
  if (!config.endpoint) throw new Error('Endpoint is required');
  if (!config.collection) throw new Error('Collection name is required');

  if (config.provider === 'qdrant') {
    await syncQdrant(chunks, embeddings, config, onProgress);
  } else {
    await syncPinecone(chunks, embeddings, config, onProgress);
  }
}

async function syncQdrant(
  chunks: ChunkPayload[],
  embeddings: number[][],
  config: VectorDBConfig,
  onProgress?: (p: SyncProgress) => void,
): Promise<void> {
  let synced = 0;
  let failed = 0;

  // Try to create the collection (ignore if it already exists).
  try {
    await fetch(
      `${config.endpoint}/collections/${config.collection}`,
      {
        method: 'PUT',
        headers: jsonHeaders(config),
        body: JSON.stringify({
          vectors: { size: embeddings[0].length, distance: 'Cosine' },
        }),
      },
    );
  } catch {
    // Collection may already exist or the server doesn't support PUT.
    // Proceed with upsert — it will fail if the collection truly doesn't exist.
  }

  for (let i = 0; i < chunks.length; i += BATCH_SIZE) {
    const batch = chunks.slice(i, i + BATCH_SIZE);
    const embBatch = embeddings.slice(i, i + BATCH_SIZE);

    const points = batch.map((c, j) => ({
      id: i + j + 1,
      vector: embBatch[j],
      payload: {
        text: c.text,
        chunk_index: c.chunk_index,
        token_count: c.token_count,
        heading_path: c.heading_path,
        section_kind: c.section_kind,
        source_path: c.source_path,
      } satisfies Record<string, unknown>,
    }));

    try {
      const res = await fetch(
        `${config.endpoint}/collections/${config.collection}/points`,
        {
          method: 'PUT',
          headers: jsonHeaders(config),
          body: JSON.stringify({ points }),
        },
      );
      if (!res.ok) {
        const body = await res.text();
        throw new Error(`Qdrant ${res.status}: ${body}`);
      }
      synced += batch.length;
    } catch (e) {
      console.error(`Qdrant batch ${i} failed:`, e);
      failed += batch.length;
    }

    onProgress?.({ synced, total: chunks.length, failed });
  }
}

async function syncPinecone(
  chunks: ChunkPayload[],
  embeddings: number[][],
  config: VectorDBConfig,
  onProgress?: (p: SyncProgress) => void,
): Promise<void> {
  let synced = 0;
  let failed = 0;

  for (let i = 0; i < chunks.length; i += BATCH_SIZE) {
    const batch = chunks.slice(i, i + BATCH_SIZE);
    const embBatch = embeddings.slice(i, i + BATCH_SIZE);

    const vectors = batch.map((c, j) => ({
      id: `chunk_${i + j}`,
      values: embBatch[j],
      metadata: {
        text: c.text,
        chunk_index: c.chunk_index,
        token_count: c.token_count,
        heading_path: c.heading_path.join(' > '),
        section_kind: c.section_kind,
        source_path: c.source_path,
      } satisfies Record<string, unknown>,
    }));

    try {
      const res = await fetch(`${config.endpoint}/vectors/upsert`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Api-Key': config.apiKey,
        },
        body: JSON.stringify({ vectors }),
      });
      if (!res.ok) {
        const body = await res.text();
        throw new Error(`Pinecone ${res.status}: ${body}`);
      }
      synced += batch.length;
    } catch (e) {
      console.error(`Pinecone batch ${i} failed:`, e);
      failed += batch.length;
    }

    onProgress?.({ synced, total: chunks.length, failed });
  }
}

function jsonHeaders(config: VectorDBConfig): Record<string, string> {
  const h: Record<string, string> = { 'Content-Type': 'application/json' };
  if (config.apiKey) h['api-key'] = config.apiKey;
  return h;
}
