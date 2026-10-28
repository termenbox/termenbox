<script lang="ts">
  import { onMount } from 'svelte';
  import { NativeSelect } from './lib/ui/native-select';
  import AgentIcon from './AgentIcon.svelte';
  import {
    backendFor,
    getRecentAgents,
    getSupportedTemplates,
    hasViews,
    loadView,
    managerOptions,
    orderTemplatesByRecent,
    recentDirs,
    resolveView,
    saveRecentAgent,
    saveView,
    suggestName,
    TEMPLATES,
    validDir,
    validName,
    viewControl,
    viewStore,
    type View,
  } from './lib/newworkspace';
  import type { HubServer, HubWorkspace } from './lib/hub';
  import type { WindowMap } from './lib/windows';

  /** Servers a workspace can be created on (connected ones). */
  export let servers: HubServer[];
  /** The server to start with, and whether the sheet is fixed to it (opened from that server's own "+ New"). */
  export let initialServerId = '';
  export let locked = false;
  export let workspaces: HubWorkspace[];
  export let windows: WindowMap;
  export let oncreate: (req: { serverId: string; backend?: string; dir?: string; template?: string; name: string }) => void;
  export let onclose: () => void;

  let dialog: HTMLDialogElement;
  let serverId = servers.find(s => s.id === initialServerId)?.id ?? servers[0]?.id ?? '';
  let manager = '';
  /** The View the person picked for this agent; `view` is what applies (some agents have no choice). */
  let wanted: View = 'terminal';
  let template = 'claude';
  let dir = '';
  let name = '';
  let nameTouched = false;
  let showAllAgents = false;
  let showDirPicker = false; // true when "Other..." is selected
  let dirInput: HTMLInputElement;
  let fileInput: HTMLInputElement;
  onMount(() => dialog.showModal());

  $: server = servers.find(s => s.id === serverId);
  $: supportedTemplates = server ? getSupportedTemplates(server) : TEMPLATES;
  $: recentAgents = server ? getRecentAgents(serverId) : [];
  $: orderedTemplates = orderTemplatesByRecent(supportedTemplates, recentAgents);
  $: visibleTemplates = showAllAgents ? orderedTemplates : orderedTemplates.slice(0, 6);
  $: hasMoreAgents = orderedTemplates.length > 6;
  // With tmux and acp both on the server the choice is the View, made after the agent. An agent tmux cannot start (one the
  // owner added) is offered too, and is always the agent view.
  $: views = hasViews(server);
  $: managers = managerOptions(server);
  $: if (server && (!manager || !managers.includes(manager))) manager = managers[0] ?? '';
  $: acpAgents = server?.acpAgents ?? [];
  // Only acp on the server: the agents are the ACP ones.
  $: acpOnly = !!server && !views && manager === 'acp';
  $: if (acpOnly && !server?.acpAgents.includes(template)) template = server?.acpAgents[0] ?? '';
  $: if (!acpOnly && !visibleTemplates.some(t => t.id === template)) template = 'claude';
  // The remembered view for this agent on this server is loaded when either changes; a click then overrides it.
  $: wanted = loadView(viewStore(), serverId, template);
  $: control = viewControl(server, template);
  $: view = resolveView(server, template, wanted);
  $: backend = server ? backendFor(server, manager, template, wanted) : '';
  // An ACP workspace (T0): the server starts the agent itself, in a directory, from the owner's list of agents.
  $: acp = !!server && backend === 'acp';
  function pick(v: View) { wanted = v; saveView(viewStore(), serverId, template, v); }
  $: dirs = recentDirs(windows, `${serverId}/`);
  $: taken = workspaces.filter(s => s.serverId === serverId).map(s => s.id);
  $: if (!nameTouched) name = suggestName(dir, template, taken);
  // Only tmux can start in a directory or run an agent; the sheet says so for other managers instead of ignoring them.
  $: tmux = !server || server.backends.length === 0 || backend === 'tmux';
  $: ok = !!server && validName(name) && validDir(dir) && !taken.includes(name) && (!acp || (!!template && dir !== ''));
  $: problem = acp && !template ? 'This server allows no agent workspaces yet.' : acp && dir === '' ? 'Choose the directory the agent works in, like /home/you/project.' : !validDir(dir) ? 'The directory must be an absolute path, like /home/you/project.' : taken.includes(name) ? 'That name is taken.' : !validName(name) ? 'Use letters, digits, - and _ (up to 64).' : '';

  // Feature detection for native folder picker
  $: canShowDirectoryPicker = typeof window !== 'undefined' && 'showDirectoryPicker' in window;
  $: canUseWebkitDirectory = typeof document !== 'undefined' && 'webkitdirectory' in document.createElement('input');

  function selectTemplate(id: string) {
    template = id;
    if (server) saveRecentAgent(serverId, id);
  }

  function selectDir(path: string) {
    dir = path;
    showDirPicker = false;
    dirInput?.focus();
  }

  function openNativeDirPicker() {
    if (canShowDirectoryPicker) {
      // Modern File System Access API (Chrome 86+, Edge 86+)
      (async () => {
        try {
          const handle = await (window as any).showDirectoryPicker({ mode: 'read' });
          // We can't get the full path from the handle, but we can use the name
          // For now, fall back to webkitdirectory which gives us the path
          openWebkitDirPicker();
        } catch {
          // User cancelled or error
        }
      })();
    } else if (canUseWebkitDirectory) {
      openWebkitDirPicker();
    }
  }

  function openWebkitDirPicker() {
    fileInput?.click();
  }

  function handleWebkitDirPick(event: Event) {
    const input = event.target as HTMLInputElement;
    const files = input.files;
    if (files && files.length > 0) {
      // Get the common parent directory from the first file's webkitRelativePath
      const firstFile = files[0];
      const relativePath = (firstFile as any).webkitRelativePath;
      if (relativePath) {
        const dirPath = '/' + relativePath.split('/').slice(0, -1).join('/');
        if (dirPath && dirPath !== '/') {
          dir = dirPath;
        }
      }
    }
    // Reset the input so the same directory can be picked again
    input.value = '';
  }

  function create() {
    if (!ok || !server) return;
    oncreate({ serverId, backend: server.backends.length > 1 ? backend : undefined, name, ...((tmux || acp) && dir ? { dir } : {}), ...((tmux && template !== 'shell') || acp ? { template } : {}) });
    onclose();
  }
</script>

<dialog bind:this={dialog} aria-label="New workspace" on:cancel|preventDefault={onclose} on:click={(e) => { if (e.target === dialog) onclose(); }}>
  <form on:submit|preventDefault={create}>
    <h2>{locked && server ? `New workspace on ${server.server.server_name || 'Server'}` : 'New workspace'}</h2>
    {#if servers.length > 1 && !locked}
      <label>Server <NativeSelect bind:value={serverId}>{#each servers as s}<option value={s.id}>{s.server.server_name || 'Server'}</option>{/each}</NativeSelect></label>
    {/if}
    {#if managers.length > 1}
      <label>Manager <NativeSelect bind:value={manager}>{#each managers as b}<option value={b}>{b === 'acp' ? 'Agent workspace (ACP)' : b}</option>{/each}</NativeSelect></label>
    {/if}
    {#if tmux || acp}
      <div class="templates" role="radiogroup" aria-label="Start with">
        {#each visibleTemplates as t}
          <button type="button" role="radio" aria-checked={template === t.id} class:on={template === t.id} on:click={() => selectTemplate(t.id)}>
            <AgentIcon command={t.id === 'shell' ? 'bash' : t.id} agent={t.id === 'shell' ? undefined : t.id} size={24} /><span>{t.label}</span>
          </button>
        {/each}
        {#if hasMoreAgents}
          <button type="button" class="show-more" on:click={() => showAllAgents = !showAllAgents}>
            {showAllAgents ? 'Show less' : `Show all ${orderedTemplates.length} agents`}
          </button>
        {/if}
      </div>
      {#if control === 'choose' && manager === 'tmux'}
        <div class="view" role="radiogroup" aria-label="View" data-testid="view">
          <button type="button" role="radio" aria-checked={view === 'terminal'} class:on={view === 'terminal'} data-testid="view-terminal" on:click={() => pick('terminal')}>Terminal <small>agent's own TUI</small></button>
          <button type="button" role="radio" aria-checked={view === 'acp'} class:on={view === 'acp'} data-testid="view-acp" on:click={() => pick('acp')}>Agent view <small>ACP</small></button>
        </div>
      {/if}
      <label class="stack">
        <span>Directory</span>
        <div class="dir-field">
          {#if showDirPicker}
            <input bind:this={dirInput} bind:value={dir} placeholder={acp ? '/home/you/project' : '/home/you/project (optional)'} autocapitalize="off" autocomplete="off" spellcheck="false" inputmode="url" />
            <button type="button" class="secondary" on:click={() => showDirPicker = false}>← Back</button>
          {:else}
            {#if dirs.length}
              <NativeSelect bind:value={dir} aria-label="Recent directories">
                <option value="">— Pick a recent directory —</option>
                {#each dirs as d}
                  <option value={d}>{d}</option>
                {/each}
                <option value="__other__">Other…</option>
              </NativeSelect>
            {:else}
              <input bind:this={dirInput} bind:value={dir} placeholder={acp ? '/home/you/project' : '/home/you/project (optional)'} autocapitalize="off" autocomplete="off" spellcheck="false" inputmode="url" />
            {/if}
            {#if (canShowDirectoryPicker || canUseWebkitDirectory) && (tmux || acp)}
              <input bind:this={fileInput} type="file" webkitdirectory multiple style="display: none" on:change={handleWebkitDirPick} />
              <button type="button" class="secondary browse-btn" on:click={openNativeDirPicker}>
                <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" style="vertical-align: -2px; margin-right: 6px;"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" fill="currentColor"/></svg>
                Browse…
              </button>
            {/if}
          {/if}
        </div>
      </label>
      {#if dirs.length && !showDirPicker}
        <p class="dir-hint">Or pick “Other…” to type a custom path</p>
      {/if}
    {:else}
      <p class="note">{backend} starts workspaces in its own default directory.</p>
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
  .templates { display: grid; grid-template-columns: repeat(auto-fill, minmax(140px, 1fr)); gap: 8px; }
  .templates button { display: flex; align-items: center; gap: 8px; min-height: 48px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .templates button.on, .view button.on { border-color: var(--accent); }
  .templates .show-more { grid-column: 1 / -1; justify-content: center; background: transparent; border-color: var(--border); color: var(--fg-muted); font-size: 0.85rem; }
  .templates .show-more:hover { color: var(--fg); border-color: var(--fg-muted); }
  .view { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .view button { display: flex; flex-direction: column; align-items: flex-start; justify-content: center; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .view small { color: var(--fg-muted); }
  label { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 0.85rem; }
  label.stack { flex-direction: column; align-items: stretch; }
  input, button { min-height: 44px; min-width: 44px; }
  input { width: 100%; }
  .dir-field { display: flex; gap: 8px; align-items: stretch; width: 100%; }
  .dir-field > * { flex: 1; }
  .dir-field input { flex: 1 1 auto; }
  .dir-field button { flex: 0 0 auto; white-space: nowrap; }
  .browse-btn { flex: 0 0 auto; }
  .dir-hint { margin: 0; color: var(--fg-muted); font-size: 0.75rem; }
  .dirs { display: flex; flex-wrap: wrap; gap: 6px; }
  .dirs button { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .note { color: var(--fg-muted); font-size: 0.85rem; }
  .problem { color: var(--danger); font-size: 0.8rem; }

  @media (max-width: 480px) {
    .templates { grid-template-columns: 1fr 1fr; }
    .view { grid-template-columns: 1fr; }
    .dir-field { flex-direction: column; }
    .dir-field > * { width: 100%; }
  }
</style>
