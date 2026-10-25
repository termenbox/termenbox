<script lang="ts">
  // The real Home on 20 workspaces across two servers, fed by plain props. window.__home reads what the user answered and
  // lets the e2e change the data the way a server would. The page body scrolls like the app's main area does not: Home
  // owns its scroll container.
  import Home from '../src/Home.svelte';
  import type { AgentMap, WorkspaceAgent } from '../src/lib/agents';
  import type { HubServer, HubWorkspace } from '../src/lib/hub';
  import type { AttentionKind } from '../src/lib/protocol';

  const now0 = Date.now();
  const servers = ['alpha', 'beta'].map((id) => ({ id, state: 'connected' }) as unknown as HubServer);
  const workspaces: HubWorkspace[] = Array.from({ length: 20 }, (_, i) => {
    const serverId = i % 2 ? 'beta' : 'alpha';
    return { id: `s${i}`, name: `task-${i}`, agent: null, serverId, serverName: serverId, key: `${serverId}/s${i}` };
  });
  const att = (kind: AttentionKind, ageMin: number, i: number): WorkspaceAgent => ({
    state: 'unknown', seq: 1, stale: false, ts: now0, serverStatus: true, agent: 'claude',
    attention: { id: `a${i}`, session: `s${i}`, kind, raised_at: now0 - ageMin * 60_000, task: { title: `Refactor part ${i}`, started_at: now0 - 3_600_000, source: 'hook' }, prompt: kind === 'needs_input' ? `Run migration ${i}?` : undefined },
    ...(kind === 'needs_input' ? { prompt: { id: `p${i}`, kind: 'choice', choices: ['Yes', 'Yes, always', 'No'], seq: 7 + i, confidence: 'high' }, tool: { name: 'Bash', summary: `pnpm test --filter client # ${i}`, phase: 'started' } } : {}),
  });
  const base = (activity: 'working' | 'idle'): WorkspaceAgent => ({ state: activity === 'working' ? 'working' : 'idle', seq: 1, stale: false, ts: now0, serverStatus: true, activity, agent: 'codex' });
  // 3 need you (oldest: task-4, 30 min), 2 failed, 4 finished, 6 working, 5 idle.
  const initial = (): AgentMap => {
    const m: AgentMap = {};
    workspaces.forEach((s, i) => {
      m[s.key] = i === 4 ? att('needs_input', 30, i) : i === 1 ? att('needs_input', 12, i) : i === 8 ? att('needs_input', 5, i)
        : i === 2 || i === 3 ? att('failed', 20 - i, i) : i >= 10 && i < 14 ? att('finished_unseen', 40 - i, i) : i >= 14 ? base('working') : base('idle');
    });
    return m;
  };
  let agents: AgentMap = $state(initial());
  let now = $state(Date.now());
  const answers: unknown[] = [];
  const replies: unknown[] = [];
  const opened: string[] = [];
  Object.assign(window, { __home: { answers, replies, opened, agents: () => agents, set: (key: string, a: WorkspaceAgent | null) => { const n = { ...agents }; if (a) n[key] = a; else delete n[key]; agents = n; }, tick: () => { now = Date.now(); } } });
  setInterval(() => (now = Date.now()), 1000);
</script>

<div style="display:flex;flex-direction:column;height:100dvh">
  <Home {workspaces} {agents} {servers} {now} lastKey={'alpha/s0'} onopen={(k) => opened.push(k)} onservers={() => {}} onnew={() => {}}
    onreply={(key, text) => replies.push({ type: 'reply', session: key, text })}
    onanswer={(key, choice, prompt) => answers.push({ type: 'answer', session: key, prompt_id: prompt.id, seq: prompt.seq, choice })} />
</div>
