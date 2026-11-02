<script lang="ts">
  import { backdropClose } from './lib/backdropClose';
  import { onMount } from 'svelte';
  import type { UploadItem, UploadState } from './lib/uploadQueue';
  import type { UploadCollision } from './lib/protocol';
  /** Copies files from this device into the working folder of one tab (decision 0194). Nothing starts before Upload is pressed. */
  let { tabName, items, started, onpick, onconfirm, oncollision, onretry, oncancel, onclose }: {
    tabName: string;
    items: UploadItem[];
    /** The user pressed Upload: the server may now be asked. */
    started: boolean;
    onpick: (files: File[]) => void;
    onconfirm: () => void;
    oncollision: (id: number, collision: UploadCollision, rest: boolean) => void;
    onretry: (id: number) => void;
    oncancel: () => void;
    onclose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let picker: HTMLInputElement;
  let rest = $state(false);
  onMount(() => dialog.showModal());
  const folder = $derived(items.find(i => i.dir)?.dir);
  const busy = $derived(items.some(i => i.state === 'uploading' || i.state === 'paused' || i.state === 'queued'));
  const text: Record<UploadState, string> = {
    confirm: 'Waiting for you', queued: 'Queued', uploading: 'Uploading', paused: 'Paused: reconnecting',
    denied: 'Not uploaded', failed: 'Failed', cancelled: 'Cancelled', done: 'Done',
  };
  const size = (n: number) => n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KiB` : `${(n / 1048576).toFixed(1)} MiB`;
  const pct = (i: UploadItem) => i.size ? Math.min(100, Math.floor((i.received / i.size) * 100)) : 0;
  const choices: [UploadCollision, string][] = [['keep_both', 'Keep both'], ['replace', 'Replace'], ['skip', 'Skip']];
</script>

<dialog bind:this={dialog} aria-label="Upload files" oncancel={(e) => { e.preventDefault(); busy ? oncancel() : onclose(); }} use:backdropClose={() => { if (!busy) onclose(); }}>
  <h2>Upload to {tabName}</h2>
  <p class="where" data-testid="upload-folder">
    {#if folder}Folder: <code>{folder}</code>{:else}The server picks the folder this tab is in when you upload.{/if}
  </p>
  {#if items.length === 0}
    <p>Choose files, drop them here or paste an image.</p>
  {:else}
    <ul>
      {#each items as item (item.id)}
        <li data-state={item.state}>
          <div class="row"><strong>{item.name}</strong> <span>{size(item.size)}</span></div>
          {#if item.state === 'confirm' || item.state === 'queued'}
            <div class="choice" role="radiogroup" aria-label={`If ${item.name} exists`}>
              {#each choices as [value, label]}
                <button type="button" role="radio" aria-checked={item.collision === value} class:on={item.collision === value} disabled={started} onclick={() => oncollision(item.id, value, rest)}>{label}</button>
              {/each}
            </div>
          {/if}
          <div class="status" role="status">
            {text[item.state]}{#if item.state === 'uploading'}: {pct(item)}%{/if}
            {#if item.reason} — {item.reason}{/if}
            {#if item.state === 'done' && item.path}: <code>{item.path}</code>{/if}
          </div>
          {#if item.state === 'uploading' || item.state === 'paused'}<progress max="100" value={pct(item)} aria-label={`${item.name} progress`}></progress>{/if}
          {#if item.state === 'failed' || item.state === 'denied' || item.state === 'cancelled'}
            <button type="button" class="secondary" onclick={() => onretry(item.id)}>Retry</button>
          {/if}
        </li>
      {/each}
    </ul>
    {#if !started && items.length > 1}
      <label class="rest"><input type="checkbox" bind:checked={rest} /> Same action for the rest</label>
    {/if}
  {/if}
  <input bind:this={picker} type="file" multiple hidden onchange={() => { onpick(Array.from(picker.files ?? [])); picker.value = ''; }} />
  <div class="actions">
    {#if !started}
      <button type="button" class="secondary" onclick={() => picker.click()}>Choose files…</button>
      <button type="button" disabled={items.length === 0} onclick={onconfirm}>Upload {items.length > 1 ? `${items.length} files` : ''}</button>
      <button type="button" class="secondary" onclick={onclose}>Cancel</button>
    {:else if busy}
      <button type="button" class="secondary" onclick={() => picker.click()}>Add files…</button>
      <button type="button" class="danger" onclick={oncancel}>Cancel upload</button>
    {:else}
      <button type="button" onclick={onclose}>Close</button>
    {/if}
  </div>
</dialog>

<style>
  dialog { margin: auto auto 0; width: min(100%, 460px); max-height: calc(var(--app-height, 100dvh) - 16px); overflow-y: auto; padding: 16px; border: 1px solid var(--border); border-radius: 16px 16px 0 0; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0008; }
  h2 { margin: 0 0 6px; font-size: 1.05rem; overflow-wrap: anywhere; }
  .where, .status { font-size: 0.85rem; overflow-wrap: anywhere; }
  ul { list-style: none; margin: 8px 0; padding: 0; display: flex; flex-direction: column; gap: 10px; }
  li { border: 1px solid var(--border); border-radius: 10px; padding: 8px; display: flex; flex-direction: column; gap: 6px; }
  .row { display: flex; justify-content: space-between; gap: 8px; overflow-wrap: anywhere; }
  .choice { display: flex; gap: 6px; }
  .choice button { flex: 1; min-height: 44px; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .choice button.on { border-color: var(--accent); }
  progress { width: 100%; }
  .rest { display: flex; align-items: center; gap: 8px; min-height: 44px; }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
  .actions button { flex: 1; min-height: 44px; }
</style>
