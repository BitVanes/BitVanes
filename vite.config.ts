import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // Required for SharedArrayBuffer (future multi-threaded wasm).
  // Not needed for MVP single-threaded mode.
  server: {
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
    fs: {
      // Allow importing from the sibling core repo.
      allow: ['..'],
    },
  },
  optimizeDeps: {
    // Don't pre-bundle the wasm module — it needs to be loaded as-is.
    exclude: ['./src/wasm/bitvanes_wasm.js'],
  },
});
