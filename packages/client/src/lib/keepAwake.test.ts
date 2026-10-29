import { afterEach, describe, expect, it, vi } from 'vitest';

async function freshModule() {
  vi.resetModules();
  return import('./keepAwake');
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('setKeepAwake', () => {
  it('takes a screen wake lock in the browser and releases it', async () => {
    const release = vi.fn().mockResolvedValue(undefined);
    const request = vi.fn().mockResolvedValue({ release });
    vi.stubGlobal('navigator', { wakeLock: { request } });
    const { setKeepAwake } = await freshModule();

    setKeepAwake(true);
    await vi.waitFor(() => expect(request).toHaveBeenCalledWith('screen'));

    setKeepAwake(false);
    await vi.waitFor(() => expect(release).toHaveBeenCalled());
  });

  it('uses the wrapper host and ignores repeated calls with the same value', async () => {
    const host = {
      keepAwake: vi.fn().mockResolvedValue(undefined),
      allowSleep: vi.fn().mockResolvedValue(undefined),
      onBack: vi.fn(),
    };
    vi.stubGlobal('__termenboxPlatform', host);
    vi.stubGlobal('navigator', {});
    const { setKeepAwake } = await freshModule();

    setKeepAwake(true);
    await vi.waitFor(() => expect(host.keepAwake).toHaveBeenCalledTimes(1));

    setKeepAwake(true);
    await Promise.resolve();
    expect(host.keepAwake).toHaveBeenCalledTimes(1);

    setKeepAwake(false);
    await vi.waitFor(() => expect(host.allowSleep).toHaveBeenCalledTimes(1));
  });

  it('releases a lock that arrives after the screen may sleep again', async () => {
    let resolveRequest: (value: { release: () => Promise<void> }) => void = () => {};
    const release = vi.fn().mockResolvedValue(undefined);
    const request = vi.fn(() => new Promise<{ release: () => Promise<void> }>((resolve) => { resolveRequest = resolve; }));
    vi.stubGlobal('navigator', { wakeLock: { request } });
    const { setKeepAwake } = await freshModule();

    setKeepAwake(true);
    setKeepAwake(false);
    resolveRequest({ release });

    await vi.waitFor(() => expect(release).toHaveBeenCalled());
  });

  it('does nothing when the browser has no wake lock and a wrapper is absent', async () => {
    vi.stubGlobal('navigator', {});
    const { setKeepAwake } = await freshModule();
    expect(() => setKeepAwake(true)).not.toThrow();
    expect(() => setKeepAwake(false)).not.toThrow();
  });
});
