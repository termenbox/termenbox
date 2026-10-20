<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../ui/utils';
  /** The input dock: one surface, three rows at most (decision row, line input, key row). Taps on its buttons
   *  never move focus (K2): the line input is the only text focus. Send on an empty line is Enter.
   *  `multiline` makes the field grow up to five lines (the ACP prompt); Enter then sends and Shift+Enter breaks the line.
   *  `trailing` sits before the send key (a Stop key that appears while an agent works); the send key never moves.
   *  `sendDisabled` stops sending but not typing; `onsend` returning `false` keeps the line; `fieldProps` are extra
   *  attributes of the field (the ARIA of a completion list). */
  let { value = $bindable(''), placeholder = 'Send a line', label = 'Send a line', disabled = false, sendDisabled = false, decision, keys, trailing, onsend, onmic, multiline = false, input = $bindable(null), onkeydown, fieldProps = {}, class: className = '' }:
    { value?: string; placeholder?: string; label?: string; disabled?: boolean; sendDisabled?: boolean; decision?: Snippet; keys?: Snippet; trailing?: Snippet; onsend?: (line: string) => void | boolean; onmic?: () => void;
      multiline?: boolean; input?: HTMLInputElement | HTMLTextAreaElement | null; onkeydown?: (e: KeyboardEvent) => void; fieldProps?: Record<string, string | boolean | undefined>; class?: string } = $props();
  const off = $derived(disabled || sendDisabled);
  const send = () => { if (onsend?.(value) !== false) value = ''; };
  const keep = (e: PointerEvent) => e.preventDefault();
  const MAX_LINES = 5;
  /** The field is as tall as its text, to five lines: 24 px lines plus 12 px above and below. */
  function grow() {
    const el = input;
    if (!multiline || !el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, MAX_LINES * 24 + 24)}px`;
  }
  $effect(() => { value; grow(); });
  const keydown = (e: KeyboardEvent) => {
    onkeydown?.(e);
    if (e.defaultPrevented || !multiline || e.key !== 'Enter' || e.shiftKey || e.isComposing) return;
    e.preventDefault();
    if (!off) send();
  };
  const field = 'min-w-0 flex-1 border-0 bg-transparent p-0 text-input text-fg outline-none placeholder:text-muted';
</script>

<div class={cn('flex flex-col gap-2 border-t border-line bg-bg px-3 pt-2 pb-[calc(--spacing(3)+env(safe-area-inset-bottom))]', className)}>
  {#if decision}{@render decision()}{/if}
  <form class={cn('flex gap-1 rounded-card bg-raised py-1 pr-1 pl-4', multiline ? 'items-end' : 'items-center')} onsubmit={(e) => { e.preventDefault(); send(); }}>
    {#if multiline}
      <textarea bind:this={input} bind:value {placeholder} aria-label={label} {disabled} rows="1" autocapitalize="sentences" autocomplete="off" enterkeyhint="send" data-composer onkeydown={keydown} oninput={grow} {...fieldProps}
        class={cn(field, 'max-h-[144px] min-h-12 resize-none py-3 leading-6')}></textarea>
    {:else}
      <input bind:this={input} bind:value {placeholder} aria-label={label} {disabled} autocapitalize="off" autocomplete="off" enterkeyhint="send" data-composer {onkeydown} {...fieldProps}
        class={cn(field, 'h-12')} />
    {/if}
    {#if onmic}<button type="button" aria-label="Dictate" onclick={onmic} onpointerdown={keep} class="grid size-11 place-items-center rounded-control bg-transparent p-0 text-base text-soft">🎤</button>{/if}
    {#if trailing}{@render trailing()}{/if}
    <button type="submit" disabled={off} aria-label={value ? 'Send' : 'Enter'} onpointerdown={keep} class="grid size-11 place-items-center rounded-control bg-accent p-0 text-base text-on-accent">↵</button>
  </form>
  {#if keys}{@render keys()}{/if}
</div>
