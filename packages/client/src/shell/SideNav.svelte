<script lang="ts">
  import { onDestroy } from 'svelte';
  import { RowDragger, type DragView } from '../lib/dragsort';
  import { DESTINATIONS, type Destination } from '../lib/router';
  import { HeldView, type NavGroup } from '../lib/navlist';
  import { STATES } from '../lib/ds/states';
  import AgentBadge from '../lib/ds/AgentBadge.svelte';
  import ServerStatusIcon from '../ServerStatusIcon.svelte';
  import { cn } from '../lib/ui/utils';

  /**
   * The desktop sidebar: destinations, then every server with its workspaces and their state. The list holds still while a
   * pointer is down and for 2 s after it scrolls (ui-never-moves-under-the-finger); a state change shows in place.
   */
  let { current, onnavigate, groups, activeKey = null, onopen, onserver, onnew, onrename, oncloseworkspace, onmove, onsearch, searchHint, splitOpen = false, onsplit, width, id, class: className = '' }: {
    current: Destination; onnavigate: (d: Destination) => void; groups: NavGroup[]; activeKey?: string | null; onopen: (key: string) => void; onserver: (id: string) => void;
    onnew?: () => void; onrename?: (key: string, name: string) => void; oncloseworkspace?: (key: string) => void; /** Drop `key` beside `target.key`: the sidebar's own drag, or the arrow keys on a row's handle. */ onmove?: (key: string, target: { key: string; side: 'before' | 'after' }) => void; onsearch: () => void; searchHint: string; splitOpen?: boolean; onsplit?: () => void; width?: number; id?: string; class?: string;
  } = $props();

  const held = new HeldView();
  let rev = $state(0);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const shown = $derived.by(() => { void rev; return held.view(groups, Date.now()); });
  // A mouse button can be released anywhere, so the release is heard on the window, not on the list.
  function down(e: PointerEvent) {
    const id = e.pointerId;
    held.down(id);
    const release = (ev: PointerEvent) => {
      if (ev.pointerId !== id) return;
      window.removeEventListener('pointerup', release, true);
      window.removeEventListener('pointercancel', release, true);
      held.up(id);
      rev++;
    };
    window.addEventListener('pointerup', release, true);
    window.addEventListener('pointercancel', release, true);
  }
  function scrolled() {
    held.scrolled(Date.now());
    clearTimeout(timer);
    timer = setTimeout(() => rev++, 2050); // the hold ends 2 s after the last scroll: show what arrived meanwhile
  }
  onDestroy(() => clearTimeout(timer));
  // Rename in place: a double click, a click on the name of the open workspace, F2, or the pencil. Enter saves, Esc or leaving the field cancels.
  let editing = $state<string | null>(null);
  let draft = $state('');
  function startEdit(key: string, name: string) { if (!onrename) return; editing = key; draft = name; }
  function commit(key: string, name: string) {
    if (editing !== key) return;
    editing = null;
    const next = draft.trim();
    if (next && next !== name) onrename?.(key, next);
  }
  const focusSelect = (el: HTMLInputElement) => { el.focus(); el.select(); };
  function editKey(e: KeyboardEvent, key: string, name: string) {
    if (e.key === 'Enter') { e.preventDefault(); commit(key, name); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); editing = null; }
  }
  function rowKey(e: KeyboardEvent, key: string, name: string) {
    if (e.key === 'F2' && onrename) { e.preventDefault(); startEdit(key, name); }
  }
  // Reordering by the handle (the same dragger as the phone list): the row follows the pointer and the list changes on release.
  let scroller: HTMLElement | undefined = $state();
  let drag: DragView | null = $state(null);
  let announcement = $state('');
  const dragger = new RowDragger({
    scroller: () => scroller,
    onview: (v) => (drag = v),
    ondrop: (key, target) => {
      onmove?.(key, target);
      const name = groups.flatMap((g) => g.workspaces).find((s) => s.key === key)?.name ?? '';
      const mine = groups.find((g) => g.workspaces.some((s) => s.key === key))?.workspaces ?? [];
      const rest = mine.filter((s) => s.key !== key);
      const at = rest.findIndex((s) => s.key === target.key);
      if (at >= 0) announcement = `${name} moved to position ${at + (target.side === 'after' ? 2 : 1)} of ${mine.length}`;
    },
  });
  $effect(() => () => dragger.destroy());
  const needs = $derived(groups.reduce((n, g) => n + g.workspaces.filter((s) => s.state === 'needs-you').length, 0));
</script>

<aside {id} aria-label="Workspace" data-shell="sidebar" style:width={width ? `${width}px` : undefined} class={cn('flex shrink-0 flex-col border-r border-line bg-panel', !width && 'w-64', className)}>
  <nav aria-label="Main" class="flex flex-col gap-2 p-2">
    {#each DESTINATIONS as d (d.id)}
      <button type="button" data-destination={d.id} aria-current={current === d.id ? 'page' : undefined} onclick={() => onnavigate(d.id)}
        class={cn('flex h-11 items-center justify-between rounded-control bg-transparent px-3 text-base font-semibold text-soft', current === d.id && 'bg-raised text-accent-text')}>
        <span>{d.label}</span>
        {#if d.id === 'workspaces' && needs}<span class="min-w-5 rounded-full bg-warn px-1 text-center text-xs font-bold text-on-warn" aria-label={`${needs} need you`}>{needs}</span>{/if}
      </button>
    {/each}
    <button type="button" onclick={onsearch} class="flex h-11 items-center justify-between rounded-control border border-line bg-transparent px-3 text-base text-soft">
      <span>Search</span><kbd class="font-sans text-xs text-muted">{searchHint}</kbd>
    </button>
  </nav>

  <div class="min-h-0 flex-1 overflow-y-auto border-t border-line px-2 pb-2" data-workspaces-scroll bind:this={scroller} onscroll={scrolled} onpointerdowncapture={down}>
    {#if onnew}
      <div class="flex items-center justify-between pt-3 pl-1"><h2 class="text-xs font-semibold tracking-wider text-soft uppercase">Servers</h2>
        <button type="button" onclick={onnew} class="h-11 rounded-control bg-transparent px-3 text-base font-semibold text-accent-text">+ New</button></div>
    {/if}
    {#each shown as g (g.server.id)}
      <section aria-label={g.server.server.server_name || 'Server'} data-server={g.server.id}>
        <button type="button" onclick={() => onserver(g.server.id)} class="mt-1 flex h-11 w-full items-center gap-2 rounded-control bg-transparent px-3 text-left">
          <ServerStatusIcon state={g.server.state} name={g.server.server.server_name || 'Server'} />
          <span class="truncate text-base font-semibold text-fg">{g.server.server.server_name || 'Server'}</span>
        </button>
        {#each g.workspaces as s (s.key)}
          {@const grip = !!onmove && g.workspaces.length > 1}
          {@const dragging = drag?.key === s.key}
          <div data-row={s.key} data-list={g.server.id} class={cn('group relative flex h-11 w-full items-center rounded-control', s.key === activeKey && 'bg-raised')} class:dragging
            class:drop-before={drag?.target?.key === s.key && drag?.target?.side === 'before'} class:drop-after={drag?.target?.key === s.key && drag?.target?.side === 'after'}
            style:transform={dragging ? `translateY(${drag?.dy ?? 0}px)` : undefined}>
            {#if grip && editing !== s.key}
              <button type="button" class="grip row-action" data-testid="workspace-grip" data-grip aria-label={`Reorder ${s.name}. Drag, or use the up and down arrow keys`}
                onpointerdown={(e) => dragger.down(e, s.key)} onpointermove={(e) => dragger.move(e)} onpointerup={(e) => dragger.up(e)} onpointercancel={(e) => dragger.up(e, true)} onkeydown={(e) => dragger.key(e, s.key)} oncontextmenu={(e) => e.preventDefault()}>
                <svg viewBox="0 0 12 18" width="10" height="15" aria-hidden="true" fill="currentColor"><circle cx="3" cy="3" r="1.5" /><circle cx="9" cy="3" r="1.5" /><circle cx="3" cy="9" r="1.5" /><circle cx="9" cy="9" r="1.5" /><circle cx="3" cy="15" r="1.5" /><circle cx="9" cy="15" r="1.5" /></svg>
              </button>
            {/if}
            {#if editing === s.key}
              <div class="flex min-w-0 flex-1 items-center gap-2 pr-3 pl-6">
                <AgentBadge agent={s.agent} state={s.state} size={20} />
                <input use:focusSelect bind:value={draft} aria-label="Workspace name" maxlength="64" autocomplete="off" spellcheck="false" data-rename={s.key}
                  onkeydown={(e) => editKey(e, s.key, s.name)} onblur={() => commit(s.key, s.name)}
                  class="h-11 min-w-0 flex-1 rounded-control border border-accent bg-bg px-2 text-base text-fg outline-none" />
              </div>
            {:else}
              <button type="button" data-key={s.key} aria-current={s.key === activeKey ? 'true' : undefined} onclick={() => onopen(s.key)} onkeydown={(e) => rowKey(e, s.key, s.name)}
                class={cn('flex h-11 min-w-0 flex-1 items-center gap-2 rounded-control bg-transparent pr-3 text-left', grip ? 'pl-1' : 'pl-6')}>
                <AgentBadge agent={s.agent} state={s.state} size={20} />
                <span class="min-w-0 flex-1 truncate text-base text-fg" role="presentation">{s.name}</span>
                <span class={cn('shrink-0 text-xs', STATES[s.state].text)}>{STATES[s.state].label}</span>
              </button>
              {#if onrename}
                <button type="button" aria-label={`Rename ${s.name}`} title="Rename (F2)" data-rename-button={s.key} onclick={() => startEdit(s.key, s.name)}
                  class="row-action h-11 w-8 shrink-0 rounded-control bg-transparent text-soft">✎</button>
              {/if}
              {#if oncloseworkspace}
                <button type="button" aria-label={`Close ${s.name}`} title="Close workspace" data-close-button={s.key} onclick={() => oncloseworkspace?.(s.key)}
                  class="row-action h-11 w-8 shrink-0 rounded-control bg-transparent text-soft">×</button>
              {/if}
            {/if}
          </div>
        {:else}
          <p class="py-2 pl-6 text-sm text-muted">No workspaces</p>
        {/each}
      </section>
    {/each}
    <div class="sr-only" role="status" aria-live="polite">{announcement}</div>
  </div>

  {#if onsplit}
    <div class="border-t border-line p-2">
      <button type="button" aria-pressed={splitOpen} onclick={onsplit} class={cn('h-11 w-full rounded-control border border-line bg-transparent px-3 text-left text-base text-soft', splitOpen && 'bg-raised text-fg')}>Terminal beside the decision</button>
    </div>
  {/if}
</aside>

<style>
  /* Row actions show on hover and keyboard focus, and always where there is no hover (a touch screen). */
  .row-action { opacity: 0; }
  :global([data-row]:hover) .row-action, :global([data-row]:focus-within) .row-action { opacity: 1; }
  .grip { flex: 0 0 24px; width: 24px; height: 44px; margin-left: 2px; padding: 0; display: flex; align-items: center; justify-content: center; background: transparent; color: var(--fg-muted); border: 0; touch-action: none; user-select: none; -webkit-user-select: none; cursor: grab; }
  .dragging { z-index: 3; opacity: 0.95; filter: drop-shadow(0 4px 10px #0006); }
  .dragging .grip { cursor: grabbing; }
  .drop-before::before, .drop-after::after { content: ''; position: absolute; left: 0; right: 0; height: 2px; background: var(--accent, currentColor); }
  .drop-before::before { top: -1px; }
  .drop-after::after { bottom: -1px; }
  .row-action:hover { color: var(--fg); }
  @media (hover: none) { .row-action { opacity: 1; } }
</style>
