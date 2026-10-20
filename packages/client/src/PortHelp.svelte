<script lang="ts">
  import { firewallCommands } from './lib/onboard';

  /** The server's UDP port from its directory record, when known. */
  export let port: number | null;
  $: cmds = firewallCommands(port);
  $: label = port ? `UDP port ${port}` : 'its UDP port';
</script>

<p class="note">Whoever administers the server opens {label} once, with the command for its firewall:</p>
{#each cmds as c}
  <p class="note tool">{c.tool}</p>
  <pre class="command">{c.command}</pre>
{/each}
<p class="note">If the host also has a network firewall or security group (a cloud provider's), allow inbound {label} there too. Your own network (cafe, work or mobile data) may block UDP as well; try another network if the port is open.</p>

<style>
  .note { color: var(--fg-muted); font-size: 0.85rem; margin: 8px 0; }
  .tool { margin: 4px 0 0; font-size: 0.8rem; }
  .command { background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 12px; font-family: monospace; font-size: 0.75rem; white-space: pre-wrap; overflow-wrap: anywhere; user-select: all; cursor: copy; margin: 0 0 8px; }
</style>
