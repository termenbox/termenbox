<script lang="ts">
  import { onMount } from 'svelte';
  import { NativeSelect } from './lib/ui/native-select';
  import AgentRow from './AgentRow.svelte';
  import FolderField from './FolderField.svelte';
  import FolderPicker from './FolderPicker.svelte';
  import { displayPath } from './lib/dirs';
  import type { DirReply } from './lib/dirClient';
  import {
    backendFor,
    defaultAgent,
    getHome,
    getLastServer,
    getRecentAgents,
    getRecentDirs,
    getSupportedTemplates,
    hasViews,
    loadView,
    managerOptions,
    mergeRecentDirs,
    resolveView,
    saveHome,
    saveLastServer,
    saveRecentAgent,
    saveRecentDir,
    saveView,
    suggestName,
    TEMPLATE_BY_ID,
    TEMPLATES,
    validDir,
    validLabel,
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
  /** Lists the folders of a server (`list_dir`) for the folder picker. */
  export let listDir: (serverId: string, req: { path?: string; prefix?: string; hidden?: boolean }) => Promise<DirReply> = async () => ({ ok: false, reason: 'Folder browsing is not available here.' });

  let dialog: HTMLDialogElement;
  let serverId = servers.find(s => s.id === initialServerId)?.id ?? (locked ? undefined : servers.find(s => s.id === getLastServer())?.id) ?? servers[0]?.id ?? '';
  const first = servers.find(s => s.id === serverId);
  let manager = '';
  /** The View the person picked for this agent; `view` is what applies (some agents have no choice). */
  let wanted: View = 'terminal';
  let template = first ? defaultAgent(getSupportedTemplates(first), getRecentAgents(serverId)) : 'claude';
  let dir = '';
  let name = '';
  let nameTouched = false;
  let agentsOpen = false;
  let dirsOpen = false;
  let pickerOpen = false;
  /** Each server user's home, learned from its folder listings. */
  let homes: Record<string, string> = {};
  onMount(() => dialog.showModal());

  $: anyLayer = agentsOpen || dirsOpen || pickerOpen;
  $: server = servers.find(s => s.id === serverId);
  $: recentAgents = server ? getRecentAgents(serverId) : [];
  $: managers = managerOptions(server);
  $: if (server && (!manager || !managers.includes(manager))) manager = managers[0] ?? '';
  $: acpAgents = server?.acpAgents ?? [];
  // With tmux and acp both on the server the choice is the View, made after the agent. The View shows only for an
  // agent the server lists in `acpAgents`.
  $: views = hasViews(server);
  // Only acp on the server (or picked as the manager): the agents are the ACP ones.
  $: acpOnly = !!server && !views && manager === 'acp';
  $: acpChoices = acpAgents.filter(id => TEMPLATE_BY_ID[id]).map(id => TEMPLATE_BY_ID[id]);
  $: choices = acpOnly ? acpChoices : TEMPLATES;
  $: if (acpOnly && !acpChoices.some(t => t.id === template)) template = recentAgents.find(a => acpChoices.some(t => t.id === a)) ?? acpChoices[0]?.id ?? '';
  else if (!acpOnly && !choices.some(t => t.id === template)) template = defaultAgent(choices, recentAgents);
  // The remembered view for this agent on this server is loaded when either changes; a click then overrides it.
  $: wanted = loadView(viewStore(), serverId, template);
  $: control = viewControl(server, template);
  $: view = resolveView(server, template, wanted);
  $: backend = server ? backendFor(server, manager, template, wanted) : '';
  // An ACP workspace (T0): the server starts the agent itself, in a directory, from the owner's list of agents.
  $: acp = !!server && backend === 'acp';
  function pickView(v: View) { wanted = v; saveView(viewStore(), serverId, template, v); }
  $: recentFolders = mergeRecentDirs(getRecentDirs(serverId), windows, `${serverId}/`);
  $: taken = workspaces.filter(s => s.serverId === serverId).flatMap(s => [s.id, s.name]);
  $: if (!nameTouched) name = suggestName(dir, template, taken);
  // Only tmux can start in a directory or run an agent; the sheet says so for other managers instead of ignoring them.
  $: tmux = !server || server.backends.length === 0 || backend === 'tmux';
  $: ok = !!server && validLabel(name) && validDir(dir) && !taken.includes(name.trim()) && (!acp || (!!template && dir !== ''));
  $: problem = acp && !template ? 'This server allows no agent workspaces yet.' : acp && dir === '' ? 'Choose the directory the agent works in, like /home/you/project.' : !validDir(dir) ? 'The directory must be an absolute path, like /home/you/project.' : taken.includes(name.trim()) ? 'That name is taken.' : !validLabel(name) ? 'Use one line of at most 64 characters, without |.' : '';
  $: home = homes[serverId] ?? getHome(serverId);
  $: agentLabel = template === 'shell' ? 'Shell' : TEMPLATE_BY_ID[template]?.label ?? template;
  $: summary = tmux || acp
    ? `${agentLabel} ${dir ? `in ${displayPath(dir, home, 36)}` : 'in the default folder'}`
    : `${backend} workspace`;

  // The picker learns the home folder from the first listing, which the field then uses to show `~/…`.
  const list = (req: { path?: string; prefix?: string; hidden?: boolean }) => {
    const id = serverId;
    return listDir(id, req).then(r => { if (r.ok) { homes = { ...homes, [id]: r.home }; saveHome(id, r.home); } return r; });
  };

  function create() {
    if (!ok || !server) return;
    if (tmux || acp) saveRecentAgent(serverId, template);
    if ((tmux || acp) && dir) saveRecentDir(serverId, dir);
    saveLastServer(serverId);
    oncreate({ serverId, backend: server.backends.length > 1 ? backend : undefined, name: name.trim(), ...((tmux || acp) && dir ? { dir } : {}), ...((tmux && template !== 'shell') || acp ? { template } : {}) });
    onclose();
  }
</script>

<dialog
  bind:this={dialog}
  aria-label="New workspace"
  on:cancel|preventDefault={() => { if (!anyLayer) onclose(); }}
  on:click={(e) => { if (e.target === dialog && !anyLayer) onclose(); }}
>
  <div class="wrap">
    <form on:submit|preventDefault={create}>
      <div class="scroll">
        <h2>{locked && server ? `New workspace on ${server.server.server_name || 'Server'}` : 'New workspace'}</h2>
        {#if servers.length > 1 && !locked}
          <label>Server <NativeSelect bind:value={serverId}>{#each servers as s}<option value={s.id}>{s.server.server_name || 'Server'}</option>{/each}</NativeSelect></label>
        {/if}
        {#if managers.length > 1}
          <label>Manager <NativeSelect bind:value={manager}>{#each managers as b}<option value={b}>{b === 'acp' ? 'Agent workspace (ACP)' : b}</option>{/each}</NativeSelect></label>
        {/if}
        {#if tmux || acp}
          <fieldset>
            <legend>{acp ? 'Agent' : 'First tab'}</legend>
            <AgentRow templates={choices} recent={recentAgents} selected={template} onselect={(id) => (template = id)} bind:moreOpen={agentsOpen} />
          </fieldset>

          {#if control === 'choose' && manager === 'tmux'}
            <div class="view" role="radiogroup" aria-label="View" data-testid="view">
              <button type="button" role="radio" aria-checked={view === 'terminal'} class:on={view === 'terminal'} data-testid="view-terminal" on:click={() => pickView('terminal')}>Terminal <small>agent's own TUI</small></button>
              <button type="button" role="radio" aria-checked={view === 'acp'} class:on={view === 'acp'} data-testid="view-acp" on:click={() => pickView('acp')}>Agent view <small>ACP</small></button>
            </div>
          {/if}

          <fieldset>
            <legend>Folder</legend>
            <FolderField value={dir} {home} recents={recentFolders} required={acp} onchange={(p) => (dir = p)} onbrowse={() => (pickerOpen = true)} bind:moreOpen={dirsOpen} />
          </fieldset>
        {:else}
          <p class="note">{backend} starts workspaces in its own default directory.</p>
        {/if}
        <label class="stack">Name <input bind:value={name} on:input={() => nameTouched = true} autocapitalize="off" autocomplete="off" spellcheck="false" /></label>
      </div>
      <footer>
        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
        <p class="summary" aria-live="polite" data-testid="summary" title={dir}>{summary}</p>
        <div class="actions"><button type="button" class="secondary" on:click={onclose}>Cancel</button><button type="submit" disabled={!ok}>Create</button></div>
      </footer>
    </form>
    <FolderPicker bind:open={pickerOpen} {list} value={dir} onselect={(p) => (dir = p)} />
  </div>
</dialog>

<style>
  /* Below 480 px a full-width bottom sheet, from 480 px a centred modal; one column at every width. The wrap is the
     positioned parent of the layers (they must be inside the dialog, FACT-0063); only .scroll scrolls. */
  dialog { margin: auto 0 0; width: 100%; max-width: none; max-height: var(--app-height, 100dvh); padding: 0; overflow: hidden; border: 1px solid var(--border); border-radius: 16px 16px 0 0; background: var(--bg-secondary); color: var(--fg); }
  dialog[open] { display: flex; }
  dialog::backdrop { background: #0008; }
  .wrap { position: relative; display: flex; flex: 1; min-width: 0; min-height: 0; flex-direction: column; }
  form { display: flex; flex: 1; min-height: 0; flex-direction: column; }
  .scroll { flex: 1; min-height: 0; overflow-y: auto; padding: 16px 16px 8px; display: flex; flex-direction: column; gap: 10px; }
  footer { flex: none; display: flex; flex-direction: column; gap: 6px; padding: 10px 16px calc(12px + env(safe-area-inset-bottom)); border-top: 1px solid var(--border); background: var(--bg-secondary); }
  @media (min-width: 480px) {
    dialog { margin: auto; width: min(100% - 32px, 560px); max-height: calc(var(--app-height, 100dvh) - 32px); border-radius: 16px; }
    footer { padding-bottom: 12px; }
  }

  fieldset { border: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  fieldset legend { font-size: 0.85rem; font-weight: 500; color: var(--fg-muted); margin-bottom: 6px; }
  .view { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .view button { display: flex; flex-direction: column; align-items: flex-start; justify-content: center; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .view button.on { border-color: var(--accent); }
  .view small { color: var(--fg-muted); }
  .note { color: var(--fg-muted); font-size: 0.85rem; }

  label { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 0.85rem; }
  label.stack { flex-direction: column; align-items: stretch; }
  input, button { min-height: 44px; min-width: 44px; }
  input { width: 100%; }

  .summary { margin: 0; font-size: 0.85rem; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .problem { margin: 0; color: var(--danger); font-size: 0.8rem; }

</style>
