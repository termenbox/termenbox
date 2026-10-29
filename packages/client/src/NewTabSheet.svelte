<script lang="ts">
  import { onMount } from 'svelte';
  import AgentIcon from './AgentIcon.svelte';
  import { TEMPLATES } from './lib/newworkspace';
  import { relativeDirProblem, tabKindLabel, uniqueTabLabel } from './lib/workspaceTabs';
  import type { HubWorkspace } from './lib/hub';
  /** A new tab in an open workspace: its template, its name and a directory inside the workspace. */
  let { workspace, oncreate, onclose }: {
    workspace: HubWorkspace;
    oncreate: (req: { template: string; label: string; dir: string }) => void;
    onclose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let template = $state('shell');
  let label = $state(uniqueTabLabel(tabKindLabel('shell'), workspace.tabs));
  let dir = $state('');
  let touched = $state(false);
  const suggested = $derived(uniqueTabLabel(tabKindLabel(template), workspace.tabs));
  $effect(() => { if (!touched) label = suggested; });
  const problem = $derived(relativeDirProblem(dir));
  const ok = $derived(!!workspace && !!label.trim() && !problem);
  onMount(() => dialog.showModal());
  const pick = (id: string) => { template = id; if (!touched) label = uniqueTabLabel(tabKindLabel(id), workspace.tabs); };
  function create() {
    if (!ok) return;
    oncreate({ template, label: label.trim(), dir: dir.trim() });
    onclose();
  }
</script>

<dialog bind:this={dialog} aria-label="New tab" oncancel={(e) => { e.preventDefault(); onclose(); }} onclick={(e) => { if (e.target === dialog) onclose(); }}>
  <form onsubmit={(e) => { e.preventDefault(); create(); }}>
    <h2>New tab in {workspace.name}</h2>
    <div class="templates" role="radiogroup" aria-label="Start with">
      {#each TEMPLATES as t}
        <button type="button" role="radio" aria-checked={template === t.id} class:on={template === t.id} onclick={() => pick(t.id)}>
          <AgentIcon command={t.id === 'shell' ? 'bash' : t.id} agent={t.id === 'shell' ? undefined : t.id} size={24} /><span>{t.label}</span>
        </button>
      {/each}
    </div>
    <label class="stack">Name
      <input bind:value={label} oninput={() => (touched = true)} maxlength="64" autocomplete="off" autocapitalize="off" spellcheck="false" enterkeyhint="next" />
    </label>
    <label class="stack">Directory inside {workspace.dir || 'the workspace'}
      <input bind:value={dir} placeholder="optional, like packages/api" autocomplete="off" autocapitalize="off" spellcheck="false" inputmode="url" />
    </label>
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
    <div class="actions"><button type="button" class="secondary" onclick={onclose}>Cancel</button><button type="submit" disabled={!ok}>Create</button></div>
  </form>
</dialog>

<style>
  dialog { margin: auto auto 0; width: min(100%, 460px); max-height: calc(var(--app-height, 100dvh) - 16px); overflow-y: auto; padding: 0; border: 1px solid var(--border); border-radius: 16px 16px 0 0; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0008; }
  form { padding: 16px; display: flex; flex-direction: column; gap: 10px; }
  h2 { margin: 0; font-size: 1.05rem; overflow-wrap: anywhere; }
  .templates { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 6px; }
  .templates button { display: flex; align-items: center; gap: 8px; min-height: 48px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .templates button.on { border-color: var(--accent); }
  label.stack { display: flex; flex-direction: column; gap: 4px; font-size: 0.85rem; }
  input { width: 100%; min-height: 44px; font-size: 16px; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .actions button { min-height: 44px; min-width: 44px; }
  .problem { color: var(--danger); font-size: 0.8rem; margin: 0; }
</style>
