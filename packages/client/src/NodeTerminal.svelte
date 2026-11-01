<script lang="ts">
  import { onMount } from 'svelte';
  import Terminal from './Terminal.svelte';
  import type { HubConnection } from './lib/hub';
  import type { NodeTerminalTarget } from './lib/fleet';
  /** A node's terminal opened from a fleet card (step 2.5): bytes come through the conductor only while this is open. */
  let { connection, target, onclose }: { connection: HubConnection; target: NodeTerminalTarget; onclose: () => void } = $props();
  let state = $state<'opening' | 'open' | 'closed'>('opening');
  onMount(() => {
    const stop = connection.subscribe((msg) => {
      if (msg.type === 'conductor_terminal_opened') state = 'open';
      else if (msg.type === 'conductor_terminal_closed') state = 'closed';
    });
    // Zero size: the server uses the size this connection already reports, and the terminal view resizes right after.
    connection.send({ type: 'conductor_open_terminal', node: target.node, workspace: target.workspace, tab: target.tab, cols: 0, rows: 0 });
    return () => { stop(); connection.send({ type: 'conductor_close_terminal' }); };
  });
</script>

<div class="node-terminal" role="dialog" aria-label={`Terminal on ${target.nodeName}`}>
  <header>
    <button type="button" class="secondary" onclick={onclose}>Back</button>
    <span class="title">{target.nodeName} · {target.workspace}</span>
    <span class="state" role="status">{state === 'opening' ? 'Opening…' : state === 'closed' ? 'Closed' : ''}</span>
  </header>
  <div class="body"><Terminal {connection} activeWorkspaceId={target.id} /></div>
</div>

<style>
  .node-terminal { position: fixed; inset: 0; z-index: 30; display: flex; flex-direction: column; background: var(--bg, #000); }
  header { display: flex; align-items: center; gap: 12px; padding: 8px 12px; }
  header button { min-height: 44px; min-width: 44px; }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .body { flex: 1; min-height: 0; display: flex; }
  .body > :global(*) { flex: 1; min-width: 0; }
</style>
