import { afterEach, describe, expect, it, vi } from 'vitest';

async function freshModule() {
  vi.resetModules();
  return import('./keepAwake');
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('setKeepAwake', () => {
  it('asks the wrapper host and ignores repeated calls with the same value', async () => {
    const host = {
      keepAwake: vi.fn().mockResolvedValue(undefined),
      allowSleep: vi.fn().mockResolvedValue(undefined),
    };
    vi.stubGlobal('__termenboxPlatform', host);
    const { setKeepAwake } = await freshModule();

    setKeepAwake(true);
    setKeepAwake(true);
    expect(host.keepAwake).toHaveBeenCalledTimes(1);

    setKeepAwake(false);
    expect(host.allowSleep).toHaveBeenCalledTimes(1);
  });

  it('does nothing in the browser', async () => {
    const { setKeepAwake } = await freshModule();
    expect(() => setKeepAwake(true)).not.toThrow();
    expect(() => setKeepAwake(false)).not.toThrow();
  });

  it('survives a host without the capability or whose plugin rejects', async () => {
    vi.stubGlobal('__termenboxPlatform', { keepAwake: vi.fn().mockRejectedValue(new Error('no plugin')) });
    const { setKeepAwake } = await freshModule();
    expect(() => setKeepAwake(true)).not.toThrow();
    expect(() => setKeepAwake(false)).not.toThrow();
  });
});
