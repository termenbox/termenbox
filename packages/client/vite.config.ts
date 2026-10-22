import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { readFileSync } from 'node:fs';

// The release commit bumps only packages/server/Cargo.toml, so the app shows that version (client and server ship together).
const appVersion = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(new URL('../server/Cargo.toml', import.meta.url), 'utf8'))?.[1] ?? 'dev';

export default defineConfig({
  define: { __APP_VERSION__: JSON.stringify(appVersion) },
  plugins: [tailwindcss(), svelte()],
  build: {
    outDir: 'dist',
    sourcemap: true,
  },
  server: {
    port: 3000,
  },
});
