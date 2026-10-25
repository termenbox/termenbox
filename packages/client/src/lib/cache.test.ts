import { describe, expect, it } from 'vitest';
import { cacheRemaining, cacheText, expiringSoon } from './cache';
import type { WorkspaceAgent } from './agents';

const a = (patch: Partial<WorkspaceAgent>): WorkspaceAgent => ({ state: 'idle', seq: 1, stale: false, ts: 0, agent: 'claude', lastResponseAt: 0, ...patch });

describe('cache estimate', () => {
  it('counts down from the last answer of an idle agent and stops at zero', () => {
    expect(cacheRemaining(a({}), 120_000, 300_000)).toBe(180_000);
    expect(cacheRemaining(a({}), 400_000, 300_000)).toBe(0);
    expect(cacheRemaining(a({ state: 'done' }), 0, 300_000)).toBe(300_000);
  });
  it('has nothing to say for a working agent, a shell, or one that never answered', () => {
    expect(cacheRemaining(a({ state: 'working' }), 1, 300_000)).toBeNull();
    expect(cacheRemaining(a({ agent: undefined }), 1, 300_000)).toBeNull();
    expect(cacheRemaining(a({ lastResponseAt: undefined }), 1, 300_000)).toBeNull();
    expect(cacheRemaining(undefined, 1, 300_000)).toBeNull();
  });
  it('says "about" for an estimate only', () => {
    expect(cacheText(180_000)).toBe('about 3 min');
    expect(cacheText(180_000, true)).toBe('3 min');
    expect(cacheText(30_000)).toBe('about 30 s');
    expect(cacheText(0)).toBe('cache cold');
  });
  it('is expiring only in the last minute and not once cold', () => {
    expect(expiringSoon(59_000)).toBe(true);
    expect(expiringSoon(61_000)).toBe(false);
    expect(expiringSoon(0)).toBe(false);
    expect(expiringSoon(null)).toBe(false);
  });
});
