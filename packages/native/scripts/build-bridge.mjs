import { build } from 'esbuild';

// One classic script per mobile platform web dir. The client reads it as
// globalThis.__termenboxPlatform; it is injected after `cap sync`/`cap copy`.
await build({
  entryPoints: ['bridge/mobile.ts'],
  bundle: true,
  format: 'iife',
  target: 'es2022',
  outfile: 'bridge/dist/bridge.js',
  legalComments: 'none',
  logLevel: 'info',
});
