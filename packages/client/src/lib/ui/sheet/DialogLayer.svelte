<script lang="ts">
  import type { Snippet } from 'svelte';
  import { tick } from 'svelte';
  import { cn } from '../utils';
  import { FOCUSABLE, trapTarget } from './layer';

  let {
    open = $bindable(false),
    title,
    onclose,
    class: className = '',
    children,
    footer,
  }: { open?: boolean; title: string; onclose?: () => void; class?: string; children: Snippet; footer?: Snippet } = $props();

  // A descendant of the native <dialog>: showModal() paints above everything outside it (FACT-0058), so a layer
  // has to live inside the dialog's positioned wrapper rather than beside it.
  const uid = $props.id();
  const titleId = `layer-title-${uid}`;
  let root: HTMLElement | undefined = $state();
  let opener: Element | null = null;

  function close() {
    open = false;
    onclose?.();
  }

  function items(): HTMLElement[] {
    return root ? [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(el => el.offsetParent !== null || el === document.activeElement) : [];
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation(); // the dialog's own cancel must not also fire
      e.preventDefault();
      close();
    } else if (e.key === 'Tab') {
      const list = items();
      const to = trapTarget(e.shiftKey, list.indexOf(document.activeElement as HTMLElement), list.length);
      if (to !== null) {
        e.preventDefault();
        list[to].focus();
      } else if (list.length === 0) e.preventDefault();
    }
  }

  // Focus goes to the first control on open (after the body's own autofocus, if any) and back to the opener on close.
  $effect(() => {
    if (!open) return;
    opener = document.activeElement;
    tick().then(() => {
      if (!root || root.contains(document.activeElement)) return;
      const first = root.querySelector<HTMLElement>('[data-autofocus]') ?? items().find(el => el.dataset.layerBack === undefined) ?? items()[0];
      first?.focus();
    });
    return () => {
      const back = opener;
      opener = null;
      if (back instanceof HTMLElement && back.isConnected) back.focus();
    };
  });
</script>

{#if open}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="layer-scrim" data-testid="layer-scrim" onpointerdown={(e) => { if (e.target === e.currentTarget) close(); }} role="presentation">
    <div
      bind:this={root}
      role="region"
      aria-labelledby={titleId}
      class={cn('layer', className)}
      {onkeydown}
    >
      <span class="grabber" aria-hidden="true"></span>
      <header>
        <button type="button" class="back" data-layer-back onclick={close} aria-label="Back">
          <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor" aria-hidden="true"><path d="M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z"/></svg>
        </button>
        <h3 id={titleId}>{title}</h3>
      </header>
      <div class="body">{@render children()}</div>
      {#if footer}<footer>{@render footer()}</footer>{/if}
    </div>
  </div>
{/if}

<style>
  /* Fills the dialog's positioned form wrapper. Below 768 px: a bottom sheet over a scrim. From 768 px: fills the
     form area like a drill-in pane, no scrim. */
  .layer-scrim { position: absolute; inset: 0; z-index: 10; display: flex; align-items: flex-end; background: #0008; }
  .layer { position: relative; width: 100%; max-height: 92%; min-height: 50%; display: flex; flex-direction: column; background: var(--bg-secondary); color: var(--fg); border-top: 1px solid var(--border); border-radius: 16px 16px 0 0; animation: layer-up 160ms ease-out; }
  .grabber { align-self: center; width: 36px; height: 4px; margin: 8px 0 0; border-radius: 2px; background: var(--border); }
  header { display: flex; align-items: center; gap: 8px; padding: 4px 8px 4px 4px; }
  h3 { margin: 0; font-size: 1rem; font-weight: 700; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .back { flex: none; display: inline-flex; align-items: center; justify-content: center; min-width: 44px; min-height: 44px; padding: 0; background: transparent; border: none; color: var(--fg); }
  .body { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 16px 16px; }
  footer { flex: none; padding: 12px 16px calc(12px + env(safe-area-inset-bottom)); border-top: 1px solid var(--border); background: var(--bg-secondary); }

  @media (min-width: 768px) {
    .layer-scrim { background: none; }
    .layer { max-height: none; min-height: 0; height: 100%; border: none; border-radius: inherit; animation: none; }
    .grabber { display: none; }
  }
  @keyframes layer-up { from { transform: translateY(24px); opacity: 0; } to { transform: none; opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .layer { animation: none; } }
</style>
