<script lang="ts">
  import { Dialog, Command } from 'bits-ui';
  import { searchEntries, type PaletteEntry } from '../lib/palette';
  import AgentBadge from '../lib/ds/AgentBadge.svelte';

  /**
   * Ctrl/Cmd+K: jump to a workspace, server or destination, or run an action. bits-ui supplies the dialog (focus trap,
   * Escape) and the listbox keys; the search and the order are ours (lib/palette.ts). When it closes, focus goes back to
   * where it was, unless the choice moved the user elsewhere.
   */
  let { open = $bindable(false), entries, onrun, returnTo = null }: { open?: boolean; entries: PaletteEntry[]; onrun: (e: PaletteEntry) => void; returnTo?: HTMLElement | null } = $props();
  let query = $state('');
  let value = $state('');
  let ran = false;
  let inputEl = $state<HTMLInputElement | null>(null);
  const results = $derived(searchEntries(entries, query));
  $effect(() => { if (!open) { query = ''; } });
  $effect(() => { value = results[0]?.id ?? ''; });
  function pick(e: PaletteEntry) { ran = true; open = false; onrun(e); }
  // The highlighted row is read from the list the library keeps, which can lag a keystroke or two behind the text on a
  // slow machine: Enter right after typing then opened the previous top result (or nothing). The text is the truth, so
  // the highlight follows it at once, and Enter opens the highlighted row of what is shown now.
  function typed(ev: Event) { value = searchEntries(entries, (ev.currentTarget as HTMLInputElement).value)[0]?.id ?? ''; }
  function enter(ev: KeyboardEvent) {
    if (ev.key !== 'Enter' || ev.isComposing) return;
    ev.preventDefault();
    ev.stopPropagation(); // the library's own Enter would open its (possibly stale) selection a second time
    const e = results.find((r) => r.id === value) ?? results[0];
    if (e) pick(e);
  }
  function closeFocus(ev: Event) {
    ev.preventDefault();
    if (!ran && returnTo?.isConnected) returnTo.focus({ preventScroll: true });
    ran = false;
  }
  const KIND: Record<PaletteEntry['kind'], string> = { destination: 'Go to', action: 'Action', server: 'Server', workspace: 'Workspace' };
</script>

<Dialog.Root bind:open>
  <Dialog.Overlay class="absolute inset-0 z-[100] bg-black/60" />
  <Dialog.Content data-shell="palette" onOpenAutoFocus={(ev) => { ev.preventDefault(); inputEl?.focus({ preventScroll: true }); }} onCloseAutoFocus={closeFocus} class="absolute top-[10%] left-1/2 z-[101] flex max-h-[80%] w-[min(560px,calc(100%-32px))] -translate-x-1/2 flex-col overflow-hidden rounded-card border border-line bg-panel shadow-sheet">
    <Dialog.Title class="sr-only">Command palette</Dialog.Title>
    <Dialog.Description class="sr-only">Type to find a workspace, server, destination or action. Up and Down choose, Enter opens, Escape closes.</Dialog.Description>
    <Command.Root shouldFilter={false} loop label="Search" bind:value class="flex min-h-0 flex-1 flex-col">
      <Command.Input bind:ref={inputEl} bind:value={query} oninput={typed} onkeydowncapture={enter} placeholder="Jump to a workspace, server or action"
        class="h-14 w-full shrink-0 border-0 border-b border-line bg-transparent px-4 text-input text-fg outline-none placeholder:text-muted" />
      <Command.List class="min-h-0 flex-1 overflow-y-auto p-2">
        <Command.Viewport>
          {#each results as r (r.id)}
            <Command.Item value={r.id} keywords={[r.label]} onSelect={() => pick(r)} data-entry={r.id}
              class="flex h-11 cursor-pointer items-center gap-3 rounded-control px-3 text-base text-fg data-[selected]:bg-raised">
              {#if r.kind === 'workspace'}<AgentBadge agent={r.agent} state={r.state} size={20} />{/if}
              <span class="min-w-0 flex-1 truncate">{r.label}</span>
              <span class="shrink-0 text-xs text-soft">{r.kind === 'workspace' ? `${r.detail}` : KIND[r.kind]}</span>
            </Command.Item>
          {/each}
          {#if !results.length}<p class="px-3 py-6 text-center text-base text-soft" role="status">Nothing matches</p>{/if}
        </Command.Viewport>
      </Command.List>
    </Command.Root>
  </Dialog.Content>
</Dialog.Root>
