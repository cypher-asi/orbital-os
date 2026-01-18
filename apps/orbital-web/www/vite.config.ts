import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

export default defineConfig({
  plugins: [react()],
  root: '.',
  base: '/',
  publicDir: 'public',
  // Include WASM files as assets
  assetsInclude: ['**/*.wasm'],
  build: {
    outDir: 'dist',
    sourcemap: true,
  },
  server: {
    port: 3000,
    // Required headers for SharedArrayBuffer (used by Web Workers)
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
    // Ensure proper MIME types for WASM
    fs: {
      strict: false,
    },
  },
  optimizeDeps: {
    // Exclude wasm-bindgen generated files from optimization
    exclude: ['./pkg/orbital_web.js'],
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, '.'),
    },
  },
});
