// Builds the Worker the server embeds (`dist/worker.js`) and its Durable Object migration. `test/dist.test.ts` fails when
// the committed copy differs from a fresh build, so a change here is committed together with its `dist/`.
import { build } from 'esbuild';
import { writeFileSync } from 'node:fs';

export const options = {
  entryPoints: ['src/worker.ts'],
  bundle: true,
  format: 'esm',
  target: 'es2022',
  minify: true,
  legalComments: 'none',
  write: false,
  outfile: 'dist/worker.js',
};

// The class the Worker exports is `Relay`; SQLite-backed, the only kind the free plan has.
export const migrations = JSON.stringify([{ tag: 'v1', new_sqlite_classes: ['Relay'] }], null, 2) + '\n';

if (import.meta.url === `file://${process.argv[1]}`) {
  const { outputFiles } = await build(options);
  writeFileSync('dist/worker.js', outputFiles[0].contents);
  writeFileSync('dist/migrations.json', migrations);
}
