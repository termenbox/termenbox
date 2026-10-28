import { readFileSync } from 'node:fs';
import { build } from 'esbuild';
import { describe, expect, it } from 'vitest';
// @ts-expect-error plain ESM script
import { migrations, options } from '../build.mjs';

describe('the embedded Worker', () => {
  it('the committed dist/ equals a fresh build (rebuild with `pnpm build` and commit it)', async () => {
    const { outputFiles } = await build(options);
    expect(Buffer.from(outputFiles[0].contents).equals(readFileSync('dist/worker.js'))).toBe(true);
    expect(readFileSync('dist/migrations.json', 'utf8')).toBe(migrations);
  });

  it('holds no secret, no storage write and no fetch to anywhere', () => {
    const js = readFileSync('dist/worker.js', 'utf8');
    expect(js).not.toMatch(/\.put\(|\.sql|\bfetch\(["'`]http/);
    expect(JSON.parse(readFileSync('dist/migrations.json', 'utf8'))[0].new_sqlite_classes).toEqual(['Relay']);
  });
});
