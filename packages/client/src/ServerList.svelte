<script lang="ts">
  import type { ServerListItem } from './lib/protocol';
  import { serverKey, type HubServer } from './lib/hub';
  import ServerMenu from './ServerMenu.svelte';
  import ServerIssue from './ServerIssue.svelte';

  export let servers: ServerListItem[] = [];
  export let states: HubServer[] = [];
  /** Sessions each connected server has, by server id. */
  export let counts: Record<string, number> = {};
  export let onpause: (id: string) => void;
  export let onresume: (id: string) => void;
  export let onretry: (id: string) => void;
  export let onadd: () => void;
  /** Scan QR (decision 0127): the camera opens full-screen and a pairing link goes straight to Confirm. */
  export let onscan: () => void = () => {};
  export let onremove: (index: number) => void;
  export let onshare: (index: number) => void;

  const word = (state: string) => (state === 'connected' ? 'Connected' : state.charAt(0).toUpperCase() + state.slice(1));
  const sessionsText = (n: number) => (n === 1 ? '1 session' : `${n} sessions`);
  const remove = (server: ServerListItem, index: number) => {
    if (confirm(`Remove "${server.server_name}" from this browser? You can add it again with its link or QR.`)) onremove(index);
  };
</script>

<div class="server-list" style="flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 48px 24px;">
  <div style="width: 100%; max-width: 600px;">
    <div style="display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 12px; margin-bottom: 32px;">
      <h2 style="font-size: 1.5rem;">Your Servers</h2>
      <div style="display: flex; gap: 8px;">
        <button class="secondary" on:click={onscan} data-testid="scan-qr">Scan QR</button>
        <button on:click={onadd} style="display: flex; align-items: center; gap: 8px;">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
          Add Server
        </button>
      </div>
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
      <div style="display: grid; gap: 12px;">
        {#each servers as server, index (serverKey(server))}
          {@const status = states.find(s => s.id === serverKey(server))}
          {@const state = status?.state ?? 'connecting'}
          <!-- A row has one height whatever the server is doing: the name, one line of state, and two fixed slots at the end. -->
          <div class="server-card" data-server={serverKey(server)} data-state={state}>
            <div class="info">
              <h3>{server.server_name || 'Server'}</h3>
              <p class="addr">{server.server_url || (server.record ? `${server.record.ip}:${server.record.udp_port}` : server.bundle ? 'Paired by link' : '')}</p>
              <p class="state" class:ok={state === 'connected'}>
                <span aria-hidden="true">{state === 'connected' ? '●' : '○'}</span> {word(state)}{#if state === 'connected' && status?.path} · {status.path}{/if}{#if state === 'connected'} · <span data-testid="session-count">{sessionsText(counts[serverKey(server)] ?? 0)}</span>{/if}
              </p>
            </div>
            <span class="slot">{#if status && state !== 'connected'}<ServerIssue name={server.server_name || 'Server'} {status} udpPort={server.record?.udp_port} />{/if}</span>
            <ServerMenu name={server.server_name || 'Server'} paused={state === 'paused'} canRetry={!!status && state !== 'connected' && state !== 'paused'} canShare={!!server.server_id}
              onpause={() => onpause(serverKey(server))} onresume={() => onresume(serverKey(server))} onretry={() => onretry(serverKey(server))} onshare={() => onshare(index)} onremove={() => remove(server, index)} />
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .server-list { overflow-y: auto; }
  .server-card { display: flex; align-items: center; gap: 4px; min-height: 84px; padding: 12px 8px 12px 20px; background: var(--bg-secondary); border: 1px solid var(--border); border-radius: 12px; }
  .info { flex: 1; min-width: 0; }
  .info h3 { font-size: 1rem; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .info p { font-size: 0.8rem; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .addr { font-size: 0.75rem; }
  .state.ok { color: var(--fg); }
  /* The slot is there whether it holds the issue button or not, so the end of the row never moves. */
  .slot { display: inline-grid; width: 44px; height: 44px; flex-shrink: 0; place-items: center; }
  button { min-height: 44px; min-width: 44px; }
</style>
