import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { loadCapture } from './terminal-replay.mjs';

const dir = resolve(import.meta.dirname, 'fixtures');

describe('the installer capture', () => {
  it('has a timing that covers every byte of the typescript', () => {
    const { bytes, chunks } = loadCapture(dir);
    expect(chunks.length).toBeGreaterThan(3);
    expect(chunks.reduce((a, [, n]) => a + n, 0)).toBe(bytes.length);
  });

  it('holds no real address: no dotted quad, and the link names only documentation addresses', () => {
    const text = readFileSync(resolve(dir, 'installer.typescript'), 'utf8');
    expect(text.match(/\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}/g) ?? []).toEqual([]);
    const payload = /#\/pair\/([A-Za-z0-9_-]+)/.exec(text)[1];
    const bundle = Buffer.from(payload, 'base64url').toString('latin1');
    for (const ip of bundle.match(/\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}/g) ?? []) {
      expect(ip, `${ip} in the pairing link`).toMatch(/^(192\.0\.2|198\.51\.100|203\.0\.113)\./);
    }
  });

  it('ends with the pairing block the video asserts on', () => {
    const text = readFileSync(resolve(dir, 'installer.typescript'), 'utf8');
    expect(text).toContain('Scan this, open the link');
    expect(text).toMatch(/https:\/\/app\.termenbox\.dev\/#\/pair\/\S+/);
  });
});
