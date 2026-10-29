<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { rememberFocus } from './lib/focus';
  /** The one rename field, for a workspace or a tab. Top-aligned so the soft keyboard never covers it. */
  let { title, field = 'Name', value, onsave, onclose }: {
    title: string;
    field?: string;
    value: string;
    onsave: (name: string) => void;
    onclose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let draft = $state(value);
  const restoreFocus = rememberFocus();
  onMount(() => dialog.showModal());
  onDestroy(restoreFocus);
  const focusSelect = (el: HTMLInputElement) => { el.focus(); el.select(); };
  function save() {
    const next = draft.trim();
    if (!next) return;
    if (next !== value) onsave(next);
    onclose();
  }
</script>

<dialog bind:this={dialog} aria-label={title} oncancel={(e) => { e.preventDefault(); onclose(); }} onclick={(e) => { if (e.target === dialog) onclose(); }}>
  <form onsubmit={(e) => { e.preventDefault(); save(); }}>
    <h2>{title}</h2>
    <label>{field}
      <input use:focusSelect bind:value={draft} maxlength="64" autocomplete="off" autocapitalize="off" spellcheck="false" enterkeyhint="done" />
    </label>
    <div class="actions">
      <button type="button" class="secondary" onclick={onclose}>Cancel</button>
      <button type="submit" disabled={!draft.trim()}>Save</button>
    </div>
  </form>
</dialog>

<style>
  dialog { margin: 16px auto auto; width: min(100% - 32px, 380px); padding: 0; border: 1px solid var(--border); border-radius: 12px; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0008; }
  form { display: flex; flex-direction: column; gap: 10px; padding: 16px; }
  h2 { margin: 0; font-size: 1.05rem; overflow-wrap: anywhere; }
  label { display: flex; flex-direction: column; gap: 4px; font-size: 0.85rem; }
  input { min-height: 44px; width: 100%; padding: 0 12px; font-size: 16px; color: var(--fg); background: var(--bg); border: 1px solid var(--border); border-radius: 8px; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .actions button { min-height: 44px; min-width: 44px; }
</style>
