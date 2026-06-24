import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // Cross-origin isolation (COOP/COEP) is NOT enabled: it would block the
  // cross-origin Xenova ONNX model download from the HuggingFace CDN in dev.
  // Single-threaded wasm + onnxruntime-web work without SharedArrayBuffer.
  // To enable SAB multi-threading later, set COEP to "credentialless" and
  // COOP to "same-origin" (credentialless keeps cross-origin CDN loads working).
  server: {
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
