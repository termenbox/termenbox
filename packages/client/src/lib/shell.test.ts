import { describe, it, expect } from 'vitest';
import { get } from 'svelte/store';
import { parseRoute, routeHash, navigate, startRouter, destination, DESTINATIONS } from './router';
import { navGroups, HeldView, fleetOf } from './navlist';
import { buildEntries, searchEntries, matchShortcut, loadShortcutsEnabled, saveShortcutsEnabled, type KeyLike } from './palette';
import { edgeSwipeBack } from './edgeswipe';
import type { HubServer, HubSession } from './hub';
import type { AgentMap, SessionAgent } from './agents';

const server = (id: string, state: HubServer['state'] = 'connected'): HubServer => ({ id, server: { server_id: id, server_name: id, server_url: '', registered_at: 0 }, state, reason: '', version: '', backends: [], acpAgents: [], controlMode: false, tmuxVersion: '', prefix: '' });
const session = (serverId: string, id: string): HubSession => ({ id, name: id, agent: null, serverId, key: `${serverId}/${id}`, serverName: serverId } as HubSession);
const agent = (state: SessionAgent['state']): SessionAgent => ({ state, seq: 1, stale: false, ts: 0 });

describe('router', () => {
  it('reads only the four destinations from the hash', () => {
    for (const d of DESTINATIONS) expect(parseRoute(routeHash(d.id))).toBe(d.id);
    expect(parseRoute('#/sessions/')).toBe('sessions');
    for (const h of ['', '#', '#claim=abc', '#server=x', '#/nowhere', '#/home/extra']) expect(parseRoute(h)).toBeNull();
  });
  it('navigate moves the store and writes the hash once', () => {
    const calls: string[] = [];
    const loc = { hash: '', pathname: '/', search: '' };
    const win = { location: loc, history: { pushState: (_a: unknown, _b: string, url: string) => { calls.push(url); loc.hash = url.slice(url.indexOf('#')); } }, addEventListener() {}, removeEventListener() {} } as never;
    navigate('activity', win);
    navigate('activity', win);
    expect(get(destination)).toBe('activity');
    expect(calls).toEqual(['/#/activity']);
  });
  it('startRouter follows the address bar and ignores other hashes', () => {
    const listeners: Record<string, () => void> = {};
    const loc = { hash: '#/settings', pathname: '/', search: '' };
    const win = { location: loc, history: {}, addEventListener: (n: string, f: () => void) => { listeners[n] = f; }, removeEventListener() {} } as never;
    const stop = startRouter(win);
    expect(get(destination)).toBe('settings');
    loc.hash = '#claim=zzz';
    listeners.hashchange();
    expect(get(destination)).toBe('settings');
    loc.hash = '#/home';
    listeners.popstate();
    expect(get(destination)).toBe('home');
    stop();
  });
});

describe('session list for the sidebar', () => {
  const servers = [server('alpha'), server('beta')];
  const sessions = [session('alpha', 'b'), session('alpha', 'a'), session('beta', 'c')];
  it('groups by server, needs-you first, then by name', () => {
    const agents: AgentMap = { 'alpha/b': agent('needs_input'), 'alpha/a': agent('working') };
    const g = navGroups(servers, sessions, agents);
    expect(g.map((x) => x.server.id)).toEqual(['alpha', 'beta']);
    expect(g[0].sessions.map((s) => s.name)).toEqual(['b', 'a']);
    expect(g[0].sessions[0].state).toBe('needs-you');
    expect(g[1].sessions[0].state).toBe('idle');
  });
  it('maps agent states to fleet states', () => {
    expect(fleetOf(agent('error'))).toBe('needs-you');
    expect(fleetOf(agent('done'))).toBe('finished');
    expect(fleetOf(undefined)).toBe('idle');
  });
  it('holds order and membership while a pointer is down, updating state in place', () => {
    const v = new HeldView();
    const before = v.view(navGroups(servers, sessions, {}), 0);
    expect(before[0].sessions.map((s) => s.name)).toEqual(['a', 'b']);
    v.down(1);
    const changed = navGroups(servers, [...sessions, session('alpha', 'new')], { 'alpha/b': agent('needs_input') });
    const held = v.view(changed, 10);
    expect(held[0].sessions.map((s) => s.name)).toEqual(['a', 'b']); // no reorder, no insert
    expect(held[0].sessions[1].state).toBe('needs-you'); // fresh state, same place
    v.up(1);
    expect(v.view(changed, 20)[0].sessions.map((s) => s.name)).toEqual(['b', 'a', 'new']);
  });
  it('holds for 2 s after a scroll', () => {
    const v = new HeldView();
    v.view(navGroups(servers, sessions, {}), 0);
    v.scrolled(1000);
    const next = navGroups(servers, sessions, { 'alpha/b': agent('needs_input') });
    expect(v.view(next, 2500)[0].sessions.map((s) => s.name)).toEqual(['a', 'b']);
    expect(v.view(next, 3100)[0].sessions.map((s) => s.name)).toEqual(['b', 'a']);
  });
});

describe('palette search', () => {
  const many = [...Array.from({ length: 10 }, (_, i) => session('alpha', `svc-${i}`)), ...Array.from({ length: 10 }, (_, i) => session('beta', i === 7 ? 'needle' : `job-${i}`))];
  const entries = buildEntries({ servers: [server('alpha'), server('beta', 'paused')], sessions: many, agents: {}, wide: true, rightOpen: false, canCreate: true });
  it('lists destinations, actions, servers and sessions', () => {
    const ids = entries.map((e) => e.id);
    expect(ids).toContain('dest:home');
    expect(ids).toContain('action:new-session');
    expect(ids).toContain('action:pause:alpha');
    expect(ids).toContain('action:resume:beta');
    expect(ids).toContain('action:toggle-right');
    expect(ids).toContain('server:alpha');
    expect(ids.filter((i) => i.startsWith('session:'))).toHaveLength(20);
  });
  it('finds a session by name among 20 on two servers', () => {
    expect(searchEntries(entries, 'needle')[0].id).toBe('session:beta/needle');
    expect(searchEntries(entries, 'ndl')[0].id).toBe('session:beta/needle');
    expect(searchEntries(entries, 'zzzz')).toEqual([]);
  });
  it('matches the server named in the detail and ranks prefixes first', () => {
    expect(searchEntries(entries, 'beta job-3')[0].id).toBe('session:beta/job-3');
    expect(searchEntries(entries, 'pause')[0].label).toBe('Pause server alpha');
    expect(searchEntries(entries, 'se')[0].label.toLowerCase().startsWith('se')).toBe(true);
  });
  it('an empty query keeps the built order, capped', () => {
    expect(searchEntries(entries, '').map((e) => e.id)).toEqual(entries.slice(0, 50).map((e) => e.id));
  });
  it('hides what cannot be done', () => {
    const e = buildEntries({ servers: [], sessions: [], agents: {}, wide: false, rightOpen: false, canCreate: false });
    expect(e.some((x) => x.id === 'action:new-session' || x.id === 'action:toggle-right')).toBe(false);
  });
});

describe('shortcuts', () => {
  const key = (o: Partial<KeyLike>): KeyLike => ({ key: 'k', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...o });
  it('Ctrl+K on Windows/Linux, Cmd+K on macOS, nothing else', () => {
    expect(matchShortcut(key({ ctrlKey: true }), false)).toBe('palette');
    expect(matchShortcut(key({ ctrlKey: true, key: 'K' }), false)).toBe('palette');
    expect(matchShortcut(key({ metaKey: true }), true)).toBe('palette');
    expect(matchShortcut(key({ ctrlKey: true }), true)).toBeNull(); // Ctrl+K on a Mac is the terminal's
    expect(matchShortcut(key({ metaKey: true }), false)).toBeNull();
    for (const k of ['c', 'Tab', 'Escape', 'l']) expect(matchShortcut(key({ ctrlKey: true, key: k }), false)).toBeNull();
    expect(matchShortcut(key({}), false)).toBeNull();
  });
  it('ignores composition, AltGraph, and extra modifiers', () => {
    expect(matchShortcut(key({ ctrlKey: true, isComposing: true }), false)).toBeNull();
    expect(matchShortcut(key({ ctrlKey: true, keyCode: 229 }), false)).toBeNull();
    expect(matchShortcut(key({ ctrlKey: true, altKey: true, getModifierState: (k) => k === 'AltGraph' }), false)).toBeNull();
    expect(matchShortcut(key({ ctrlKey: true, shiftKey: true }), false)).toBeNull();
    expect(matchShortcut(key({ ctrlKey: true, altKey: true }), false)).toBeNull();
  });
  it('can be turned off and the choice is kept', () => {
    const mem = new Map<string, string>();
    const store = { getItem: (k: string) => mem.get(k) ?? null, setItem: (k: string, v: string) => void mem.set(k, v) };
    expect(loadShortcutsEnabled(store)).toBe(true);
    saveShortcutsEnabled(false, store);
    expect(loadShortcutsEnabled(store)).toBe(false);
    saveShortcutsEnabled(true, store);
    expect(loadShortcutsEnabled(store)).toBe(true);
  });
});

describe('edge swipe back', () => {
  it('needs a start at the left edge, a long horizontal move to the right', () => {
    expect(edgeSwipeBack({ x: 4, y: 300 }, { x: 120, y: 310 })).toBe(true);
    expect(edgeSwipeBack({ x: 60, y: 300 }, { x: 200, y: 300 })).toBe(false); // not from the edge
    expect(edgeSwipeBack({ x: 4, y: 300 }, { x: 40, y: 300 })).toBe(false); // too short
    expect(edgeSwipeBack({ x: 4, y: 300 }, { x: 120, y: 420 })).toBe(false); // mostly vertical
    expect(edgeSwipeBack({ x: 4, y: 300 }, { x: -50, y: 300 })).toBe(false); // wrong way
  });
});
