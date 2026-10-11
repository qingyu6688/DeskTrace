import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [vue()],
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
    hmr: { host: '127.0.0.1', port: 1421 },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'tests/**/*.test.ts'],
    clearMocks: true,
  },
});
