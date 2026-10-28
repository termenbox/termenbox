import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import ServerDiagnosticsDialog from '../ServerDiagnosticsDialog.svelte';
import { BundleReader, SeenExits, bundleFileName, exitText, incidentPlan, trimForUrl } from './diagnostics';
import { MAX_URL_TEXT } from './report';

const CERT = 'eyJ2IjoxLCJraWQiOiJrMSIsInN1YiI6NDIsImxvZ2luIjoib2N0byJ9.mGt9Nv_YUuWy4UuPrwU4roLVMdX5b0l8f92FM0oHyk8aa';

const log = (n: number) => Array.from({ length: n }, (_, i) => `{"ts":"2026-10-28T11:51:${String(i % 60).padStart(2, '0')}.000Z","level":"info","fields":{"event":"tick","n":${i}}}`).join('\n');
const bundle = (n: number) => `termenbox server diagnostics\nversion: 0.1.57\nchannel: stable\n\nprevious exit:\n{"type":"previous_exit","at_ms":1,"streak":2,"crashed":true}\n\nlog (newest last):\n${log(n)}\n`;

describe('BundleReader', () => {
  it('returns the text only when the last part is in, in order', () => {
    const r = new BundleReader();
    expect(r.add(0, 3, 'a\n')).toBeNull();
    expect(r.add(1, 3, 'b\n')).toBeNull();
    expect(r.add(2, 3, 'c\n')).toBe('a\nb\nc\n');
  });
  it('drops a answer with a gap and starts over at part 0', () => {
    const r = new BundleReader();
    r.add(0, 3, 'a');
    expect(r.add(2, 3, 'c')).toBeNull();
    expect(r.add(0, 1, 'fresh')).toBe('fresh');
  });
  it('keeps what the server sent, source locations and times included', () => {
    const text = 'panic at packages/server/src/acp.rs:12 11:51:09';
    expect(new BundleReader().add(0, 1, text)).toBe(text);
  });
});

describe('SeenExits', () => {
  it('announces an unclean exit once per at_ms and again for a newer one', () => {
    const seen = new SeenExits();
    expect(seen.announce('s1', 100)).toBe(true);
    expect(seen.announce('s1', 100)).toBe(false);
    expect(seen.announce('s2', 100)).toBe(true);
    expect(seen.announce('s1', 200)).toBe(true);
  });
  it('remembers across page loads, and works with storage that throws', () => {
    const data: Record<string, string> = {};
    const storage = { getItem: (k: string) => data[k] ?? null, setItem: (k: string, v: string) => { data[k] = v; } };
    expect(new SeenExits(storage).announce('s1', 100)).toBe(true);
    expect(new SeenExits(storage).announce('s1', 100)).toBe(false);
    const broken = { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } };
    const seen = new SeenExits(broken);
    expect(seen.announce('s1', 100)).toBe(true);
    expect(seen.announce('s1', 100)).toBe(false);
  });
  it('says what happened in one line', () => {
    const line = exitText({ serverId: 's', atMs: 0, streak: 3, crashed: true }, 'box', 5 * 60000);
    expect(line).toBe('box crashed (last seen 5 min ago, 3 times in a row).');
  });
});

describe('the issue address', () => {
  it('uses the incident template and prefills the version', () => {
    const plan = incidentPlan(bundle(3), 'I was running a task');
    const q = new URL(plan.url).searchParams;
    expect(q.get('template')).toBe('incident.yml');
    expect(q.get('version')).toBe('0.1.57');
    expect(q.get('what')).toBe('I was running a task');
    expect(plan.trimmed).toBe(false);
    expect(q.get('diagnostics')).toBe(bundle(3));
  });
  it('trims a long bundle to the limit report.test.ts asserts, keeping the head and the newest log lines', () => {
    const long = bundle(5000);
    const plan = incidentPlan(long);
    const diag = new URL(plan.url).searchParams.get('diagnostics') ?? '';
    expect(plan.trimmed).toBe(true);
    expect(encodeURIComponent(diag).length).toBeLessThanOrEqual(MAX_URL_TEXT);
    expect(diag).toContain('version: 0.1.57');
    expect(diag).toContain('"n":4999');
    expect(diag).not.toContain('"n":0}');
    expect(plan.copy).toBeUndefined();
  });
  it('still carries the newest log lines when a crash file made the head long', () => {
    const crash = `{"backtrace":"${'frame '.repeat(2000)}"}`;
    const long = bundle(300).replace('previous exit:', `newest crash file:\n${crash}\n\nprevious exit:`);
    const t = trimForUrl(long);
    expect(t.text).toContain('log (newest last):');
    expect(t.text).toContain('"n":299');
    expect(encodeURIComponent(t.text).length).toBeLessThanOrEqual(MAX_URL_TEXT);
  });
  it('does not throw on non-ASCII text cut mid-character, and bounds what the user wrote', () => {
    const emoji = bundle(5).replace('channel: stable', '😀'.repeat(5000));
    expect(() => trimForUrl(emoji)).not.toThrow();
    const plan = incidentPlan(bundle(5000), 'ы'.repeat(1500));
    expect(plan.url.length - 'https://github.com/termenbox/termenbox/issues/new?'.length).toBeLessThan(MAX_URL_TEXT + 1600 + 200);
    expect(new URL(plan.url).searchParams.get('what')?.length).toBeGreaterThan(0);
  });
  it('trims even when the head alone is too long', () => {
    const t = trimForUrl('x'.repeat(MAX_URL_TEXT * 2));
    expect(t.trimmed).toBe(true);
    expect(encodeURIComponent(t.text).length).toBeLessThanOrEqual(MAX_URL_TEXT);
  });
  it('names the download file safely', () => {
    expect(bundleFileName(new Date('2026-10-28T11:51:09.638Z'))).toBe('termenbox-diagnostics-2026-10-28T11-51-09-638Z.txt');
  });
});

describe('ServerDiagnosticsDialog', () => {
  const html = (props: { text: string | null; error?: string }) => (render as unknown as (c: unknown, o: { props: unknown }) => { body: string })(ServerDiagnosticsDialog, { props: { ...props, onclose: () => {} } }).body;

  it('offers no download or issue before the bundle has arrived', () => {
    const waiting = html({ text: null });
    expect(waiting).toContain('Asking the server');
    expect(waiting).not.toMatch(/Download|Open GitHub issue/);
    const failed = html({ text: null, error: 'Only the owner can read this server\'s diagnostics' });
    expect(failed).toContain('Only the owner');
    expect(failed).not.toMatch(/Download|Open GitHub issue/);
  });
  it('shows the bundle in full, with the buttons, once it has', () => {
    const out = html({ text: bundle(2) });
    expect(out).toContain('termenbox server diagnostics');
    expect(out).toContain('Download');
    expect(out).toContain('Open GitHub issue');
    expect(out.indexOf('termenbox server diagnostics')).toBeLessThan(out.indexOf('Open GitHub issue'));
  });
});
