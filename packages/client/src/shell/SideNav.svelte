<script lang="ts">
  import { onDestroy } from 'svelte';
  import { DESTINATIONS, type Destination } from '../lib/router';
  import { HeldView, type NavGroup } from '../lib/navlist';
  import { STATES } from '../lib/ds/states';
  import AgentBadge from '../lib/ds/AgentBadge.svelte';
  import { cn } from '../lib/ui/utils';

  /**
   * The desktop sidebar: destinations, then every server with its sessions and their state. The list holds still while a
   * pointer is down and for 2 s after it scrolls (ui-never-moves-under-the-finger); a state change shows in place.
   */
  let { current, onnavigate, groups, activeKey = null, onopen, onserver, onnew, onsearch, searchHint, rightOpen = false, onright, width, class: className = '' }: {
    current: Destination; onnavigate: (d: Destination) => void; groups: NavGroup[]; activeKey?: string | null; onopen: (key: string) => void; onserver: (id: string) => void;
    onnew?: () => void; onsearch: () => void; searchHint: string; rightOpen?: boolean; onright?: () => void; width?: number; class?: string;
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
  const serverWord = (s: string) => (s === 'connected' ? 'Connected' : s.charAt(0).toUpperCase() + s.slice(1));
  const needs = $derived(groups.reduce((n, g) => n + g.sessions.filter((s) => s.state === 'needs-you').length, 0));
</script>

<aside aria-label="Workspace" data-shell="sidebar" style:width={width != null ? `${width}px` : undefined} class={cn('flex-col shrink-0 border-r border-line bg-panel', className)}>
  <nav aria-label="Main" class="flex flex-col gap-2 p-2">
    {#each DESTINATIONS as d (d.id)}
      <button type="button" data-destination={d.id} aria-current={current === d.id ? 'page' : undefined} onclick={() => onnavigate(d.id)}
        class={cn('flex h-11 items-center justify-between rounded-control bg-transparent px-3 text-base font-semibold text-soft', current === d.id && 'bg-raised text-accent-text')}>
        <span>{d.label}</span>
        {#if d.id === 'sessions' && needs}<span class="min-w-5 rounded-full bg-warn px-1 text-center text-xs font-bold text-on-warn" aria-label={`${needs} need you`}>{needs}</span>{/if}
      </button>
    {/each}
    <button type="button" onclick={onsearch} class="flex h-11 items-center justify-between rounded-control border border-line bg-transparent px-3 text-base text-soft">
      <span>Search</span><kbd class="font-sans text-xs text-muted">{searchHint}</kbd>
    </button>
  </nav>

  <div class="min-h-0 flex-1 overflow-y-auto border-t border-line px-2 pb-2" data-sessions-scroll onscroll={scrolled} onpointerdowncapture={down}>
    {#if onnew}
      <div class="flex items-center justify-between pt-3 pl-1"><h2 class="text-xs font-semibold tracking-wider text-soft uppercase">Servers</h2>
        <button type="button" onclick={onnew} class="h-11 rounded-control bg-transparent px-3 text-base font-semibold text-accent-text">+ New</button></div>
    {/if}
    {#each shown as g (g.server.id)}
      <section aria-label={g.server.server.server_name || 'Server'} data-server={g.server.id}>
        <button type="button" onclick={() => onserver(g.server.id)} class="mt-1 flex h-11 w-full items-center justify-between gap-2 rounded-control bg-transparent px-3 text-left">
          <span class="truncate text-base font-semibold text-fg">{g.server.server.server_name || 'Server'}</span>
          <span class={cn('flex shrink-0 items-center gap-1 text-xs', g.server.state === 'connected' ? 'text-ok' : 'text-soft')}><span aria-hidden="true">{g.server.state === 'connected' ? '●' : '○'}</span>{serverWord(g.server.state)}</span>
        </button>
        {#each g.sessions as s (s.key)}
          <button type="button" data-key={s.key} aria-current={s.key === activeKey ? 'true' : undefined} onclick={() => onopen(s.key)}
            class={cn('flex h-11 w-full items-center gap-2 rounded-control bg-transparent pr-3 pl-6 text-left', s.key === activeKey && 'bg-raised')}>
            <AgentBadge agent={s.agent} state={s.state} size={20} />
            <span class="min-w-0 flex-1 truncate text-base text-fg">{s.name}</span>
            <span class={cn('shrink-0 text-xs', STATES[s.state].text)}>{STATES[s.state].label}</span>
          </button>
        {:else}
          <p class="py-2 pl-6 text-sm text-muted">No sessions</p>
        {/each}
      </section>
    {/each}
  </div>

  {#if onright}
    <div class="border-t border-line p-2">
      <button type="button" aria-pressed={rightOpen} onclick={onright} class={cn('h-11 w-full rounded-control border border-line bg-transparent px-3 text-left text-base text-soft', rightOpen && 'bg-raised text-fg')}>Right pane</button>
    </div>
  {/if}
</aside>
