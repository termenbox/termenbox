<script lang="ts">
  // The Workspaces list as the phone shows it with nothing open (WorkspaceGroups) and the workspace actions sheet, on the real
  // ServerHub with two fake servers: the user's order, dragging by the handle, Move up / Move down, and renaming.
  import WorkspaceGroups from '../src/WorkspaceGroups.svelte';
  import WorkspaceActions from '../src/WorkspaceActions.svelte';
  import { ServerHub, type HubConnection } from '../src/lib/hub';
  import { moveWorkspace, placementOf } from '../src/lib/order';
  import type { ConnectionOptions } from '../src/lib/connection';
  import type { ClientMessage, ServerMessage, WorkspaceInfo } from '../src/lib/protocol';

  const messages: { server: string; message: ClientMessage }[] = [];
  const COUNT: Record<string, number> = { alpha: 10, beta: 2 };
  class Fake implements HubConnection {
    listener?: (m: ServerMessage) => void;
    list: WorkspaceInfo[];
    opts: ConnectionOptions;
    constructor(opts: ConnectionOptions) {
      this.opts = opts;
      this.list = Array.from({ length: COUNT[opts.serverId!] }, (_, i) => ({ id: `w${i}`, name: `${opts.serverId}-${i}`, agent: null }));
    }
    start() {
      this.opts.onState('connected'); this.opts.onReady({ type: 'auth_ok', user: 'test', server_name: this.opts.serverId! }, false);
      this.emit({ type: 'workspaces', workspaces: this.list });
      // alpha-3 needs you, alpha-5 is working: neither may move.
      this.emit({ type: 'agent_snapshot', states: this.list.map((s, i) => ({ workspace: s.id, state: this.opts.serverId === 'alpha' && i === 3 ? 'needs_input' : this.opts.serverId === 'alpha' && i === 5 ? 'working' : 'idle', seq: 1, ts: Date.now(), stale: false, agent: 'claude' })) });
    }
    close() { this.opts.onState('closed'); }
    send(message: ClientMessage) { messages.push({ server: this.opts.serverId!, message }); }
    subscribe(fn: (m: ServerMessage) => void) { this.listener = fn; return () => { this.listener = undefined; }; }
    emit(m: ServerMessage) { this.listener?.(m); }
  }
  const hub = new ServerHub({ credentials: async () => ({ cert: '', sign: async () => '' }), factory: opts => new Fake(opts) });
  hub.start(['alpha', 'beta'].map(id => ({ server_id: id, server_name: id, server_url: '', registered_at: 0 })));
  const workspaces = hub.allWorkspaces, agents = hub.agents;
  let actions: string | null = null;
  let opened: string | null = null;
  $: actionWorkspace = $workspaces.find(s => s.key === actions);
  Object.assign(window, { __reorder: {
    order: () => $workspaces.map(s => s.name),
    opened: () => opened,
    sent: () => messages.map(m => m.message),
  } });
</script>
<div class="page">
  <h1>Workspaces</h1>
  <WorkspaceGroups workspaces={$workspaces} agents={$agents} now={Date.now()} onopen={key => opened = key} onnew={() => {}} onactions={key => actions = key} />
</div>
{#if actionWorkspace}<WorkspaceActions workspace={actionWorkspace} onrename={name => hub.rename(actionWorkspace.key, name)} placement={placementOf($workspaces, actionWorkspace.key)} onmove={to => moveWorkspace($workspaces, actionWorkspace.key, to)} ondetach={() => {}} oncloseworkspace={() => {}} onclose={() => actions = null} />{/if}
<style>
  .page { display: flex; flex-direction: column; height: 100dvh; background: var(--bg-secondary); color: var(--fg); }
  h1 { padding: 12px 16px 0; font-size: 1.1rem; }
</style>
