<script lang="ts">
  import { onMount } from 'svelte';
  import { NativeSelect } from './lib/ui/native-select';
  import AgentIcon from './AgentIcon.svelte';
  import { recentDirs, suggestName, TEMPLATES, validDir, validLabel } from './lib/newworkspace';
  import type { HubServer, HubWorkspace } from './lib/hub';
  import type { WindowMap } from './lib/windows';
  /**
   * A workspace and its first tab (decision 0147): server, name, directory and the first tab's template. The server
   * always creates it on tmux; Manager and View are gone from this sheet, and an agent view is not created here.
   */
  let { servers, initialServerId = '', locked = false, workspaces, windows, oncreate, onclose }: {
    servers: HubServer[];
    initialServerId?: string;
    locked?: boolean;
    workspaces: HubWorkspace[];
    windows: WindowMap;
    oncreate: (req: { serverId: string; backend?: string; dir?: string; template?: string; name: string }) => void;
    onclose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let serverId = $state(servers.find((s) => s.id === initialServerId)?.id ?? servers[0]?.id ?? '');
  let template = $state('claude');
  let dir = $state('');
  let name = $state('');
  let touched = $state(false);
  onMount(() => dialog.showModal());

  const server = $derived(servers.find((s) => s.id === serverId));
  const hasTmux = $derived(!server || server.backends.length === 0 || server.backends.includes('tmux'));
  const dirs = $derived(recentDirs(windows, `${serverId}/`));
  const taken = $derived(workspaces.filter((s) => s.serverId === serverId).map((s) => s.name));
  const suggested = $derived(suggestName(dir, template, taken));
  $effect(() => { if (!servers.some((s) => s.id === serverId)) serverId = servers[0]?.id ?? ''; });
  $effect(() => { if (!touched) name = suggested; });
  const problem = $derived(
    !hasTmux ? 'This server runs no terminals, so it cannot start a workspace here.' :
    !validDir(dir) ? 'The directory must be an absolute path, like /home/you/project.' :
    taken.includes(name.trim()) ? 'That name is taken on this server.' :
    !validLabel(name) ? 'Use one line of at most 64 characters, without |.' : '',
  );
  const ok = $derived(!!server && hasTmux && validDir(dir) && validLabel(name) && !taken.includes(name.trim()));
  const pick = (id: string) => { template = id; if (!touched) name = suggestName(dir, id, taken); };

  function create() {
    if (!ok || !server) return;
    oncreate({
      serverId,
      backend: hasTmux ? 'tmux' : undefined,
      dir: dir.trim() || undefined,
      template,
      name: name.trim(),
    });
    onclose();
  }
</script>

<dialog bind:this={dialog} aria-label="New workspace" oncancel={(e) => { e.preventDefault(); onclose(); }} onclick={(e) => { if (e.target === dialog) onclose(); }}>
  <form onsubmit={(e) => { e.preventDefault(); create(); }}>
    <h2>{locked && server ? `New workspace on ${server.server.server_name || 'Server'}` : 'New workspace'}</h2>
    {#if servers.length > 1 && !locked}
      <label>Server <NativeSelect bind:value={serverId}>{#each servers as s}<option value={s.id}>{s.server.server_name || 'Server'}</option>{/each}</NativeSelect></label>
    {/if}
    <div class="templates" role="radiogroup" aria-label="First tab">
      {#each TEMPLATES as t}
        <button type="button" role="radio" aria-checked={template === t.id} class:on={template === t.id} onclick={() => pick(t.id)}>
          <AgentIcon command={t.id === 'shell' ? 'bash' : t.id} agent={t.id === 'shell' ? undefined : t.id} size={24} /><span>{t.label}</span>
        </button>
      {/each}
    </div>
    <label class="stack">Directory <input bind:value={dir} placeholder="/home/you/project (optional)" autocapitalize="off" autocomplete="off" spellcheck="false" inputmode="url" /></label>
    {#if dirs.length}
      <div class="dirs" aria-label="Directories in use">{#each dirs as d}<button type="button" class="secondary" onclick={() => (dir = d)}>{d}</button>{/each}</div>
    {/if}
    <label class="stack">Name <input bind:value={name} oninput={() => (touched = true)} maxlength="64" autocapitalize="off" autocomplete="off" spellcheck="false" /></label>
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
    <div class="actions"><button type="button" class="secondary" onclick={onclose}>Cancel</button><button type="submit" disabled={!ok}>Create</button></div>
  </form>
</dialog>

<style>
  dialog { margin: auto auto 0; width: min(100%, 460px); max-height: calc(var(--app-height, 100dvh) - 16px); overflow-y: auto; padding: 0; border: 1px solid var(--border); border-radius: 16px 16px 0 0; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0008; }
  form { padding: 16px; display: flex; flex-direction: column; gap: 10px; }
  h2 { margin: 0; font-size: 1.05rem; }
  .templates { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 6px; }
  .templates button { display: flex; align-items: center; gap: 8px; min-height: 48px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .templates button.on { border-color: var(--accent); }
  label { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 0.85rem; }
  label.stack { flex-direction: column; align-items: stretch; }
  input, button { min-height: 44px; min-width: 44px; }
  input { width: 100%; font-size: 16px; }
  .dirs { display: flex; flex-wrap: wrap; gap: 6px; }
  .dirs button { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .problem { color: var(--danger); font-size: 0.8rem; margin: 0; }
</style>
