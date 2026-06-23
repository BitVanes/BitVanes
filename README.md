# @BitVanes/web

The visual ETL studio and user dashboard for BitVanes. This is a local-first web application that allows users to drag-and-drop massive files, configure semantic chunking profiles, and view real-time token distribution without ever uploading data to a third-party server.

## Key Responsibilities
* **UI/UX Layer:** Provide a clean dashboard for designing "chunking logic profiles".
* **Web Worker Orchestration:** Spin up background worker threads to run the compiled `@bitvanes/core` Wasm engine without blocking the browser UI thread.
* **Database Destination Ingestion:** Direct client-side streaming of finalized token chunks to vector databases (Pinecone, Qdrant, Supabase, etc.).

## Technical Stack
* **Framework:** Vite + TypeScript + React/Vue (or chosen SPA framework)
* **Styling:** TailwindCSS
* **Core Engine:** WebAssembly binary compiled from `BitVanes/core`

## Getting Started
```bash
npm install
npm run dev
```
