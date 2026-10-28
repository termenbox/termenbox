// Conformance on the client side (step 2.3, guardrail protocol-core-stays-acp-v1): the server adapter turns each standard
// ACP `session/update` kind into `acp_view` entries; the vector in the server's testdata holds what it makes of the 14
// kinds (the server test keeps it true). The current reducer and entry view must take every one, and an unknown entry
// kind, extension or content type, without throwing. Step 5.1 moves this onto the protocol reducer.
import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import vector from '../../../server/testdata/acp-wire/standard-kinds.json';
import AcpEntryView from '../AcpEntryView.svelte';
import { applyView, emptyStream, type StreamState } from './acpStream';
import type { AcpViewMessage } from './protocol';

const STANDARD = [
  'user_message_chunk', 'agent_message_chunk', 'agent_thought_chunk', 'tool_call', 'tool_call_update', 'plan', 'available_commands_update',
  'current_mode_update', 'config_option_update', 'session_info_update', 'usage_update', 'notice', 'compaction_update', 'compaction_summary_chunk',
];

const html = (props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(AcpEntryView, { props }).body;
const base = { opened: new Set<string>(), ontoggle: () => {}, onanswer: () => {}, workspaceKey: 'srv/acp:work', name: 'work', agent: 'claude', now: 0 };
const asView = (v: unknown) => v as AcpViewMessage;
/** Reduces `view` and draws every entry; returns the state and the drawn HTML. */
const show = (view: AcpViewMessage, from: StreamState = emptyStream('w')) => {
  const state = applyView(from, view);
  return { state, drawn: state.entries.map((e) => html({ ...base, entry: e })) };
};

describe('conformance: the client takes every standard session/update kind', () => {
  it('has a sample of each of the 14 kinds', () => {
    expect(Object.keys(vector.per_kind)).toEqual(STANDARD);
    expect(vector.updates.map((u) => u.sessionUpdate)).toEqual(STANDARD);
  });

  for (const kind of STANDARD) {
    it(`${kind}: reduces and renders, or leaves the view as it was`, () => {
      const view = asView((vector.per_kind as Record<string, unknown>)[kind]);
      const { state, drawn } = show(view);
      expect(state.entries.length).toBe(view.entries.length);
      for (const out of drawn) expect(out.length).toBeGreaterThan(0);
      if (view.info) expect(state.info?.agent).toBe('agent');
    });
  }

  it('all of them together make the entries and the facts the adapter makes of them', () => {
    const { state, drawn } = show(asView(vector.view));
    expect(state.entries.map((e) => e.kind)).toEqual(['message', 'thought', 'tool', 'plan']);
    expect(drawn.join('')).toContain('Looking at it.');
    expect(drawn.join('')).toContain('Edit build.rs');
    expect(state.info).toMatchObject({ title: 'Fix the build', mode: 'plan', usage: { used: 1200, size: 200000 } });
    expect(state.info?.commands.map((c) => c.name)).toEqual(['compact']);
  });
});

describe('conformance: what the client does not know is skipped, never fatal', () => {
  const view = asView(vector.view);
  const known = show(view);

  it('an entry kind from a newer server is dropped and the rest is shown', () => {
    const odd = { id: 99, rev: 99, kind: 'hologram', text: 'boo', payload: { x: 1 } };
    const { state, drawn } = show({ ...view, entries: [...view.entries, odd] });
    expect(state.entries.map((e) => e.id)).toEqual(known.state.entries.map((e) => e.id));
    expect(drawn).toEqual(known.drawn);
  });

  it('an extension on an entry, on the view or on the facts changes nothing that is shown', () => {
    const ext = { _meta: { 'termenbox.dev/seq': 9, 'zed.dev/debugMode': true }, 'termenbox.dev/tier': { tier: 'T1' } };
    const extended = {
      ...view, ...ext,
      entries: view.entries.map((e) => ({ ...e, ...ext })),
      info: { ...view.info, ...ext },
    } as unknown as AcpViewMessage;
    const { state, drawn } = show(extended);
    expect(state.entries).toEqual(known.state.entries);
    expect(state.info).toEqual(known.state.info);
    expect(drawn).toEqual(known.drawn);
  });

  it('an unknown content type, odd shapes and wrong types in known entries render something generic', () => {
    const hostile = [
      { id: 1, rev: 1, kind: 'message', text: { type: 'hologram', data: '?' } },
      { id: 2, rev: 2, kind: 'user', text: 7 },
      { id: 3, rev: 3, kind: 'tool', tool_id: null, title: { x: 1 }, tool_kind: 'teleport', status: 'on_fire', diffs: 'no', locations: [null, 4] },
      { id: 4, rev: 4, kind: 'plan', items: [{ text: 'ok', status: 'new_status' }, null, 'x'] },
      { id: 5, rev: 5, kind: 'permission', prompt_id: 1, options: [null, { id: 3 }], state: 'maybe' },
      { id: 6, rev: 6, kind: 'notice', level: 'catastrophe', text: null },
      null,
      { rev: 7, kind: 'message' },
    ];
    const { state, drawn } = show({ ...view, reset: true, entries: hostile } as unknown as AcpViewMessage);
    expect(state.entries.length).toBeGreaterThan(0);
    expect(drawn.every((out) => out.length > 0)).toBe(true);
    expect(state.entries.find((e) => e.id === 3)).toMatchObject({ kind: 'tool', toolKind: 'teleport', status: 'on_fire' });
  });

  it('facts of the wrong shape fall back to defaults', () => {
    const { state } = show({ ...view, info: { agent: 5, dir: null, status: 'warp', modes: 'x', commands: [null], usage: { used: 'many' } } } as unknown as AcpViewMessage);
    expect(state.info?.status).toBe('running');
    expect(state.info?.modes).toEqual([]);
    expect(state.info?.usage).toBeUndefined();
  });
});
