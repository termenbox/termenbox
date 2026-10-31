import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import ServerUpdatesDialog from './ServerUpdatesDialog.svelte';
import type { HubServer } from './lib/hub';

const sec = Math.floor(Date.now() / 1000);
const status = (over: Partial<HubServer>): HubServer => ({ id: 'a', server: { server_id: 'a', server_url: '', server_name: 'alpha', registered_at: 0 }, state: 'connected', reason: '', version: '0.1.57+abc', updateAvailable: '', updating: false, backends: ['tmux'], acpAgents: [], controlMode: true, tmuxVersion: '3.8', prefix: 'C-b', updateCapabilities: ['check', 'postpone', 'skip'], ...over });
const draw = (over: Partial<HubServer>) => render(ServerUpdatesDialog, { props: { status: status(over), name: 'alpha', onclose() {}, oncheck() {}, onupdate() {}, onpostpone() {}, onskip() {} } }).body;
const has = (out: string, id: string) => out.includes(`data-testid="${id}"`);

describe('the Updates card', () => {
  it('up to date: offers only Check now', () => {
    const out = draw({ update: { phase: 'idle', checked_at: sec - 3600, checked_ok_at: sec - 3600 } });
    expect(out).toContain('Up to date.');
    expect(out).toContain('Last checked 1 h ago');
    expect(out).toContain('Running v0.1.57');
    expect([has(out, 'updates-check'), has(out, 'updates-now'), has(out, 'updates-skip'), has(out, 'updates-postpone')]).toEqual([true, false, false, false]);
  });
  it('available: offers Update now and Skip, and Postpone only when the server allows it', () => {
    const base = { updateAvailable: '0.1.58', update: { phase: 'available', available: '0.1.58', checked_ok_at: sec } };
    const out = draw(base);
    expect(out).toContain('v0.1.58 is available.');
    expect([has(out, 'updates-now'), has(out, 'updates-skip'), has(out, 'updates-postpone')]).toEqual([true, true, false]);
  });
  afterEach(() => vi.useRealTimers());
  it('scheduled: names the deadline and the choices, with Postpone while allowed', () => {
    // A deadline on another day reads "Sun 00:16", so the test must not depend on the hour it runs at.
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date(2026, 9, 31, 12, 0, 0));
    const now = Math.floor(Date.now() / 1000);
    const out = draw({ updateAvailable: '0.1.58', update: { phase: 'scheduled', available: '0.1.58', deadline: now + 7200, can_postpone: true, checked_ok_at: now } });
    expect(out).toMatch(/v0\.1\.58 installs at \d\d:\d\d unless you postpone or skip\./);
    expect(has(out, 'updates-postpone')).toBe(true);
  });
  it('installing: nothing can be started again', () => {
    const out = draw({ updating: true, updateAvailable: '0.1.58', update: { phase: 'installing', available: '0.1.58' } });
    expect(out).toContain('Installing…');
    expect(out).toMatch(/data-testid="updates-now"[^>]*disabled|disabled[^>]*data-testid="updates-now"/);
    expect(has(out, 'updates-skip')).toBe(false);
  });
  it('rolled back: says so and still lets the owner try again', () => {
    const out = draw({ update: { phase: 'rolled-back' } });
    expect(out).toContain('rolled back');
  });
  it('a failed check keeps the notice and shows why; a stale one warns', () => {
    const out = draw({ updateAvailable: '0.1.58', update: { phase: 'available', available: '0.1.58', checked_at: sec, checked_ok_at: sec - 80 * 3600, error: 'offline' } });
    expect(out).toContain('v0.1.58 is available.');
    expect(out).toContain('The last check failed: offline');
    expect(has(out, 'updates-stale')).toBe(true);
  });
  it('a check in flight says so and cannot be asked twice', () => {
    const out = draw({ checking: true, update: { phase: 'idle', checked_ok_at: sec } });
    expect(out).toContain('Checking…');
    expect(out).toMatch(/disabled[^>]*data-testid="updates-check"|data-testid="updates-check"[^>]*disabled/);
  });
  it('an older server shows no button its helper would reject', () => {
    const out = draw({ updateCapabilities: undefined, updateAvailable: '0.1.58' });
    expect([has(out, 'updates-check'), has(out, 'updates-skip'), has(out, 'updates-postpone'), has(out, 'updates-now')]).toEqual([false, false, false, true]);
    expect(out).toContain('Not checked yet');
  });
  it('a server that is not connected has nothing to show', () => {
    const out = draw({ state: 'unreachable' });
    expect(out).toContain('Connect to this server');
    expect(has(out, 'updates-now')).toBe(false);
  });
});
