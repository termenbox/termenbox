import { describe, expect, it } from 'vitest';
import { fromForm, mutable, toForm, toggle } from './pushSettings';
import { parseFocusLink } from './router';
import type { PushSettings } from './protocol';

const base: PushSettings = { initial_wait_secs: 600, max_interval_secs: 3600, multiplier: 2, utc_offset_minutes: 0, muted_agents: [] };

describe('push settings form', () => {
  it('shows minutes and sends seconds, with the browser offset so quiet hours are local', () => {
    const f = toForm(base);
    expect(f).toMatchObject({ waitMin: 10, maxMin: 60, multiplier: 2, quiet: '' });
    expect(fromForm({ ...f, quiet: ' 22:00-07:00 ' }, 120)).toEqual({ ...base, quiet_hours: '22:00-07:00', utc_offset_minutes: 120 });
  });
  it('leaves quiet hours out when empty, and pulls numbers into the server limits', () => {
    const s = fromForm({ waitMin: 0, maxMin: 0, multiplier: 99, quiet: '  ', muted: [] }, 0);
    expect(s.quiet_hours).toBeUndefined();
    expect(s).toMatchObject({ initial_wait_secs: 60, max_interval_secs: 60, multiplier: 10 });
    expect(fromForm({ waitMin: Number.NaN, maxMin: 5, multiplier: Number.NaN, quiet: '', muted: [] }, 0)).toMatchObject({ initial_wait_secs: 600, max_interval_secs: 600, multiplier: 2 });
  });
  it('mutes and unmutes by agent, and keeps an already muted agent on offer', () => {
    expect(toggle([], 'claude')).toEqual(['claude']);
    expect(toggle(['claude', 'codex'], 'claude')).toEqual(['codex']);
    expect(mutable(['claude'], ['codex', 'claude'])).toEqual(['claude', 'codex']);
  });
});

describe('push deep link', () => {
  it('reads the workspace key out of #/home?focus=, decoded', () => {
    expect(parseFocusLink('#/home?focus=one%2Ftmux%3Awork')).toBe('one/tmux:work');
    expect(parseFocusLink('#/home?focus=a%2Db')).toBe('a-b');
  });
  it('ignores every other hash, a broken escape and an oversized key', () => {
    for (const h of ['', '#/home', '#/workspaces?focus=x', '#/home?focus=', '#/home?focus=%E0%A4%A', `#/home?focus=${'a'.repeat(300)}`, '#claim=abc']) expect(parseFocusLink(h)).toBeNull();
  });
});
