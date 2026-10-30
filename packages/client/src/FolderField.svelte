<script lang="ts">
  import PickerSheet from './lib/ui/sheet/PickerSheet.svelte';
  import { displayPath } from './lib/dirs';

  /** The folder as a readonly line with recent chips; the path is only typed in the picker `onbrowse` opens. */
  let {
    value,
    home = '',
    recents,
    onchange,
    onbrowse,
    required = false,
    moreOpen = $bindable(false),
  }: {
    value: string;
    /** The server user's home, once known from a listing; paths under it show as `~/…`. */
    home?: string;
    /** Most recent first, up to 10. */
    recents: string[];
    onchange: (path: string) => void;
    onbrowse: () => void;
    required?: boolean;
    moreOpen?: boolean;
  } = $props();

  const CHIPS = 3;
  const others = $derived(recents.filter(r => r !== value));
  const chips = $derived(others.slice(0, CHIPS));
  const shortOf = (p: string) => {
    const parts = p.split('/').filter(Boolean);
    return parts.length <= 2 ? p : parts.slice(-2).join('/');
  };
  const options = $derived(recents.map(p => ({ value: p, label: shortOf(p), description: displayPath(p, home, 0) })));
</script>

<div class="flex flex-col gap-2" data-testid="folder-field">
  {#if recents.length}
    <div class="flex flex-wrap gap-2" data-testid="dir-chips">
      {#each chips as r (r)}
        <button type="button" class="chip" data-testid="dir-chip" title={r} onclick={() => onchange(r)}>{shortOf(r)}</button>
      {/each}
      {#if others.length > chips.length}
        <button type="button" class="chip more" data-testid="dirs-more" aria-haspopup="dialog" onclick={() => (moreOpen = true)}>More</button>
      {/if}
    </div>
  {/if}
  <div class="flex items-stretch gap-2">
    <button
      type="button"
      class="field"
      class:empty={!value}
      aria-label="Folder"
      title={value}
      data-testid="folder-value"
      onclick={onbrowse}
    >
      {#if value}
        <span>{displayPath(value, home, 0)}</span>
      {:else}
        <span>Choose a folder{required ? '' : ' (optional)'}</span>
      {/if}
    </button>
    <button type="button" class="change" data-testid="folder-change" onclick={onbrowse}>{value ? 'Change' : 'Choose'}</button>
  </div>
</div>

<PickerSheet
  bind:open={moreOpen}
  title="Recent folders"
  {options}
  selected={value}
  onselect={onchange}
  filterable
  filterPlaceholder="Filter folders…"
  emptyMessage="No folders match"
  data-testid="dirs-layer"
/>

<style>
  .chip { min-height: 44px; max-width: 100%; padding: 0 14px; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 22px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chip.more { background: transparent; color: var(--fg-muted); }
  .field { flex: 1; min-width: 0; min-height: 48px; padding: 8px 12px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 8px; font-family: ui-monospace, monospace; font-size: 0.9rem; }
  .field.empty { color: var(--fg-muted); font-family: inherit; }
  .field span { display: block; min-width: 0; white-space: normal; overflow-wrap: anywhere; word-break: break-word; }
  .change { flex: none; min-height: 48px; padding: 0 16px; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 8px; }
</style>
