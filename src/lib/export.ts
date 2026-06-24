/**
 * Export utilities: download chunks in various formats.
 *
 * - JSON: universal, human-readable. Feed into Python/Node RAG scripts.
 * - Arrow IPC: binary columnar. DuckDB/Polars/LanceDB direct ingestion.
 * - Profile: pipeline config JSON. CLI replay (Milestone 4 contract).
 */

import type { ChunkRow, PipelineConfig } from './wasm-bridge';

/** Triggers a browser download of a Blob with the given filename. */
function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

/** Strips a file extension to get the base name for export filenames. */
function baseName(fileName: string): string {
  return fileName.replace(/\.[^.]+$/, '') || 'document';
}

/** Quotes a CSV cell per RFC 4180. */
function csvCell(v: unknown): string {
  const s = v == null ? '' : String(v);
  if (/[",\n]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
  return s;
}

/** Downloads chunks as a JSON file. */
export function exportJSON(chunks: ChunkRow[], fileName: string): void {
  const data = chunks.map((c) => ({
    chunk_index: c.chunk_index,
    text: c.text,
    token_count: c.token_count,
    heading_path: c.heading_path,
    section_kind: c.section_kind,
  }));
  const json = JSON.stringify(data, null, 2);
  downloadBlob(
    new Blob([json], { type: 'application/json' }),
    `${baseName(fileName)}.chunks.json`,
  );
}

/** Downloads chunks as a CSV file (heading_path joined for flatness). */
export function exportCSV(chunks: ChunkRow[], fileName: string): void {
  const header = [
    'chunk_index',
    'text',
    'token_count',
    'source_path',
    'heading_path',
    'section_kind',
  ];
  const rows = chunks.map((c) =>
    [
      c.chunk_index,
      c.text,
      c.token_count,
      c.source_path,
      c.heading_path.join(' › '),
      c.section_kind,
    ]
      .map(csvCell)
      .join(','),
  );
  const csv = [header.join(','), ...rows].join('\n');
  downloadBlob(
    new Blob([csv], { type: 'text/csv' }),
    `${baseName(fileName)}.chunks.csv`,
  );
}

/** Downloads chunks + their embeddings as JSONL (one record per line).
 *  Each record: { chunk_index, text, token_count, heading_path, section_kind,
 *                 source_path, embedding: number[] }. */
export function exportJSONL(
  chunks: ChunkRow[],
  embeddings: number[][],
  fileName: string,
): void {
  if (chunks.length !== embeddings.length) {
    throw new Error(
      `chunk count (${chunks.length}) != embedding count (${embeddings.length})`,
    );
  }
  const lines = chunks.map((c, i) =>
    JSON.stringify({
      chunk_index: c.chunk_index,
      text: c.text,
      token_count: c.token_count,
      heading_path: c.heading_path,
      section_kind: c.section_kind,
      source_path: c.source_path,
      embedding: embeddings[i],
    }),
  );
  downloadBlob(
    new Blob([lines.join('\n')], { type: 'application/x-ndjson' }),
    `${baseName(fileName)}.chunks.jsonl`,
  );
}

/** Downloads the Arrow IPC bytes as a .arrow file. */
export function exportArrowIPC(arrowBytes: Uint8Array, fileName: string): void {
  // Copy into a fresh ArrayBuffer-backed Uint8Array (Blob rejects SharedArrayBuffer).
  downloadBlob(
    new Blob([new Uint8Array(arrowBytes)], { type: 'application/octet-stream' }),
    `${baseName(fileName)}.arrow`,
  );
}

/** Downloads the pipeline config as profile.json (for CLI replay). */
export function exportProfile(config: PipelineConfig, fileName: string): void {
  const profile = {
    ...config,
    // Strip the source_label — it's per-file, not part of the profile.
    source_label: undefined,
  };
  const json = JSON.stringify(profile, null, 2);
  downloadBlob(
    new Blob([json], { type: 'application/json' }),
    `${baseName(fileName)}.profile.json`,
  );
}

/** Parses a profile.json File into a PipelineConfig (for profile import). */
export async function importProfile(file: File): Promise<PipelineConfig> {
  const text = await file.text();
  const parsed = JSON.parse(text) as Partial<PipelineConfig>;
  if (typeof parsed !== 'object' || parsed === null) {
    throw new Error('Profile is not a JSON object');
  }
  return parsed as PipelineConfig;
}
