<script lang="ts">
  import { EmptyState, QueueItem } from './lib/ds';
  import type { AgentMap } from './lib/agents';
  import type { HubWorkspace } from './lib/hub';
  import type { TabAgentState } from './lib/protocol';
  import { ageText } from './lib/cards';
  import { workspaceGroups, type Row } from './lib/home';
  import { dropIndex, moveWorkspace, movedText } from './lib/order';
  import { RowDragger, type DragView } from './lib/dragsort';

  /**
   * The Workspaces destination without an open workspace: every workspace, by server, in the user's own order and in the rows Home
   * uses. Each row has a handle to drag it (the only part that does not scroll), a pencil to rename it and a cross to close it,
   * which always asks first; a server without tabs also keeps the actions menu, where rename and "Move up / Move down" work without dragging.
   */
  let { workspaces, agents, now, onopen, onnew, onactions, onrename, onclose, onmove, unhealthy, tabStates = {} }: { workspaces: HubWorkspace[]; agents: AgentMap; now: number; /** Ids of the servers that say they are unhealthy (U14). */ unhealthy?: ReadonlySet<string>; tabStates?: Record<string, TabAgentState>; onopen: (key: string) => void; onnew: (serverId?: string) => void; onactions?: (key: string) => void; /** The pencil: rename the workspace where it is shown. */ onrename?: (key: string, name: string) => void; /** The cross: close it, always after the confirmation that names it and its server. */ onclose?: (key: string) => void; /** The server's own order when the server serves tabs. */ onmove?: (key: string, index: number) => void } = $props();
  const groups = $derived(workspaceGroups(workspaces, agents, unhealthy, tabStates));
  const meta = (r: Row) => [r.serverName, r.tabs ? `${r.tabs} ${r.tabs === 1 ? 'tab' : 'tabs'}` : '', r.since ? `waiting ${ageText(r.since, now)}` : ''].filter(Boolean).join(' · ');

  let scroller: HTMLElement | undefined = $state();
  let drag: DragView | null = $state(null);
  let announcement = $state('');
  const dragger = new RowDragger({
    scroller: () => scroller,
    onview: (v) => (drag = v),
    ondrop: (key, target) => {
      if (onmove) {
        const index = dropIndex(workspaces, key, target);
        if (index !== null) {
          onmove(key, index);
          announcement = `${workspaces.find((s) => s.key === key)?.name ?? ''} moved to position ${index + 1} of ${workspaces.filter((s) => s.serverId === workspaces.find((w) => w.key === key)?.serverId).length}`;
        }
        return;
      }
      const p = moveWorkspace(workspaces, key, target.side === 'before' ? { before: target.key } : { after: target.key });
      if (p) announcement = movedText(workspaces.find((s) => s.key === key)?.name ?? '', p);
    },
  });
  $effect(() => () => dragger.destroy());
</script>

<div class="flex min-h-0 flex-1 flex-col">
  {#if groups.length}
    <div class="flex justify-end px-4 pt-3" data-workspaces-toolbar>
      <button type="button" data-testid="workspaces-new" onclick={() => onnew()}
        class="h-11 rounded-control bg-transparent px-3 text-base font-semibold text-accent-text">+ New</button>
    </div>
  {/if}
  <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4" data-workspace-groups bind:this={scroller}>
    <div class="sr-only" role="status" aria-live="polite">{announcement}</div>
    {#each groups as g (g.serverId)}
      <section class="flex flex-col gap-2" aria-label={g.title} data-server={g.serverId}>
        {#if groups.length > 1}<h2 class="text-sm font-semibold text-soft">{g.title} · {g.rows.length}</h2>{/if}
        {#each g.rows as r (r.key)}
          {@const dragging = drag?.key === r.key}
          <div data-row={r.key} data-list={g.serverId} class="relative flex items-center gap-1" class:dragging style:transform={dragging ? `translateY(${drag?.dy ?? 0}px)` : undefined}
            class:drop-before={drag?.target?.key === r.key && drag?.target?.side === 'before'} class:drop-after={drag?.target?.key === r.key && drag?.target?.side === 'after'}>
            {#if g.rows.length > 1}
              <button type="button" class="grip" data-testid="workspace-grip" data-grip aria-label={`Reorder ${r.name}. Drag, or use the up and down arrow keys`}
                onpointerdown={(e) => dragger.down(e, r.key)} onpointermove={(e) => dragger.move(e)} onpointerup={(e) => dragger.up(e)} onpointercancel={(e) => dragger.up(e, true)} onkeydown={(e) => dragger.key(e, r.key)} oncontextmenu={(e) => e.preventDefault()}>
                <svg viewBox="0 0 12 18" width="12" height="18" aria-hidden="true" fill="currentColor"><circle cx="3" cy="3" r="1.5" /><circle cx="9" cy="3" r="1.5" /><circle cx="3" cy="9" r="1.5" /><circle cx="9" cy="9" r="1.5" /><circle cx="3" cy="15" r="1.5" /><circle cx="9" cy="15" r="1.5" /></svg>
              </button>
            {/if}
            <QueueItem class="min-w-0 flex-1" name={r.name} summary={r.title} meta={meta(r)} state={r.state} agent={r.agent} unhealthy={r.unhealthy} onclick={() => onopen(r.key)} />
            {#if onrename}
              <button type="button" class="rename" data-testid="workspace-rename" aria-label={`Rename ${r.name}`} title="Rename" onclick={() => onrename(r.key, r.name)}>
                <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="m11.5 2.5 2 2L6 12l-2.7.7L4 10z" /></svg>
              </button>
            {/if}
            {#if onclose}
              <button type="button" class="close" data-testid="workspace-close" aria-label={`Close ${r.name}`} title="Close" onclick={() => onclose?.(r.key)}>
                <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="m4 4 8 8M12 4l-8 8" /></svg>
              </button>
            {/if}
            {#if onactions}
              <button type="button" class="more" data-testid="workspace-actions" aria-label={`Actions for ${r.name}: rename, move, detach`} onclick={() => onactions(r.key)}>
                <svg viewBox="0 0 16 4" width="16" height="4" aria-hidden="true" fill="currentColor"><circle cx="2" cy="2" r="1.6" /><circle cx="8" cy="2" r="1.6" /><circle cx="14" cy="2" r="1.6" /></svg>
              </button>
            {/if}
          </div>
        {/each}
      </section>
    {:else}
      <EmptyState title="No workspaces yet" body="Start one and it shows up here.">
        <button type="button" onclick={() => onnew()} class="min-h-11 rounded-card bg-accent px-4 text-base font-semibold text-on-accent">Start a workspace</button>
      </EmptyState>
    {/each}
  </div>
</div>

<style>
  /* 44px targets. Only the handle takes the finger away from scrolling. */
  .grip, .more, .rename, .close { flex: 0 0 44px; width: 44px; height: 44px; display: flex; align-items: center; justify-content: center; background: transparent; color: var(--fg-muted, currentColor); border: 0; padding: 0; }
  .rename { color: var(--fg-muted, currentColor); }
  .grip { touch-action: none; user-select: none; -webkit-user-select: none; -webkit-touch-callout: none; cursor: grab; }
  .dragging { z-index: 3; opacity: 0.95; filter: drop-shadow(0 4px 10px #0006); }
  .dragging .grip { cursor: grabbing; }
  .drop-before::before, .drop-after::after { content: ''; position: absolute; left: 0; right: 0; height: 3px; border-radius: 2px; background: var(--accent); z-index: 2; }
  .drop-before::before { top: -6px; }
  .drop-after::after { bottom: -6px; }
</style>
