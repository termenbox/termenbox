import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import { createRawSnippet } from 'svelte';
import { stripTarget } from './WorkspaceStrip.svelte';
import { StatusChip, AgentBadge, FleetTile, WorkspaceStrip, DecisionCard, OptionButton, QueueItem, Composer, MoreSheet, ChartFrame, EmptyState, STATES } from './index';

const text = (t: string) => createRawSnippet(() => ({ render: () => `<span>${t}</span>` }));
const html = (c: any, props: Record<string, unknown>) => render(c, { props }).body;

describe('design system components render', () => {
  it('status chip says the state in words and a glyph, whatever the colour', () => {
    for (const [state, s] of Object.entries(STATES)) {
      const out = html(StatusChip, { state });
      expect(out).toContain(s.label);
      expect(out).toContain(s.glyph);
    }
  });
  it('agent badge names the state for assistive tech', () => {
    expect(html(AgentBadge, { agent: 'claude', state: 'failed' })).toContain('aria-label="Failed"');
  });
  it('fleet tile is a button only when it filters', () => {
    expect(html(FleetTile, { state: 'working', count: 3 })).not.toContain('<button');
    const out = html(FleetTile, { state: 'working', count: 3, onclick: () => {} });
    expect(out).toContain('<button');
    expect(out).toContain('3');
    expect(out).toContain('Working');
  });
  it('workspace strip marks the open workspace and offers a new one', () => {
    const items = [{ id: 'a', name: 'api', agent: 'claude', state: 'needs-you' as const }, { id: 'b', name: 'web', state: 'idle' as const }];
    const out = html(WorkspaceStrip, { items, selected: 'a', onadd: () => {} });
    expect(out).toContain('aria-selected="true"');
    expect(out).toContain('api');
    expect(out).not.toContain('>web<');
    expect(out).toContain('aria-label="New workspace"');
  });
  it('decision card shows who asks, the wait, the question and its options', () => {
    const out = html(DecisionCard, { workspace: 'api', waited: '4 min', context: 'Edit src/a.ts', question: 'Allow the edit?', children: text('options') });
    for (const t of ['api', 'Waiting 4 min', 'Edit src/a.ts', 'Allow the edit?', 'options']) expect(out).toContain(t);
  });
  it('option button shows its number and the primary look', () => {
    const out = html(OptionButton, { hint: '1', label: 'Yes', detail: 'Once', primary: true });
    expect(out).toContain('bg-warn');
    expect(out).toContain('Yes');
    expect(out).toContain('Once');
    expect(html(OptionButton, { label: 'No' })).toContain('bg-raised');
  });
  it('queue item carries the name, the line and the state word', () => {
    const out = html(QueueItem, { name: 'api', summary: 'Waiting for approval', state: 'needs-you' });
    expect(out).toContain('Waiting for approval');
    expect(out).toContain('Needs you');
  });
  it('composer line is 16 px so iOS does not zoom, and its buttons are named', () => {
    const out = html(Composer, { onmic: () => {} });
    expect(out).toContain('text-input');
    expect(out).toContain('aria-label="Dictate"');
    expect(out).toContain('aria-label="Enter"');
    expect(html(Composer, { value: 'ls' })).toContain('aria-label="Send"');
  });
  it('more sheet renders nothing while closed', () => {
    expect(html(MoreSheet, { open: false, groups: [{ title: 'Keys', items: [{ label: 'Esc' }] }] })).not.toContain('Esc');
  });
  it('chart frame gives the chart a title and a caption in words', () => {
    const out = html(ChartFrame, { title: 'Cost', caption: '12 workspaces this week', children: text('chart') });
    for (const t of ['Cost', '12 workspaces this week', 'chart']) expect(out).toContain(t);
  });
  it('empty state says what to do next', () => {
    const out = html(EmptyState, { title: 'No workspaces', body: 'Start one to see it here.', children: text('New workspace') });
    for (const t of ['No workspaces', 'Start one', 'New workspace']) expect(out).toContain(t);
  });
});

describe('cn keeps the custom type scale', () => {
  it('does not treat a size as a colour', async () => {
    const { cn } = await import('../ui/utils');
    expect(cn('text-title', 'text-warn')).toBe('text-title text-warn');
    expect(cn('text-sm', 'text-base')).toBe('text-base');
  });
});

describe('workspace strip keys', () => {
  it('moves with arrows (wrapping), Home and End; ignores other keys', () => {
    expect(stripTarget('ArrowRight', 2, 3)).toBe(0);
    expect(stripTarget('ArrowLeft', 0, 3)).toBe(2);
    expect(stripTarget('Home', 2, 3)).toBe(0);
    expect(stripTarget('End', 0, 3)).toBe(2);
    for (const k of ['a', 'Tab', 'Escape', 'ArrowUp']) expect(stripTarget(k, 1, 3)).toBeNull();
    expect(stripTarget('ArrowRight', 0, 0)).toBeNull();
  });
  it('has one tab stop and names every tab with its state', () => {
    const items = [{ id: 'a', name: 'api', state: 'needs-you' as const }, { id: 'b', name: 'web', state: 'idle' as const }];
    const out = html(WorkspaceStrip, { items, selected: 'b' });
    expect(out.match(/tabindex="0"/g)).toHaveLength(1);
    expect(out).toContain('aria-label="api, Needs you"');
  });
});
