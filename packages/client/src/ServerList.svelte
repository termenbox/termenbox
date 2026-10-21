<script lang="ts">
  import type { ServerListItem } from './lib/protocol';
  import { serverKey, type HubServer } from './lib/hub';
  import PortHelp from './PortHelp.svelte';
  import PathReport from './PathReport.svelte';

  export let servers: ServerListItem[] = [];
  export let states: HubServer[] = [];
  export let onpause: (id: string) => void;
  export let onresume: (id: string) => void;
  export let onretry: (id: string) => void;
  export let onadd: () => void;
  export let onremove: (index: number) => void;
  export let onshare: (index: number) => void;
</script>

<div class="server-list" style="flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 48px 24px;">
  <div style="width: 100%; max-width: 600px;">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 32px;">
      <h2 style="font-size: 1.5rem;">Your Servers</h2>
      <button on:click={onadd} style="display: flex; align-items: center; gap: 8px;">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
        Add Server
      </button>
    </div>

    {#if servers.length === 0}
      <div class="empty-state" style="text-align: center; padding: 64px 24px; background: var(--bg-secondary); border: 1px solid var(--border); border-radius: 12px;">
        <svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="var(--fg-muted)" stroke-width="1.5" style="margin-bottom: 16px;"><rect x="2" y="3" width="20" height="14" rx="2"></rect><path d="M8 21h8M12 17v4"></path></svg>
        <h3 style="margin-bottom: 8px;">No servers yet</h3>
        <p style="color: var(--fg-muted); margin-bottom: 24px;">Deploy a server on your VPS and register it here</p>
        <button on:click={onadd} style="display: inline-flex; align-items: center; gap: 8px;">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
          Add Your First Server
        </button>
      </div>
    {:else}
      <div style="display: grid; gap: 16px;">
        {#each servers as server, index}
          {@const status = states.find(s => s.id === serverKey(server))}
          <div class="server-card" style="background: var(--bg-secondary); border: 1px solid var(--border); border-radius: 12px; padding: 20px; display: flex; align-items: center; gap: 16px; transition: border-color 0.15s;" on:mouseenter={() => {}} on:mouseleave={() => {}}>
            <div style="width: 48px; height: 48px; border-radius: 10px; background: linear-gradient(135deg, var(--accent), #a371f7); display: flex; align-items: center; justify-content: center;">
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2"><rect x="2" y="3" width="20" height="14" rx="2"></rect><path d="M8 21h8M12 17v4"></path></svg>
            </div>
            <div style="flex: 1; min-width: 0;">
              <h3 style="font-size: 1rem; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">{server.server_name || 'Server'}</h3>
              <p style="font-size: 0.75rem; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">{server.server_url || (server.record ? `${server.record.ip}:${server.record.udp_port}` : server.bundle ? 'Paired by link' : '')}</p>
              <p class="state">{status?.state ?? 'connecting'}{#if status?.state === 'connected' && status.path} · {status.path}{/if}</p>
              {#if status?.reason}<p class="reason">{status.reason}</p>{/if}
              {#if status?.state === 'unreachable'}
                {#if status.failures?.length}<PathReport failures={status.failures} />{/if}
                <details class="help">
                  <summary>Why can't I connect?</summary>
                  <p class="note">The server may be stopped, or its UDP port may be blocked. On the machine, <code>systemctl status termenbox</code> (or <code>systemctl --user status termenbox</code> for a user install) says whether it runs; a server whose port is taken stops with a message in <code>journalctl</code>.</p>
                  {#if server.record?.udp_port}<PortHelp port={server.record.udp_port} />{/if}
                </details>
              {/if}
            </div>
            <div style="display: flex; gap: 8px;">
              <button class="secondary" aria-pressed={status?.state === 'paused'} on:click={() => status?.state === 'paused' ? onresume(serverKey(server)) : onpause(serverKey(server))}>{status?.state === 'paused' ? 'Resume' : 'Pause'}</button>
              {#if status && status.state !== 'connected' && status.state !== 'paused'}<button class="secondary" on:click={() => onretry(serverKey(server))}>Retry</button>{/if}
              {#if server.server_id}
                <button class="secondary" aria-label="Send to another device" title="Send to another device" on:click={() => onshare(index)} style="padding: 8px;">
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 3v12M8 7l4-4 4 4"></path><path d="M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7"></path></svg>
                </button>
              {/if}
              <button class="secondary" aria-label="Remove server" on:click={() => confirm(`Remove "${server.server_name}" from this browser? You can add it again with its link or QR.`) && onremove(index)} style="padding: 8px; color: var(--danger);">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .server-list { overflow-y: auto; }
  .server-card { flex-wrap: wrap; }
  .state { font-size: 0.8rem; text-transform: capitalize; }
  .help { font-size: 0.85rem; margin-top: 6px; }
  .help summary { cursor: pointer; color: var(--fg-muted); }
  .help .note { color: var(--fg-muted); margin: 8px 0; }
  .reason { font-size: 0.8rem; color: var(--fg-muted); overflow-wrap: anywhere; }
  button { min-height: 44px; min-width: 44px; }
</style>
