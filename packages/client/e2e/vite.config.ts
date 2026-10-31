import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { resolve } from 'node:path';

// Bundles the real Connection (WebRTC and WebSocket transports) and the real AppShell + KeyRow for the
// browser end-to-end test.
export default defineConfig({
  root: __dirname,
  base: './',
  plugins: [tailwindcss(), svelte()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: { input: { index: resolve(__dirname, 'index.html'), shell: resolve(__dirname, 'shell.html'), cards: resolve(__dirname, 'cards.html'), navigation: resolve(__dirname, 'navigation.html'), gallery: resolve(__dirname, 'gallery.html'), keyrow: resolve(__dirname, 'keyrow.html'), frame: resolve(__dirname, 'frame.html'), home: resolve(__dirname, 'home.html'), decision: resolve(__dirname, 'decision.html'), activity: resolve(__dirname, 'activity.html'), failures: resolve(__dirname, 'failures.html'), reorder: resolve(__dirname, 'reorder.html'), 'acp-history': resolve(__dirname, 'acp-history.html') } },
  },
});
