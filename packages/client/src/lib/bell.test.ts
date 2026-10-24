import { describe, expect, it } from 'vitest';
import { bellEnabled, setBellEnabled } from './bell';

describe('bell setting', () => {
  it('is on unless the owner muted it', () => {
    const store = new Map<string, string>();
    const storage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
    expect(bellEnabled(storage)).toBe(true);
    setBellEnabled(false, storage);
    expect(bellEnabled(storage)).toBe(false);
    setBellEnabled(true, storage);
    expect(bellEnabled(storage)).toBe(true);
  });
});

describe('needs-you sound', () => {
  it('reports unsupported where there is no Web Audio', async () => {
    const { playChime } = await import('./bell');
    expect(playChime('bloop', 'ask')).toBe('unsupported');
  });
});
