<script lang="ts">
  // The decision card in the inbox and in the workspace view (?surface=inbox|workspace), fed by plain props. window.__decision
  // reads what the user sent and lets the e2e do what the server does: clear the Attention item.
  import InboxSheet from '../src/InboxSheet.svelte';
  import SessionView from '../src/SessionView.svelte';
  import type { AgentMap, WorkspaceAgent } from '../src/lib/agents';
  import type { InboxItem } from '../src/lib/inbox';
  import type { ViewModel } from '../src/lib/sessionView';

  const surface = new URLSearchParams(location.search).get('surface') ?? 'inbox';
  const t0 = Date.now() - 4 * 60_000;
  const needs: WorkspaceAgent = {
    state: 'unknown', seq: 3, stale: false, ts: Date.now(), serverStatus: true, agent: 'claude',
    attention: { id: 'a1', workspace: 'api', kind: 'needs_input', raised_at: t0, prompt: 'Allow the edit?', task: { title: 'Move login to the workspace store', started_at: t0 - 60_000, source: 'hook' } },
    prompt: { id: 'p1', kind: 'choice', choices: ['Yes', 'Yes, always', 'No'], seq: 3, confidence: 'high' },
    tool: { name: 'Edit', summary: 'src/auth/session.ts +12 -3', phase: 'started' },
  };
  let agents: AgentMap = $state({ 'srv/api': needs });
  let now = $state(Date.now());
  setInterval(() => (now = Date.now()), 1000);
  const items: InboxItem[] = [
    { id: 'srv:a1:' + t0, key: 'srv/api', name: 'api', kind: 'needs_input', at: t0, title: 'Move login to the workspace store', read: false },
    { id: 'srv:a0:1', key: 'srv/docs', name: 'docs', kind: 'finished_unseen', at: t0 - 1000, title: 'Update the README', read: false },
  ];
  const sent: unknown[] = [];
  let view: ViewModel | null = $state({ workspace: 'api', tier: 't1', tail: ['$ claude', 'Allow the edit?'], hash: 'h1', agent: 'claude',
    prompt: { id: 'p1', question: 'Allow the edit?', kind: 'choice', options: [{ label: 'Yes' }, { label: 'Yes, always' }, { label: 'No' }] } });
  Object.assign(window, { __decision: { sent, clear: () => { agents = {}; view = view && { ...view, prompt: undefined }; } } });
</script>

{#if surface === 'inbox'}
  <InboxSheet {items} {now} {agents} onopen={(k) => sent.push({ open: k })} onread={() => {}} onclear={() => {}} onclose={() => {}}
    onanswer={(key, choice, prompt) => sent.push({ type: 'answer', workspace: key, prompt_id: prompt.id, seq: prompt.seq, choice })}
    onreply={(key, text) => sent.push({ type: 'reply', workspace: key, text })} />
{:else}
  <div style="height:100dvh">
    <SessionView name="api" agent="claude" fleet={view?.prompt ? 'needs-you' : 'idle'} task="Move login to the workspace store" waitingSince={t0} {now} {view}
      step={{ name: 'Edit', summary: 'src/auth/session.ts +12 -3' }}
      onanswer={(m) => sent.push(m)} onreply={(line) => sent.push({ type: 'reply', workspace: 'api', text: line })} onterminal={() => sent.push({ terminal: true })} />
  </div>
{/if}
