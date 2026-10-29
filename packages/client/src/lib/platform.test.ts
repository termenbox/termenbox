import { afterEach, describe, expect, it, vi } from 'vitest';
import { nativeHost } from './platform';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('nativeHost', () => {
  it('is null in the browser', () => {
    expect(nativeHost()).toBeNull();
  });

  it('returns the wrapper host when every capability is present', () => {
    const host = { keepAwake: vi.fn(), allowSleep: vi.fn(), onBack: vi.fn() };
    vi.stubGlobal('__termenboxPlatform', host);
    expect(nativeHost()).toBe(host);
  });

  it('ignores a partial host', () => {
    vi.stubGlobal('__termenboxPlatform', { keepAwake: vi.fn(), allowSleep: vi.fn() });
    expect(nativeHost()).toBeNull();
  });
});
