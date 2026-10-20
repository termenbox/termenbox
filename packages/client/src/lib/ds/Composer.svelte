<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../ui/utils';
  /** The input dock: one surface, three rows at most (decision row, line input, key row). Taps on its buttons
   *  never move focus (K2): the line input is the only text focus. Send on an empty line is Enter. */
  let { value = $bindable(''), placeholder = 'Send a line', label = 'Send a line', disabled = false, decision, keys, onsend, onmic, class: className = '' }:
    { value?: string; placeholder?: string; label?: string; disabled?: boolean; decision?: Snippet; keys?: Snippet; onsend?: (line: string) => void; onmic?: () => void; class?: string } = $props();
  const send = () => { onsend?.(value); value = ''; };
  const keep = (e: PointerEvent) => e.preventDefault();
</script>

<div class={cn('flex flex-col gap-2 border-t border-line bg-bg px-3 pt-2 pb-[calc(--spacing(3)+env(safe-area-inset-bottom))]', className)}>
  {#if decision}{@render decision()}{/if}
  <form class="flex items-center gap-1 rounded-card bg-raised py-1 pr-1 pl-4" onsubmit={(e) => { e.preventDefault(); send(); }}>
    <input bind:value {placeholder} aria-label={label} {disabled} autocapitalize="off" autocomplete="off" enterkeyhint="send" data-composer
      class="h-12 min-w-0 flex-1 border-0 bg-transparent p-0 text-input text-fg outline-none placeholder:text-muted" />
    {#if onmic}<button type="button" aria-label="Dictate" onclick={onmic} onpointerdown={keep} class="grid size-11 place-items-center rounded-control bg-transparent p-0 text-base text-soft">🎤</button>{/if}
    <button type="submit" {disabled} aria-label={value ? 'Send' : 'Enter'} onpointerdown={keep} class="grid size-11 place-items-center rounded-control bg-accent p-0 text-base text-on-accent">↵</button>
  </form>
  {#if keys}{@render keys()}{/if}
</div>
