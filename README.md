# @BitVanes/web

The visual ETL studio for BitVanes — a local-first web app that lets you
drag-and-drop documents, configure semantic chunking profiles, view token
distribution, and export results or sync them to a vector database. Files are
processed entirely in your browser; nothing is uploaded to a server.

## Architecture

- **Framework:** Vite 6 + TypeScript + React 19.
- **Engine:** the `@bitvanes/core` WebAssembly binary (compiled from Rust).
- **Styling:** hand-written CSS with a small design-token system
  (`src/index.css`) — **not** Tailwind.
- **Off-main-thread processing:** the wasm engine runs inside a dedicated
  [`Web Worker`](src/lib/engine.worker.ts) so large documents never block the
  UI. Arrow data is read via zero-copy FFI (`arrow-js-ffi`) inside the worker,
  then returned to the main thread as plain JS values.
- **PDF:** Mozilla PDF.js extracts text client-side (`src/lib/pdf.ts`) before
  the engine sees it (text-layer only; scanned image PDFs are unsupported).
- **Embeddings:** on-device via `@xenova/transformers` (all-MiniLM-L6-v2,
  384-dim), downloaded and cached in the browser.
- **Vector DB sync:** direct browser `fetch` to a user-supplied database.

## Features

- Drag-and-drop upload (`.pdf`, `.md`, `.txt`, `.html`, `.json`)
- Format / tokenizer / max-tokens / **overlap** / PII-scrub configuration
- **Custom regex PII patterns** (regex + replacement, arbitrary count)
- Chunk preview table with heading ancestry and section kind
- Token-distribution histogram
- Export to **JSON**, **CSV**, **Arrow IPC**, or **JSONL with embeddings**
- **Profile export/import** — a profile JSON that `bitvanes-cli` replays
  byte-for-byte (`bitvanes -c profile.json -i ./docs/`)
- On-device embedding generation (all-MiniLM-L6-v2, 384-dim)
- Sync embeddings + chunks to a vector DB

## Vector database support

| Provider | Browser `fetch` | Notes |
|----------|-----------------|-------|
| **Qdrant** | ✅ Works | Sends permissive CORS headers; collection auto-created. |
| **Pinecone** | ⚠️ CORS-limited | The data plane generally does not allow direct browser `fetch`; route through a proxy/edge function for production. |

API keys are held in memory only and never persisted. (Supabase is not
currently wired up.)

## Getting started

```bash
npm install
npm run sync-wasm   # copy the rebuilt @bitvanes/core wasm pkg into src/wasm/
npm run dev
```

Build for production:

```bash
npm run build      # tsc -b && vite build
npm run preview
```

## Notes

- Cross-origin isolation (COOP/COEP) is intentionally **not** enabled in dev,
  because it would block the cross-origin Xenova model download. Single-threaded
  wasm + onnxruntime-web run fine without SharedArrayBuffer.
- The wasm binary lives under `src/wasm/` (gitignored build artifact) and is
  refreshed from `../core/crates/wasm/pkg` via `npm run sync-wasm`.
