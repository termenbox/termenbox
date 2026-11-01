import { describe, it, expect, vi } from 'vitest';
import { render } from 'svelte/server';
import SearchSheet from './SearchSheet.svelte';
import { IDLE, NOTICE, cutText, type Hit, type SearchState } from './lib/sessionSearch';

const html = (props: Record<string, unknown>) =>
  (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(SearchSheet, { props: { open: true, onquery: () => {}, onstop: () => {}, onpick: () => {}, onresume: () => {}, ...props } }).body;

const hit = (over: Partial<Hit> = {}): Hit => ({ serverId: 'a', serverName: 'Box', workspace: 'w1', tab: 'build', agent: 'claude', up: true, turn: 'u1', kind: 'user', snippet: 'fix the parser bug', highlight: [8, 14], dir: '', session: '', at: Date.parse('2026-10-30T10:00:00Z'), ...over });
const st = (over: Partial<SearchState>): SearchState => ({ ...IDLE, ...over });

describe('SearchSheet', () => {
  it('renders nothing closed', () => {
    expect(html({ open: false, search: IDLE })).not.toContain('session-search');
  });
  it('shows the server notice, an input and the first-use hint', () => {
    const out = html({ search: IDLE });
    expect(out).toContain('data-testid="search-input"');
    expect(out).toContain(NOTICE.replace('’', '’'));
    expect(out).toContain('Type to search');
  });
  it('shows streaming hits grouped by workspace with the match highlighted and a Stop while searching', () => {
    const out = html({ search: st({ query: 'parser', status: 'searching', pending: ['a'], hits: [hit(), hit({ turn: 'u2' }), hit({ workspace: 'w2', turn: 'u3', up: false })] }), nameOf: (_s: string, w: string) => (w === 'w1' ? 'API' : 'Docs') });
    expect(out).toContain('Searching');
    expect(out).toContain('data-testid="search-stop"');
    expect(out.split('data-testid="search-group"').length - 1).toBe(2);
    expect(out.split('data-testid="search-hit"').length - 1).toBe(3);
    expect(out).toContain('<mark');
    expect(out).toMatch(/<mark[^>]*>parser<\/mark>/);
    expect(out).toContain('>API<');
    expect(out).toContain('Resume');
    expect(out).toContain('Open');
  });
  it('offers the three groupings, workspace by default, and names the workspace on rows when there is no workspace header', () => {
    const out = html({ search: st({ query: 'x', status: 'done', hits: [hit()] }) });
    expect(out).toContain('data-testid="search-group-agent"');
    expect(out).toContain('data-testid="search-group-workspace"');
    expect(out).toContain('data-testid="search-group-none"');
    expect(out).toMatch(/data-testid="search-group-workspace"[^>]*aria-pressed="true"|aria-pressed="true"[^>]*data-testid="search-group-workspace"/);
    expect(out).not.toContain('data-testid="search-where"');
  });
  it('says a search was cut, and why an agent is off', () => {
    const out = html({ search: st({ query: 'x', status: 'done', hits: [hit()], cuts: [cutText('time')!], off: [{ agent: 'codex', reason: 'unknown file version' }] }) });
    expect(out).toContain('Search was cut');
    expect(out).toContain('Search is off for codex: unknown file version');
    expect(out).not.toContain('data-testid="search-stop"');
  });
  it('shows an empty state when nothing matched, and escapes what the server sent', () => {
    const out = html({ search: st({ query: '<b>x</b>', status: 'done' }) });
    expect(out).toContain('No conversation matches');
    expect(out).not.toContain('<b>x</b>');
  });
  it('escapes a snippet', () => {
    const out = html({ search: st({ query: 'x', status: 'done', hits: [hit({ snippet: '<img src=x onerror=alert(1)>', highlight: [0, 4] })] }) });
    expect(out).not.toContain('<img');
  });
  it('says a server went away', () => {
    expect(html({ search: st({ query: 'x', status: 'done', lost: 1 }) })).toContain('went away');
  });
  it('offers the time chips, 30 days by default, and Search older when conversations were skipped', () => {
    const out = html({ search: st({ query: 'x', status: 'done', hits: [hit()], older: 3 }) });
    for (const id of ['24h', '7d', '30d', 'any']) expect(out).toContain(`data-testid="search-range-${id}"`);
    expect(out).toMatch(/aria-pressed="true"[^>]*data-testid="search-range-30d"|data-testid="search-range-30d"[^>]*aria-pressed="true"/);
    expect(out).toContain('3 older conversations not searched.');
    expect(out).toContain('data-testid="search-older-go"');
    expect(html({ search: st({ query: 'x', status: 'done', older: 3, range: 'any' }) })).not.toContain('search-older-go');
    expect(html({ search: st({ query: 'x', status: 'done' }) })).not.toContain('search-older"');
  });
  it('groups a conversation no workspace holds under its directory, with the full path as a tooltip, and badges it Resume', () => {
    const out = html({ search: st({ query: 'x', status: 'done', hits: [hit({ workspace: '', tab: '', up: false, dir: '/home/me/code/api', session: 's1' })] }) });
    expect(out).toContain('>code/api<');
    expect(out).toContain('title="/home/me/code/api"');
    expect(out).toContain('Resume');
    expect(out).not.toContain('>Open<');
  });
  it('draws a first piece, offers the rest, and the per-conversation expansion once a conversation shows three hits', () => {
    const many = Array.from({ length: 40 }, (_, i) => hit({ turn: `u${i}`, session: i < 3 ? 's1' : `s${i}` }));
    const out = html({ search: st({ query: 'x', status: 'done', hits: many }) });
    expect(out.split('data-testid="search-hit"').length - 1).toBe(30);
    expect(out).toContain('data-testid="search-more"');
    expect(out.split('data-testid="search-more-session"').length - 1).toBe(1);
    const expanded = html({ search: st({ query: 'x', status: 'done', hits: many.slice(0, 3), expanded: ['a/s1'] }) });
    expect(expanded).not.toContain('search-more-session');
    expect(expanded).not.toContain('data-testid="search-more"');
  });
  it('offers more past the cap when the server gave a cursor, and not while searching', () => {
    const base = { query: 'x', hits: [hit()], next: { a: 'c' } };
    expect(html({ search: st({ ...base, status: 'done' }) })).toContain('data-testid="search-more"');
    expect(html({ search: st({ ...base, status: 'searching', pending: ['a'] }) })).not.toContain('data-testid="search-more"');
  });
  it('labels the flat list and gives a row its full directory as a tooltip', () => {
    const store = { getItem: () => 'none', setItem: () => {} };
    vi.stubGlobal('localStorage', store);
    try {
      const out = html({ search: st({ query: 'x', status: 'done', hits: [hit({ workspace: '', dir: '/home/me/code/api' })] }) });
      expect(out).toContain('aria-label="Matches"');
      expect(out).toMatch(/data-testid="search-where" title="\/home\/me\/code\/api"/);
    } finally { vi.unstubAllGlobals(); }
  });
});
