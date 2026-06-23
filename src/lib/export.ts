/**
 * Export utilities: download chunks in various formats.
 *
 * - JSON: universal, human-readable. Feed into Python/Node RAG scripts.
 * - Arrow IPC: binary columnar. DuckDB/Polars/LanceDB direct ingestion.
 * - Profile: pipeline config JSON. CLI replay (Milestone 4 contract).
 */

import { RecordBatchStreamWriter } from 'apache-arrow';
import { getArrowTable, type ChunkRow, type PipelineConfig } from './wasm-bridge';

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

/** Downloads the Arrow RecordBatch as an IPC stream file (.arrow). */
export function exportArrowIPC(slotId: number, fileName: string): void {
  const table = getArrowTable(slotId);
  const writer = RecordBatchStreamWriter.writeAll(table);
  const bytes = writer.toUint8Array(true);
  // Copy into a fresh ArrayBuffer-backed Uint8Array (Blob rejects SharedArrayBuffer).
  downloadBlob(
    new Blob([new Uint8Array(bytes)], { type: 'application/octet-stream' }),
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
