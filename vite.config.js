import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  // Tauri serves `dist/` (tauri.conf.json `frontendDist`); the web host too.
  build: { outDir: 'dist', emptyOutDir: true },
  server: { port: 5173, strictPort: true },
  clearScreen: false,
  resolve: process.env.VITEST ? { conditions: ['browser'] } : undefined,
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.js'],
    reporters: ['default', 'junit'],
    outputFile: { junit: 'artifacts/vitest-junit.xml' },
  },
});
