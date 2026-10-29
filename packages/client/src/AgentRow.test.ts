import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import AgentRow from './AgentRow.svelte';
import { TEMPLATES, getSupportedTemplates, visibleAgents } from './lib/newworkspace';
import type { HubServer } from './lib/hub';

const html = (props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(AgentRow, { props: { onselect: () => {}, ...props } }).body;
const tiles = (out: string) => [...out.matchAll(/data-testid="agent-tile"[\s\S]*?<span class="[^"]*">([^<]*)<\/span>/g)].map(m => m[1]);
const ids = (xs: { id: string }[]) => xs.map(x => x.id);

describe('AgentRow', () => {
  it('shows three tiles and a More tile with the count', () => {
    const out = html({ templates: TEMPLATES, recent: [], selected: 'claude' });
    expect(tiles(out)).toHaveLength(3);
    expect(out).toContain('data-testid="agents-more"');
    expect(out).toMatch(/agents-more[\s\S]*<small[^>]*>\s*(\d+)/);
    expect(out).toContain('grid-cols-2');
    expect(out).toContain('min-[480px]:grid-cols-4');
  });
  it('starts cold on claude, codex and shell', () => {
    expect(ids(visibleAgents(TEMPLATES, [], 'claude'))).toEqual(['claude', 'codex', 'shell']);
  });
  it('puts recent agents first', () => {
    expect(ids(visibleAgents(TEMPLATES, ['gemini', 'shell'], 'gemini'))).toEqual(['gemini', 'shell', 'claude']);
  });
  it('shows a pick from the layer as a selected tile, replacing the last one', () => {
    const v = visibleAgents(TEMPLATES, [], 'gemini');
    expect(ids(v)).toEqual(['claude', 'codex', 'gemini']);
    const out = html({ templates: TEMPLATES, recent: [], selected: 'gemini' });
    expect(out).toMatch(/aria-checked="true"[\s\S]*?Gemini/);
    expect(out.match(/aria-checked="true"/g)).toHaveLength(1);
  });
  it('lists every supported agent in the layer, recent ones marked, and the empty message', () => {
    const out = html({ templates: TEMPLATES, recent: ['gemini'], selected: 'claude', moreOpen: true });
    expect(out).toContain('data-testid="agents-layer"');
    expect(out).toContain('Choose agent');
    expect(out).toContain('Recent');
    for (const t of TEMPLATES) expect(out).toContain(t.label);
  });
  it('has no More tile when everything fits', () => {
    expect(html({ templates: TEMPLATES.slice(0, 3), recent: [], selected: 'claude' })).not.toContain('agents-more');
  });
  it('never offers an agent an acp-only server cannot start', () => {
    const server = { backends: ['acp'], acpAgents: ['claude'] } as unknown as HubServer;
    const supported = getSupportedTemplates(server);
    const out = html({ templates: supported, recent: ['codex', 'gemini'], selected: 'gemini' });
    for (const id of ids(supported)) expect(supported.map(t => t.id)).toContain(id);
    expect(tiles(out).join(' ')).not.toMatch(/Codex|Gemini/);
    expect(ids(visibleAgents(supported, ['codex'], 'codex'))).not.toContain('codex');
  });
});
