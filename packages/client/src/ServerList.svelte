<script lang="ts">
  import type { ServerListItem } from './lib/protocol';
  import { serverKey, type HubServer } from './lib/hub';
  import ServerMenu from './ServerMenu.svelte';
  import ServerIssue from './ServerIssue.svelte';
  import ServerUpdatesDialog from './ServerUpdatesDialog.svelte';

  export let servers: ServerListItem[] = [];
  export let states: HubServer[] = [];
  /** Workspaces each connected server has, by server id. */
  export let counts: Record<string, number> = {};
  export let onpause: (id: string) => void;
  export let onresume: (id: string) => void;
  export let onretry: (id: string) => void;
  export let onadd: () => void;
  /** Scan QR (decision 0127): the camera opens full-screen and a pairing link goes straight to Confirm. */
  export let onscan: () => void = () => {};
  export let onremove: (index: number) => void;
  export let onshare: (index: number) => void;
  /** Gives the server a new name on this device only (the server keeps its own). */
  export let onrename: (index: number, name: string) => void = () => {};
  /** Asks the server to install its newer release. */
  export let onupdate: (id: string) => void = () => {};
  /** The Updates card's other actions (decision 0153); the server decides which of them its helper accepts. */
  export let oncheck: (id: string) => void = () => {};
  export let onpostpone: (id: string) => void = () => {};
  export let onskip: (id: string) => void = () => {};
  /** The server whose Updates card is open; the banner opens it too. */
  export let updatesFor: string | null = null;

  const word = (state: string) => (state === 'connected' ? 'Connected' : state.charAt(0).toUpperCase() + state.slice(1));
  const workspacesText = (n: number) => (n === 1 ? '1 workspace' : `${n} workspaces`);
  /** `0.1.53+abc1234` shows as `v0.1.53`: the build is for bug reports, not for the card. */
  const shortVersion = (v: string) => (v ? `v${v.split('+')[0]}` : '');
  const update = (server: ServerListItem, from: string, to: string) => {
    if (confirm(`Update "${server.server_name || 'Server'}" from ${shortVersion(from) || 'its current version'} to v${to}? The server restarts and reconnects in a few seconds.`)) onupdate(serverKey(server));
  };
  $: updatesStatus = updatesFor ? states.find(s => s.id === updatesFor) : undefined;
  $: updatesServer = updatesFor ? servers.find(s => serverKey(s) === updatesFor) : undefined;
  // Rename in place, as the sidebar does for workspaces: Enter saves, Esc or leaving the field cancels.
  let editing: string | null = null;
  let draft = '';
  const startEdit = (server: ServerListItem) => {
    editing = serverKey(server); draft = server.server_name || '';
  };
  const commit = (server: ServerListItem, index: number) => {
    if (editing !== serverKey(server)) return;
    editing = null;
    const next = draft.trim();
    if (next && next !== server.server_name) onrename(index, next);
  };
  // The menu gives focus back to its button as it closes, so the field takes it afterwards.
  const focusSelect = (el: HTMLInputElement) => { setTimeout(() => { el.focus(); el.select(); }, 60); };
  const remove = (server: ServerListItem, index: number) => {
    if (confirm(`Remove "${server.server_name}" from this browser? You can add it again with its link or QR.`)) onremove(index);
  };
</script>

{#if updatesFor && updatesServer}
  <ServerUpdatesDialog status={updatesStatus} name={updatesServer.server_name || 'Server'} onclose={() => { updatesFor = null; }}
    oncheck={() => oncheck(updatesFor ?? '')} onpostpone={() => onpostpone(updatesFor ?? '')} onskip={() => onskip(updatesFor ?? '')}
    onupdate={() => { if (updatesStatus?.updateAvailable) update(updatesServer, updatesStatus.version, updatesStatus.updateAvailable); }} />
{/if}

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
          {@const available = status?.updateAvailable ?? ''}
          <!-- A row has one height whatever the server is doing: the name, one line of state, and two fixed slots at the end. -->
          <div class="server-card" data-server={serverKey(server)} data-state={state}>
            <div class="info">
              <div class="title">
                {#if editing === serverKey(server)}
                  <input use:focusSelect bind:value={draft} aria-label="Server name" maxlength="64" autocomplete="off" spellcheck="false" data-testid="server-rename"
                    on:keydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); commit(server, index); } else if (e.key === 'Escape') { e.preventDefault(); editing = null; } }} on:blur={() => commit(server, index)} />
                {:else}
                  <h3>{server.server_name || 'Server'}</h3>
                {/if}
                {#if status?.version}<span class="version" data-testid="server-version">{shortVersion(status.version)}</span>{/if}
                {#if status?.updating}
                  <span class="updating" data-testid="server-updating" role="status">Updating…</span>
                {:else if status && state === 'connected' && available}
                  <button type="button" class="update" data-testid="server-update" title={`Update to v${available}`} aria-label={`Update ${server.server_name || 'Server'} to v${available}`}
                    on:click={() => update(server, status.version, available)}>
                    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"></circle><path d="M12 16V8M8.5 11.5 12 8l3.5 3.5"></path></svg>
                  </button>
                {/if}
              </div>
              <p class="addr">{server.server_url || (server.record ? `${server.record.ip}:${server.record.udp_port}` : server.bundle ? 'Paired by link' : '')}</p>
              <p class="state" class:ok={state === 'connected'}>
                <span class="part"><span aria-hidden="true">{state === 'connected' ? '●' : '○'}</span> {word(state)}</span>
                {#if state === 'connected' && status?.path}<span class="dot" aria-hidden="true">·</span><span class="part sep">{status.path}</span>{/if}
                {#if state === 'connected'}<span class="dot" aria-hidden="true">·</span><span class="part sep" data-testid="workspace-count">{workspacesText(counts[serverKey(server)] ?? 0)}</span>{/if}
              </p>
            </div>
            <span class="slot">{#if status && state !== 'connected'}<ServerIssue name={server.server_name || 'Server'} {status} udpPort={server.record?.udp_port} />{/if}</span>
            <ServerMenu name={server.server_name || 'Server'} paused={state === 'paused'} canRetry={!!status && state !== 'connected' && state !== 'paused'} canShare={!!server.server_id} canUpdates={state === 'connected'} onupdates={() => { updatesFor = serverKey(server); }}
              onpause={() => onpause(serverKey(server))} onresume={() => onresume(serverKey(server))} onretry={() => onretry(serverKey(server))} onshare={() => onshare(index)} onrename={() => startEdit(server)} onremove={() => remove(server, index)} />
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
  .title { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .title input { flex: 1; min-width: 0; height: 32px; padding: 0 8px; font-size: 1rem; font-weight: 600; color: var(--fg); background: var(--bg); border: 1px solid var(--accent); border-radius: 6px; }
  .version { flex-shrink: 0; font-size: 0.75rem; color: var(--fg-muted); }
  .updating { flex-shrink: 0; font-size: 0.75rem; color: var(--warn); }
  /* A yellow button the size of a finger, with the row's height unchanged (it overlaps the card's padding). */
  .update { display: inline-grid; flex-shrink: 0; place-items: center; width: 44px; height: 44px; margin: -12px -8px -12px 0; padding: 0; color: var(--warn); background: transparent; border: 0; border-radius: 8px; cursor: pointer; }
  .update:hover { background: color-mix(in srgb, var(--warn) 15%, transparent); }
  .info h3 { font-size: 1rem; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .info p { font-size: 0.8rem; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .addr { font-size: 0.75rem; }
  .state.ok { color: var(--fg); }
  /* The state line is one row of parts with the separator between them as its own element, so it never depends on the spaces around markup. */
  .state { display: flex; min-width: 0; }
  .part { flex-shrink: 0; white-space: nowrap; }
  .part.sep { flex-shrink: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .dot { flex-shrink: 0; margin: 0 0.5em; color: var(--fg-muted); }
  /* The slot is there whether it holds the issue button or not, so the end of the row never moves. */
  .slot { display: inline-grid; width: 44px; height: 44px; flex-shrink: 0; place-items: center; }
  button { min-height: 44px; min-width: 44px; }
</style>
