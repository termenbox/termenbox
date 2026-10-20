<script lang="ts">
  import { Dialog } from '../lib/ui';
  import { SHORTCUTS, PALETTE_KEYS, SPLITTER_KEYS } from '../lib/palette';
  /** What the app intercepts, where, and how to turn it off. Nothing else is intercepted. */
  let { open = $bindable(true), mac, enabled, onclose }: { open?: boolean; mac: boolean; enabled: boolean; onclose?: () => void } = $props();
</script>

<Dialog bind:open title="Keyboard shortcuts" {onclose}>
  <div data-shell="shortcut-help" class="flex flex-col gap-4 text-base">
    <p class="text-soft">{enabled ? 'These are the only keys the app takes from the page. Every other key, including Ctrl+C, Tab and Escape, goes to the terminal, and only Ctrl or Cmd+K works while you are typing into a field or the terminal.' : 'Shortcuts are off, so every key goes to the terminal. Turn them on in Settings.'}</p>
    <dl class="flex flex-col gap-3">
      {#each SHORTCUTS as s (s.id)}
        <div class="flex items-baseline justify-between gap-4"><dt><b>{s.label}</b><span class="block text-sm text-soft">{s.scope}</span></dt><dd><kbd class="rounded-control bg-raised px-2 py-1 font-sans text-sm">{mac ? s.mac : s.other}</kbd></dd></div>
      {/each}
    </dl>
    <h3 class="text-xs font-semibold tracking-wider text-soft uppercase">In the palette</h3>
    <dl class="flex flex-col gap-2">
      {#each PALETTE_KEYS as k (k.keys)}
        <div class="flex items-baseline justify-between gap-4"><dt>{k.label}</dt><dd><kbd class="rounded-control bg-raised px-2 py-1 font-sans text-sm">{k.keys}</kbd></dd></div>
      {/each}
    </dl>
    <h3 class="text-xs font-semibold tracking-wider text-soft uppercase">On a divider (Tab to it)</h3>
    <dl class="flex flex-col gap-2">
      {#each SPLITTER_KEYS as k (k.keys)}
        <div class="flex items-baseline justify-between gap-4"><dt>{k.label}</dt><dd><kbd class="rounded-control bg-raised px-2 py-1 font-sans text-sm">{k.keys}</kbd></dd></div>
      {/each}
    </dl>
    <p class="text-sm text-soft">{mac ? 'Cmd+K is never sent to a terminal, so nothing is lost on a Mac.' : 'On Windows and Linux Ctrl+K is also the shell\'s "delete to end of line"; while shortcuts are on, the app takes it. Turn them off in Settings to get it back.'}</p>
  </div>
</Dialog>
