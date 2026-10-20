<script lang="ts">
  import { onMount } from 'svelte';
  import { NativeSelect } from './lib/ui/native-select';
  import AgentIcon from './AgentIcon.svelte';
  import { recentDirs, suggestName, TEMPLATES, validDir, validName } from './lib/newsession';
  import type { HubServer, HubSession } from './lib/hub';
  import type { WindowMap } from './lib/windows';

  /** Servers a session can be created on (connected ones). */
  export let servers: HubServer[];
  /** The server to start with, and whether the sheet is fixed to it (opened from that server's own "+ New"). */
  export let initialServerId = '';
  export let locked = false;
  export let sessions: HubSession[];
  export let windows: WindowMap;
  export let oncreate: (req: { serverId: string; backend?: string; dir?: string; template?: string; name: string }) => void;
  export let onclose: () => void;

  let dialog: HTMLDialogElement;
  let serverId = servers.find(s => s.id === initialServerId)?.id ?? servers[0]?.id ?? '';
  let backend = '';
  let template = 'claude';
  let dir = '';
  let name = '';
  let nameTouched = false;
  onMount(() => dialog.showModal());

  $: server = servers.find(s => s.id === serverId);
  $: if (server && (!backend || !server.backends.includes(backend))) backend = server.backends[0] ?? '';
  // An ACP session (T0): the server starts the agent itself, in a directory, from the owner's list of agents.
  $: acp = !!server && backend === 'acp';
  $: acpAgents = server?.acpAgents ?? [];
  $: choices = acp ? acpAgents.map(id => ({ id, label: TEMPLATES.find(t => t.id === id)?.label ?? id })) : TEMPLATES;
  $: if (acp && !acpAgents.includes(template)) template = acpAgents[0] ?? '';
  $: if (!acp && !TEMPLATES.some(t => t.id === template)) template = 'claude';
  $: dirs = recentDirs(windows, `${serverId}/`);
  $: taken = sessions.filter(s => s.serverId === serverId).map(s => s.id);
  $: if (!nameTouched) name = suggestName(dir, template, taken);
  // Only tmux can start in a directory or run an agent; the sheet says so for other managers instead of ignoring them.
  $: tmux = !server || server.backends.length === 0 || backend === 'tmux';
  $: ok = !!server && validName(name) && validDir(dir) && !taken.includes(name) && (!acp || (!!template && dir !== ''));
  $: problem = acp && !template ? 'This server allows no agent sessions yet.' : acp && dir === '' ? 'Choose the directory the agent works in, like /home/you/project.' : !validDir(dir) ? 'The directory must be an absolute path, like /home/you/project.' : taken.includes(name) ? 'That name is taken.' : !validName(name) ? 'Use letters, digits, - and _ (up to 64).' : '';

  function create() {
    if (!ok || !server) return;
    oncreate({ serverId, backend: server.backends.length > 1 ? backend : undefined, name, ...((tmux || acp) && dir ? { dir } : {}), ...((tmux && template !== 'shell') || acp ? { template } : {}) });
    onclose();
  }
</script>

<dialog bind:this={dialog} aria-label="New session" on:cancel|preventDefault={onclose} on:click={(e) => { if (e.target === dialog) onclose(); }}>
  <form on:submit|preventDefault={create}>
    <h2>{locked && server ? `New session on ${server.server.server_name || 'Server'}` : 'New session'}</h2>
    {#if servers.length > 1 && !locked}
      <label>Server <NativeSelect bind:value={serverId}>{#each servers as s}<option value={s.id}>{s.server.server_name || 'Server'}</option>{/each}</NativeSelect></label>
    {/if}
    {#if server && server.backends.length > 1}
      <label>Manager <NativeSelect bind:value={backend}>{#each server.backends as b}<option value={b}>{b === 'acp' ? 'Agent session (ACP)' : b}</option>{/each}</NativeSelect></label>
    {/if}
    {#if tmux || acp}
      <div class="templates" role="radiogroup" aria-label="Start with">
        {#each choices as t}
          <button type="button" role="radio" aria-checked={template === t.id} class:on={template === t.id} on:click={() => template = t.id}>
            <AgentIcon command={t.id === 'shell' ? 'bash' : t.id} agent={t.id === 'shell' ? undefined : t.id} size={24} /><span>{t.label}</span>
          </button>
        {/each}
      </div>
      <label class="stack">Directory <input bind:value={dir} placeholder={acp ? '/home/you/project' : '/home/you/project (optional)'} autocapitalize="off" autocomplete="off" spellcheck="false" inputmode="url" /></label>
      {#if dirs.length}
        <div class="dirs" aria-label="Directories in use">{#each dirs as d}<button type="button" class="secondary" on:click={() => dir = d}>{d}</button>{/each}</div>
      {/if}
    {:else}
      <p class="note">{backend} starts sessions in its own default directory.</p>
    {/if}
    <label class="stack">Name <input bind:value={name} on:input={() => nameTouched = true} autocapitalize="off" autocomplete="off" spellcheck="false" /></label>
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
    <div class="actions"><button type="button" class="secondary" on:click={onclose}>Cancel</button><button type="submit" disabled={!ok}>Create</button></div>
  </form>
</dialog>

<style>
  dialog { margin: auto auto 0; width: min(100%, 460px); max-height: calc(var(--app-height, 100dvh) - 16px); overflow-y: auto; padding: 0; border: 1px solid var(--border); border-radius: 16px 16px 0 0; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0008; }
  form { padding: 16px; display: flex; flex-direction: column; gap: 10px; }
  .templates { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 6px; }
  .templates button { display: flex; align-items: center; gap: 8px; min-height: 48px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .templates button.on { border-color: var(--accent); }
  label { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 0.85rem; }
  label.stack { flex-direction: column; align-items: stretch; }
  input, button { min-height: 44px; min-width: 44px; }
  input { width: 100%; }
  .dirs { display: flex; flex-wrap: wrap; gap: 6px; }
  .dirs button { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .note { color: var(--fg-muted); font-size: 0.85rem; }
  .problem { color: var(--danger); font-size: 0.8rem; }
</style>
