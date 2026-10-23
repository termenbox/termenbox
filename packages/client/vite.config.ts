import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { readFileSync } from 'node:fs';
import { bakeInstaller, CHANNELS } from './installer/bake.mjs';

// The release commit bumps only packages/server/Cargo.toml, so the app shows that version (client and server ship together).
const appVersion = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(new URL('../server/Cargo.toml', import.meta.url), 'utf8'))?.[1] ?? 'dev';

// One build per release channel, each deployed to its own origin (decision 0134). VITE_CHANNEL is stable, beta or nightly;
// VITE_APP_VERSION overrides the Cargo version for a nightly, which is stamped X.Y.Z-nightly.YYYYMMDD.
const channel = process.env.VITE_CHANNEL || 'stable';
if (!CHANNELS.includes(channel)) throw new Error(`VITE_CHANNEL must be one of ${CHANNELS.join(', ')}, got "${channel}"`);
const version = process.env.VITE_APP_VERSION || appVersion;
// Where the Pages Functions (the directory and the OAuth exchange) are: the client's own origin, unless one is named.
const apiOrigin = process.env.VITE_API_ORIGIN || process.env.TERMENBOX_CLIENT_ORIGIN || '';
const releaseKeys = readFileSync(new URL('../server/release-keys.txt', import.meta.url), 'utf8');

// /i is the installer with this client's address baked in as its default (TERMENBOX_CLIENT_ORIGIN; unset keeps the hosted one),
// and the channel and release keys of this deployment (a build without VITE_CHANNEL is stable, as before).
const installer = () => ({
  name: 'termenbox-installer',
  generateBundle() {
    const source = readFileSync(new URL('./installer/install.sh', import.meta.url), 'utf8');
    this.emitFile({ type: 'asset', fileName: 'i', source: bakeInstaller(source, process.env.TERMENBOX_CLIENT_ORIGIN, channel, releaseKeys) });
  },
});

export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify(version),
    'import.meta.env.VITE_CHANNEL': JSON.stringify(channel),
    'import.meta.env.VITE_API_ORIGIN': JSON.stringify(apiOrigin),
    'import.meta.env.VITE_APP_VERSION': JSON.stringify(version),
  },
  plugins: [tailwindcss(), svelte(), installer()],
  build: {
    outDir: 'dist',
    sourcemap: true,
  },
  server: {
    port: 3000,
  },
});
