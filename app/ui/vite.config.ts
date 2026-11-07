/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

/**
 * The CSP the Tauri shell enforces. `vite preview --mode mock` serves the mock build with it,
 * so the Playwright suite proves the UI runs without inline scripts, inline <style> or eval.
 */
export const APP_CSP = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "img-src 'self' data:",
  "font-src 'self'",
  'connect-src ipc: http://ipc.localhost',
].join('; ');

export default defineConfig(({ mode }) => ({
  plugins: [svelte()],
  // Tauri serves the built files from its own origin; relative paths keep the bundle portable.
  base: './',
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  preview: {
    port: 4179,
    strictPort: true,
    headers: { 'Content-Security-Policy': APP_CSP },
  },
  build: {
    target: 'chrome120',
    outDir: mode === 'mock' ? 'dist-mock' : 'dist',
    emptyOutDir: true,
    // The CSP forbids inline data for scripts; keep fonts and images as files (font-src 'self').
    assetsInlineLimit: 0,
    // WebView2 supports modulepreload natively; the polyfill is the only fetch() in the bundle.
    modulePreload: { polyfill: false },
    sourcemap: false,
  },
  test: {
    include: ['tests/unit/**/*.test.ts'],
    environment: 'node',
  },
}));
