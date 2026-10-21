<script lang="ts">
  import type { HubServer } from './lib/hub';
  import PathReport from './PathReport.svelte';
  export let server: HubServer;
  export let onretry: (id: string) => void;
</script>
<div class="server-status" class:degraded={server.state !== 'connected'} data-server={server.id}>
  <div><span>{server.state}</span>{#if server.version}<span class="version">server {server.version}</span>{/if}</div>
  {#if server.reason}<p>{server.reason}</p>{/if}
  {#if server.state === 'connected' && server.path}<p>{server.path}</p>{/if}
  {#if server.state === 'unreachable' && server.failures?.length}<PathReport failures={server.failures} />{/if}
  {#if server.state === 'connected' && server.backends.includes('tmux') && !server.controlMode && server.tmuxVersion}<p class="notice">tmux {server.tmuxVersion}: live window updates need 3.2 or newer</p>{/if}
  {#if server.state !== 'connected' && server.state !== 'paused'}<button class="secondary" on:click={() => onretry(server.id)}>Retry</button>{/if}
</div>
<style>
  .server-status { font-size: 0.75rem; color: var(--fg-muted); overflow-wrap: anywhere; }
  .degraded { border-left: 2px solid var(--danger); padding-left: 8px; }
  .version { display: block; }
  p { margin-top: 4px; }
  button { min-height: 44px; min-width: 44px; margin-top: 6px; }
</style>
