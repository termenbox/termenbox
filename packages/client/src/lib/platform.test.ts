import { afterEach, describe, expect, it, vi } from 'vitest';
import { nativeHost } from './platform';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('nativeHost', () => {
  it('is null in the browser', () => {
    expect(nativeHost()).toBeNull();
  });

  it('returns the wrapper host', () => {
    const host = { keepAwake: vi.fn(), allowSleep: vi.fn(), onBack: vi.fn() };
    vi.stubGlobal('__termenboxPlatform', host);
    expect(nativeHost()).toBe(host);
  });

  it('keeps a host that offers only some capabilities', () => {
    const host = { onBack: vi.fn() };
    vi.stubGlobal('__termenboxPlatform', host);
    expect(nativeHost()).toBe(host);
  });
});
