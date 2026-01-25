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
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        desktop: resolve(__dirname, 'desktop/index.html'),
      },
    },
  },
  server: {
    port: 3000,
    // Required headers for SharedArrayBuffer (used by Web Workers)
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'credentialless',
    },
    // Ensure proper MIME types for WASM
    fs: {
      strict: false,
    },
  },
  optimizeDeps: {
    // Exclude wasm-bindgen generated files from optimization
    // Also exclude npm-linked packages so Vite uses the linked source
    exclude: ['./pkg/zos_supervisor_web.js', './pkg-desktop/zos_desktop.js', '@cypher-asi/zui'],
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, '.'),
      '@desktop': resolve(__dirname, 'desktop'),
      '@components': resolve(__dirname, 'components'),
      '@apps': resolve(__dirname, 'apps'),
    },
  },
});
