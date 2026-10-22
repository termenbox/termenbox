import { describe, it, expect } from 'vitest';
import { connectPaths, diagnose, DIAGNOSES, initialState, reasonOf, reduce, STEPS, type AddServerState, type Event } from './addServer';
import { buildPaths, type Path } from './paths';

const run = (events: Event[], from: AddServerState = initialState()) => events.reduce(reduce, from);
const closable = () => ({ close() {} });
const lan: Path = buildPaths({ bundle: { server_id: 'S'.repeat(43), seq: 1, expires: 9e9, published: false, fingerprint: 'sha-256 ' + Array(32).fill('AB').join(':'), name: 'b', candidates: [{ kind: 'lan', addr: '192.168.1.5', port: 4443 }] } })[0];
const wan: Path = { key: 'wan4 8.8.8.8:4443', kind: 'wan4', label: 'Direct, internet', addr: '8.8.8.8', port: 4443 };
const tunnel: Path = { key: 'tunnel wss://t.example/ws', kind: 'tunnel', label: 'Tunnel', addr: 'wss://t.example/ws', port: 443, url: 'wss://t.example/ws' };
const diag = (e: Event) => (e.type === 'failed' ? e.diagnosis : undefined);

describe('wizard transitions', () => {
  it('walks install, connect, confirm, done in order, one step at a time', () => {
    expect(STEPS).toEqual(['install', 'connect', 'confirm', 'done']);
    let s = initialState();
    expect(s.step).toBe('install');
    s = run([{ type: 'next' }], s);
    expect(s.step).toBe('connect');
    s = run([{ type: 'looking' }, { type: 'found' }, { type: 'reading' }], s);
    expect(s.connect.phase).toBe('reading');
    s = run([{ type: 'previewed' }], s);
    expect(s.step).toBe('confirm');
    s = run([{ type: 'confirmed' }], s);
    expect(s.step).toBe('done');
  });

  it('goes through the connect phases code, looking, found, reading', () => {
    let s = run([{ type: 'next' }]);
    expect(s.connect.phase).toBe('code');
    const phases: string[] = [];
    for (const e of [{ type: 'looking' }, { type: 'found' }, { type: 'reading' }] as Event[]) {
      s = reduce(s, e);
      phases.push(s.connect.phase);
    }
    expect(phases).toEqual(['looking', 'found', 'reading']);
  });

  it('ignores events that do not apply to the current step', () => {
    const s = initialState();
    expect(reduce(s, { type: 'previewed' })).toBe(s);
    expect(reduce(s, { type: 'confirmed' })).toBe(s);
    expect(reduce(s, { type: 'found' })).toBe(s);
    expect(reduce(run([{ type: 'next' }]), { type: 'confirmed' }).step).toBe('connect');
  });

  it('goes back one step and clears the checklist', () => {
    let s = run([{ type: 'next' }, { type: 'looking', addresses: [{ key: 'a', label: 'LAN', state: 'trying' }] }, { type: 'previewed' }]);
    expect(s.step).toBe('confirm');
    s = reduce(s, { type: 'back' });
    expect(s.step).toBe('connect');
    expect(s.connect.addresses).toEqual([]);
    expect(reduce(s, { type: 'back' }).step).toBe('install');
    expect(reduce(initialState(), { type: 'reset' })).toEqual(initialState());
  });

  it('tracks each address as trying, then failed with a reason or won', () => {
    let s = run([{ type: 'next' }, { type: 'looking', addresses: [{ key: 'a', label: 'LAN', state: 'trying' }, { key: 'b', label: 'Tunnel', state: 'trying' }] }]);
    s = run([{ type: 'address', row: { key: 'a', label: 'LAN', state: 'failed', reason: 'timeout' } }, { type: 'address', row: { key: 'b', label: 'Tunnel', state: 'won' } }], s);
    expect(s.connect.addresses.map((a) => [a.key, a.state, a.reason])).toEqual([['a', 'failed', 'timeout'], ['b', 'won', undefined]]);
    s = reduce(s, { type: 'address', row: { key: 'c', label: 'x', state: 'trying' } });
    expect(s.connect.addresses).toHaveLength(3);
  });

  it('a failure stays on Connect as a diagnosis, and looking again clears it', () => {
    let s = run([{ type: 'next' }, { type: 'failed', diagnosis: DIAGNOSES.link_expired }]);
    expect(s.step).toBe('connect');
    expect(s.connect.diagnosis?.cause).toBe('link_expired');
    s = reduce(s, { type: 'looking' });
    expect(s.connect.diagnosis).toBeUndefined();
  });

  it('keeps a working fallback as a notice, never a diagnosis', () => {
    const s = run([{ type: 'next' }, { type: 'fallback' }]);
    expect(s.connect.notice).toBe('ws_fallback');
    expect(s.connect.diagnosis).toBeUndefined();
  });
});

describe('failure causes', () => {
  const paths = (failures: { kind: Path['kind']; reason: 'timeout' | 'refused' | 'denied' | 'unreachable' | 'other' }[], extra = {}) => diagnose({ type: 'paths', failures, ...extra })?.cause;

  it('maps each signal to one cause', () => {
    expect(diagnose({ type: 'link', result: 'expired' })?.cause).toBe('link_expired');
    expect(diagnose({ type: 'link', result: 'invalid' })?.cause).toBe('bad_link');
    expect(diagnose({ type: 'code', result: 'wrong' })?.cause).toBe('wrong_code');
    expect(diagnose({ type: 'code', result: 'burned' })?.cause).toBe('code_burned');
    expect(diagnose({ type: 'silence' })?.cause).toBe('server_stopped');
    expect(paths([{ kind: 'lan', reason: 'timeout' }])).toBe('different_network');
    expect(paths([{ kind: 'lan', reason: 'timeout' }], { sameNetwork: true })).toBe('guest_isolation');
    expect(paths([{ kind: 'lan', reason: 'denied' }])).toBe('local_network_denied');
    expect(paths([{ kind: 'wan4', reason: 'timeout' }, { kind: 'lan', reason: 'timeout' }])).toBe('udp_closed');
    expect(paths([{ kind: 'tunnel', reason: 'refused' }])).toBe('server_stopped');
  });

  it('gives every cause one primary action, a one-line title and a detail', () => {
    const causes = Object.keys(DIAGNOSES);
    expect(causes).toEqual(['different_network', 'guest_isolation', 'local_network_denied', 'server_stopped', 'link_expired', 'bad_link', 'udp_closed', 'announced_unreachable', 'wrong_code', 'code_burned']);
    for (const d of Object.values(DIAGNOSES)) {
      expect(d.action.label.length).toBeGreaterThan(0);
      expect(d.title.length).toBeLessThan(80);
      expect(d.detail.length).toBeGreaterThan(20);
    }
  });

  it('a burned code and an expired link both say to run the installer again', () => {
    expect(DIAGNOSES.code_burned.action.id).toBe('rerun_installer');
    expect(DIAGNOSES.link_expired.action.id).toBe('rerun_installer');
  });

  it('a working WebSocket fallback is not a failure', () => {
    expect(diagnose({ type: 'paths', failures: [{ kind: 'wan4', reason: 'timeout' }], wsWorked: true })).toBeNull();
  });

  it('reads a reason from what an attempt threw', () => {
    expect(reasonOf(new DOMException('no', 'NotAllowedError'))).toBe('denied');
    expect(reasonOf(new Error('ICE timed out'))).toBe('timeout');
    expect(reasonOf(new Error('connection refused'))).toBe('refused');
    expect(reasonOf(new Error('boom'))).toBe('other');
  });
});

describe('connectPaths with fake openers', () => {
  const instant = { stagger: () => 0 };

  it('reports trying, then won, and wins on the first success', async () => {
    const events: Event[] = [];
    const r = await connectPaths([lan, wan], async (p) => (p.key === lan.key ? closable() : Promise.reject(new Error('timeout'))), (e) => events.push(e), instant);
    expect(r.won?.path.key).toBe(lan.key);
    expect(r.fallback).toBe(false);
    const looking = events.find((e) => e.type === 'looking');
    expect(looking && looking.type === 'looking' && looking.addresses?.map((a) => a.addr)).toEqual(['192.168.1.5:4443', '8.8.8.8:4443']);
    const s = events.reduce(reduce, run([{ type: 'next' }]));
    expect(s.connect.addresses.find((a) => a.key === lan.key)?.state).toBe('won');
  });

  it('records each failed address with its reason, then diagnoses a LAN-only miss', async () => {
    const events: Event[] = [];
    const r = await connectPaths([lan], () => Promise.reject(new Error('ICE timed out')), (e) => events.push(e), instant);
    expect(r.won).toBeUndefined();
    expect(r.diagnosis?.cause).toBe('different_network');
    const row = events.find((e) => e.type === 'address');
    expect(row && row.type === 'address' && [row.row.state, row.row.reason]).toEqual(['failed', 'timeout']);
    expect(events.map(diag).find(Boolean)?.cause).toBe('different_network');
  });

  it('a denied Local Network permission is named as such', async () => {
    const r = await connectPaths([lan], () => Promise.reject(new DOMException('blocked', 'NotAllowedError')), () => {}, instant);
    expect(r.diagnosis?.cause).toBe('local_network_denied');
  });

  it('a tunnel winning after the direct path failed is a notice, not an error', async () => {
    const events: Event[] = [];
    const r = await connectPaths([wan, tunnel], async (p) => (p.kind === 'tunnel' ? closable() : Promise.reject(new Error('timed out'))), (e) => events.push(e), instant);
    expect(r.won?.path.kind).toBe('tunnel');
    expect(r.fallback).toBe(true);
    expect(events.some((e) => e.type === 'fallback')).toBe(true);
    expect(events.some((e) => e.type === 'failed')).toBe(false);
  });

  it('closed UDP with every path failing is udp_closed', async () => {
    const r = await connectPaths([wan], () => Promise.reject(new Error('timed out')), () => {}, instant);
    expect(r.diagnosis?.cause).toBe('udp_closed');
  });
});

describe('module', () => {
  it('has no DOM imports', async () => {
    const src = await import('node:fs').then((fs) => fs.readFileSync('src/lib/addServer.ts', 'utf8'));
    expect(src).not.toMatch(/\b(document|window|navigator|localStorage)\b/);
    expect(src).not.toMatch(/from 'svelte/);
  });
});
