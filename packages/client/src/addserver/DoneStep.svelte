<script lang="ts">
  import type { Diagnosis } from '../lib/addServer';
  import type { Reachability } from '../lib/onboard';
  import Failure from './Failure.svelte';
  import PortHelp from '../PortHelp.svelte';

  /** Step 4: the active path and, when degraded, the plain-language fix. The server stays added whatever the check says. */
  export let checking: boolean;
  export let reach: Reachability | null;
  /** "Direct, LAN": the path that won, when the race named one. */
  export let pathLabel = '';
  export let ondone: () => void;

  const unreachable = (port: number | null): Diagnosis => ({
    cause: 'udp_closed',
    title: `This device could not reach it${port ? ` on UDP port ${port}` : ''}.`,
    action: { id: 'copy_firewall', label: 'Done' },
    detail: 'It stays in your list and keeps retrying. Direct connections need the UDP port open in the server\'s firewall; the commands are below.',
  });
</script>

<div class="done" role="status" aria-live="polite">
  <strong>Server added</strong>
  {#if checking}
    <p class="note">Checking that this device can reach it…</p>
  {:else if reach?.state === 'reachable'}
    <p class="note">This device can reach it{pathLabel ? `: ${pathLabel}` : ''}.</p>
  {:else if reach?.state === 'degraded'}
    <p class="note" data-testid="ws-notice">Connected through the WebSocket fallback because UDP did not work. Workspaces work but can feel slower. It stays in your list.</p>
    <details data-testid="failure-details"><summary>How to open UDP{reach.port ? ` port ${reach.port}` : ''}</summary><PortHelp port={reach.port} /></details>
  {:else if reach?.state === 'unreachable'}
    <Failure diagnosis={unreachable(reach.port)} showAction={false}><PortHelp port={reach.port} /></Failure>
  {:else}
    <p class="note">Could not tell whether this device can reach it. It stays in your list.</p>
  {/if}
  <button on:click={ondone} disabled={checking} style="width: 100%;">Done</button>
</div>

<style>
  .done { border: 1px solid var(--border); border-radius: 10px; padding: 12px; display: flex; flex-direction: column; gap: 8px; }
  .note { color: var(--fg-muted); font-size: 0.85rem; margin: 0; }
  summary { cursor: pointer; color: var(--fg-muted); font-size: 0.8rem; }
</style>
