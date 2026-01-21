import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./test/setup.ts'],
    include: ['**/*.test.{ts,tsx}', '**/__tests__/**/*.{ts,tsx}'],
    exclude: ['node_modules', 'dist', 'pkg'],
    coverage: {
      reporter: ['text', 'html'],
      exclude: ['node_modules', 'test/**', 'pkg/**'],
    },
  },
});
