import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { readFileSync } from 'node:fs';
import { bakeInstaller } from './installer/bake.mjs';

// The release commit bumps only packages/server/Cargo.toml, so the app shows that version (client and server ship together).
const appVersion = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(new URL('../server/Cargo.toml', import.meta.url), 'utf8'))?.[1] ?? 'dev';

// /i is the installer with this client's address baked in as its default (TERMENBOX_CLIENT_ORIGIN; unset keeps the hosted one).
const installer = () => ({
  name: 'termenbox-installer',
  generateBundle() {
    const source = readFileSync(new URL('./installer/install.sh', import.meta.url), 'utf8');
    this.emitFile({ type: 'asset', fileName: 'i', source: bakeInstaller(source, process.env.TERMENBOX_CLIENT_ORIGIN) });
  },
});

export default defineConfig({
  define: { __APP_VERSION__: JSON.stringify(appVersion) },
  plugins: [tailwindcss(), svelte(), installer()],
  build: {
    outDir: 'dist',
    sourcemap: true,
  },
  server: {
    port: 3000,
  },
});
