// The real workspace list on a fake event stream, for screenshots and the cards e2e check.
import { mount } from 'svelte';
import '../src/app.css';
import WorkspaceList from '../src/WorkspaceList.svelte';
import { agents, applyEvents } from '../src/lib/agents';
import { applyActivity } from '../src/lib/status';
import type { AgentEvent } from '../src/lib/protocol';

const workspaces = [
  { id: 'build', name: 'build', agent: null, backend: 'tmux' },
  { id: 'api-refactor', name: 'api-refactor', agent: null, backend: 'tmux' },
  { id: 'docs', name: 'docs', agent: null, backend: 'tmux' },
  { id: 'deploy', name: 'deploy', agent: null, backend: 'tmux' },
  { id: 'scratch', name: 'scratch', agent: null },
];
const now = Date.now();
const ev = (workspace: string, seq: number, rest: object, ago = 0, confidence = 'high'): AgentEvent =>
  ({ workspace, seq, ts: now - ago, source: 'hook', confidence, ...rest }) as AgentEvent;
const stream: AgentEvent[] = [
  ev('build', 1, { kind: 'state', state: 'working' }, 4000),
  ev('build', 2, { kind: 'tool', name: 'Edit', summary: 'packages/client/src/lib/terminal/touchScroll.ts and a very long path to prove it is cut off', phase: 'started' }, 3000),
  ev('build', 3, { kind: 'plan', items: [{ text: 'Write the controller', status: 'done' }, { text: 'Wire it in', status: 'done' }, { text: 'Test on a phone', status: 'todo' }] }, 3000),
  ev('api-refactor', 1, { kind: 'state', state: 'needs_input' }, 20000),
  ev('api-refactor', 2, { kind: 'prompt', id: '1-ab12cd34', prompt_kind: 'choice', choices: ['Yes', 'Yes, and always allow access to /tmp from this project', 'No'] }, 20000),
  ev('docs', 1, { kind: 'state', state: 'done' }, 5 * 60000),
  ev('docs', 2, { kind: 'message', summary: 'Updated the README and the plan.' }, 5 * 60000),
  ev('deploy', 1, { kind: 'state', state: 'working' }, 4 * 60000),
  ev('deploy', 2, { kind: 'tool', name: 'Bash', summary: 'cargo build --release', phase: 'started' }, 4 * 60000),
];
// `build` also has server status (decision 0095): a task that started 95 s ago on the server's clock.
agents.set(applyActivity(applyEvents({}, stream).map, [{ workspace: 'build', activity: 'working', ts: now, task: { title: 'Wire the touch controller in', started_at: now - 95_000, source: 'hook' } }], now));
const sent: unknown[] = [];
(window as unknown as { __sent: unknown[]; __agents: typeof agents }).__sent = sent;
mount(WorkspaceList, { target: document.body, props: { workspaces, activeWorkspaceId: 'build', onopen: (id: string) => sent.push({ open: id }), send: (m: unknown) => sent.push(m) } });
document.body.style.cssText = 'padding:12px;max-width:390px;background:var(--bg-secondary)';
