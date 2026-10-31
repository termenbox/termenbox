import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import UpdateBanner from './UpdateBanner.svelte';
import type { HubServer } from './lib/hub';

// A fixed midday, so that a deadline an hour later is on the same day whenever the suite runs (the text drops the weekday only then).
const NOW = new Date(2026, 0, 15, 12, 0).getTime();
const sec = Math.floor(NOW / 1000);
const server = (id: string, over: Partial<HubServer> = {}): HubServer => ({ id, server: { server_id: id, server_url: '', server_name: id, registered_at: 0 }, state: 'connected', reason: '', version: '0.1.57', updateAvailable: '', updating: false, backends: [], acpAgents: [], controlMode: true, tmuxVersion: '3.8', prefix: 'C-b', updateCapabilities: ['check', 'postpone', 'skip'], ...over });
const draw = (servers: HubServer[]) => render(UpdateBanner, { props: { servers, now: NOW, onupdate() {}, onpostpone() {}, onskip() {}, onopen() {} } }).body;

describe('the update banner', () => {
  it('is absent when nothing is on offer, the server is away, or it is already installing', () => {
    expect(draw([server('a')])).not.toContain('update-banner');
    expect(draw([server('a', { updateAvailable: '0.1.58', state: 'unreachable' })])).not.toContain('update-banner');
    expect(draw([server('a', { updateAvailable: '0.1.58', updating: true })])).not.toContain('update-banner');
  });
  it('names the server, the release and the countdown, and offers Update now, Postpone and Skip', () => {
    const out = draw([server('a'), server('b', { updateAvailable: '0.1.58', update: { phase: 'scheduled', available: '0.1.58', deadline: sec + 3600, can_postpone: true } })]);
    expect(out).toContain('data-testid="update-banner"');
    expect(out).toContain('<strong>b</strong>');
    expect(out).toMatch(/installs at \d\d:\d\d unless you postpone or skip/);
    expect(out).toContain('Update now');
    expect(out).toContain('Postpone');
    expect(out).toContain('Skip');
  });
  it('offers only Update now to a server whose helper takes no other action, and floats over the page', () => {
    const out = draw([server('a', { updateAvailable: '0.1.58', updateCapabilities: [] })]);
    expect(out).toContain('Update now');
    expect(out).not.toContain('Postpone');
    expect(out).not.toMatch(/>Skip</);
    expect(out).toContain('v0.1.58 is available.');
  });
});
