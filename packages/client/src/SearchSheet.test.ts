import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import SearchSheet from './SearchSheet.svelte';
import { IDLE, NOTICE, cutText, type Hit, type SearchState } from './lib/sessionSearch';

const html = (props: Record<string, unknown>) =>
  (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(SearchSheet, { props: { open: true, onquery: () => {}, onstop: () => {}, onpick: () => {}, onresume: () => {}, ...props } }).body;

const hit = (over: Partial<Hit> = {}): Hit => ({ serverId: 'a', serverName: 'Box', workspace: 'w1', tab: 'build', agent: 'claude', up: true, turn: 'u1', kind: 'user', snippet: 'fix the parser bug', highlight: [8, 14], at: Date.parse('2026-10-30T10:00:00Z'), ...over });
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
});
