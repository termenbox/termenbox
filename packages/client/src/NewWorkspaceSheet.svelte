<script lang="ts">
  import { onMount } from 'svelte';
  import { NativeSelect } from './lib/ui/native-select';
  import AgentRow from './AgentRow.svelte';
  import FolderField from './FolderField.svelte';
  import FolderPicker from './FolderPicker.svelte';
  import { displayPath } from './lib/dirs';
  import type { DirReply } from './lib/dirClient';
  import {
    defaultAgent,
    getHome,
    getLastServer,
    getRecentAgents,
    getRecentDirs,
    getSupportedTemplates,
    mergeRecentDirs,
    saveHome,
    saveLastServer,
    saveRecentAgent,
    saveRecentDir,
    suggestName,
    TEMPLATE_BY_ID,
    TEMPLATES,
    validDir,
    validLabel,
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
  $: supportedTemplates = server ? getSupportedTemplates(server) : TEMPLATES;
  $: recentAgents = server ? getRecentAgents(serverId) : [];
  $: if (!supportedTemplates.some(t => t.id === template)) template = defaultAgent(supportedTemplates, recentAgents);
  $: recentFolders = mergeRecentDirs(getRecentDirs(serverId), windows, `${serverId}/`);
  $: taken = workspaces.filter(s => s.serverId === serverId).flatMap(s => [s.id, s.name]);
  $: if (!nameTouched) name = suggestName(dir, template, taken);
  // Every new workspace is a tmux workspace with its first tab; ACP workspaces are not created here (decision 0149).
  $: tmux = !server || server.backends.length === 0 || server.backends.includes('tmux');
  $: ok = !!server && tmux && validLabel(name) && validDir(dir) && !taken.includes(name.trim());
  $: problem = !server ? 'Choose a server.' : !tmux ? 'This server runs no terminals, so it cannot start a workspace here.' : !validDir(dir) ? 'The directory must be an absolute path, like /home/you/project.' : taken.includes(name.trim()) ? 'That name is taken.' : !validLabel(name) ? 'Use one line of at most 64 characters, without |.' : '';
  $: home = homes[serverId] ?? getHome(serverId);
  $: agentLabel = template === 'shell' ? 'Shell' : TEMPLATE_BY_ID[template]?.label ?? template;
  $: summary = `${agentLabel} ${dir ? `in ${displayPath(dir, home, 36)}` : 'in the default folder'}`;

  // The picker learns the home folder from the first listing, which the field then uses to show `~/…`.
  const list = (req: { path?: string; prefix?: string; hidden?: boolean }) => {
    const id = serverId;
    return listDir(id, req).then(r => { if (r.ok) { homes = { ...homes, [id]: r.home }; saveHome(id, r.home); } return r; });
  };

  function create() {
    if (!ok || !server) return;
    saveRecentAgent(serverId, template);
    if (dir) saveRecentDir(serverId, dir);
    saveLastServer(serverId);
    oncreate({
      serverId,
      backend: server.backends.length > 1 ? 'tmux' : undefined,
      name: name.trim(),
      ...(dir ? { dir } : {}),
      template,
    });
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
        <fieldset>
          <legend>First tab</legend>
          <AgentRow templates={supportedTemplates} recent={recentAgents} selected={template} onselect={(id) => (template = id)} bind:moreOpen={agentsOpen} />
        </fieldset>

        <fieldset>
          <legend>Folder</legend>
          <FolderField value={dir} {home} recents={recentFolders} required={false} onchange={(p) => (dir = p)} onbrowse={() => (pickerOpen = true)} bind:moreOpen={dirsOpen} />
        </fieldset>
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

  label { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 0.85rem; }
  label.stack { flex-direction: column; align-items: stretch; }
  input, button { min-height: 44px; min-width: 44px; }
  input { width: 100%; }

  .summary { margin: 0; font-size: 0.85rem; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .problem { margin: 0; color: var(--danger); font-size: 0.8rem; }

</style>
