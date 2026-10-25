<script lang="ts" module>
  export interface StripItem { id: string; name: string; agent?: string; command?: string; state: FleetState }
  /** The tab a key moves to, or null when the key is not a strip key. */
  export function stripTarget(key: string, index: number, count: number): number | null {
    if (count === 0) return null;
    if (key === 'ArrowRight') return (index + 1) % count;
    if (key === 'ArrowLeft') return (index - 1 + count) % count;
    if (key === 'Home') return 0;
    if (key === 'End') return count - 1;
    return null;
  }
</script>

<script lang="ts">
  import AgentBadge from './AgentBadge.svelte';
  import { STATES, type FleetState } from './states';
  import { cn } from '../ui/utils';
  /**
   * Every workspace's state on one row, the open one named. 48 px high with 8 px between items (DR1, DR2).
   * One tab stop (roving tabindex): Left/Right/Home/End move focus, Enter or Space opens the focused tab, the focused tab
   * scrolls into view, and when the focused tab goes away focus moves to its neighbour. Those keys are consumed here, so
   * none of them reaches the terminal behind the strip.
   */
  let { items, selected, onselect, onadd, class: className = '' }: { items: StripItem[]; selected?: string; onselect?: (id: string) => void; onadd?: () => void; class?: string } = $props();

  let root: HTMLElement | undefined = $state();
  let focused: string | null = $state(null);
  let lastIndex = 0;
  const stop = $derived(items.some((i) => i.id === focused) ? focused : items.some((i) => i.id === selected) ? selected : items[0]?.id);
  const tabs = () => [...(root?.querySelectorAll<HTMLElement>('[role=tab]') ?? [])];

  function keydown(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey || e.isComposing) return;
    const list = tabs();
    const index = list.indexOf(e.target as HTMLElement);
    if (index < 0) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault(); e.stopPropagation();
      onselect?.(items[index].id);
      return;
    }
    const to = stripTarget(e.key, index, list.length);
    if (to === null) return;
    e.preventDefault(); e.stopPropagation();
    list[to].focus();
  }
  function focusin(e: FocusEvent) {
    const el = (e.target as HTMLElement).closest<HTMLElement>('[role=tab]');
    if (!el) return;
    focused = el.dataset.id ?? null;
    lastIndex = tabs().indexOf(el);
    el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }
  // The focused tab was removed: focus would fall to the page. Put it on the tab now in its place.
  $effect(() => {
    void items;
    if (focused === null || items.some((i) => i.id === focused)) return;
    const hadFocus = root?.contains(document.activeElement) || document.activeElement === document.body;
    const next = items[Math.min(lastIndex, items.length - 1)];
    focused = next?.id ?? null;
    if (hadFocus && next) queueMicrotask(() => root?.querySelector<HTMLElement>(`[role=tab][data-id="${CSS.escape(next.id)}"]`)?.focus());
  });
</script>

<div bind:this={root} class={cn('flex items-center gap-2 overflow-x-auto px-3 py-1 [scrollbar-width:none]', className)} role="tablist" aria-label="Workspaces" onkeydown={keydown} onfocusin={focusin}>
  {#each items as item (item.id)}
    <button type="button" role="tab" data-id={item.id} aria-label={`${item.name}, ${STATES[item.state].label}`} aria-selected={item.id === selected} tabindex={item.id === stop ? 0 : -1} onclick={() => onselect?.(item.id)}
      onpointerdown={(e) => e.preventDefault()}
      class={cn('flex h-12 min-w-12 shrink-0 items-center justify-center gap-2 rounded-full bg-transparent p-1 text-fg', item.id === selected && 'bg-raised')}>
      <AgentBadge agent={item.agent} command={item.command} state={item.state} />
      {#if item.id === selected}<b class="pr-3 text-sm font-semibold">{item.name}</b>{/if}
    </button>
  {/each}
  {#if onadd}
    <button type="button" aria-label="New workspace" onclick={onadd} onpointerdown={(e) => e.preventDefault()}
      class="flex h-12 min-w-12 shrink-0 items-center justify-center rounded-full bg-transparent p-0 text-title text-muted">+</button>
  {/if}
</div>
