<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import AgentIcon from './AgentIcon.svelte';
  import { rememberFocus } from './lib/focus';
  import { RowDragger, type DragView } from './lib/dragsort';
  import { tabIconAgent, tabIconCommand, tabOrder, tabPath, tabStateKey, tabStateWord } from './lib/workspaceTabs';
  import type { HubWorkspace } from './lib/hub';
  import type { TabAgentState, TabInfo } from './lib/protocol';

  interface TabGroup { workspace: HubWorkspace; tabs: TabInfo[]; editable?: boolean }
  /**
   * Every tab of one workspace, or of every workspace from the Workspaces page (decision 0149). Rename and Delete are in
   * the row's own menu, never a one-tap on the tab; Close workspace is the only workspace action and always asks.
   */
  let { groups, activeKey = null, activeTab = null, states = {}, cacheTtlMs = 5 * 60_000, onopen, onrename, onclosetab, onmove, onnew, oncloseworkspace, onclose }: {
    groups: TabGroup[];
    activeKey?: string | null;
    activeTab?: string | null;
    /** Per-tab agent state (`hub.tabStates`), keyed by workspace key and tab id. */
    states?: Record<string, TabAgentState>;
    /** The prompt-cache lifetime the device uses for the Cache expiring filter. */
    cacheTtlMs?: number;
    onopen: (workspaceKey: string, tab: string) => void;
    onrename?: (workspaceKey: string, tab: TabInfo) => void;
    onclosetab?: (workspaceKey: string, tab: TabInfo) => void;
    onmove?: (workspaceKey: string, tab: string, index: number) => void;
    onnew?: (workspaceKey: string) => void;
    oncloseworkspace?: (workspaceKey: string) => void;
    onclose: () => void;
  } = $props();

  let dialog: HTMLDialogElement;
  let scroller: HTMLElement | undefined = $state();
  let actions: string | null = $state(null);
  let confirming: string | null = $state(null);
  let closing: string | null = $state(null);
  let drag: DragView | null = $state(null);
  let announcement = $state('');
  let filter: 'all' | 'recent' | 'expiring' = $state('all');
  const single = $derived(groups.length === 1);
  const stateOf = (workspaceKey: string, tab: string) => states[tabStateKey(workspaceKey, tab)];
  /** A tab with a warm cache in its last minute: the same rule the old Workspaces filters used (decision 0083). */
  function expiring(workspaceKey: string, tab: TabInfo): boolean {
    const s = stateOf(workspaceKey, tab.id);
    if (!s?.agent || !s.last_response_at || (s.state !== 'idle' && s.state !== 'done')) return false;
    const left = cacheTtlMs - Math.max(0, Date.now() - s.last_response_at);
    return left > 0 && left <= 60_000;
  }
  /** The server's order normally; Recent puts the tab whose agent spoke last first. */
  function shownTabs(group: TabGroup): TabInfo[] {
    const tabs = tabOrder(group.tabs);
    if (filter === 'expiring') return tabs.filter((t) => expiring(group.workspace.key, t));
    if (filter === 'recent') {
      return [...tabs].sort((a, b) => {
        const sa = stateOf(group.workspace.key, a.id);
        const sb = stateOf(group.workspace.key, b.id);
        return (sb?.last_response_at ?? sb?.ts ?? 0) - (sa?.last_response_at ?? sa?.ts ?? 0);
      });
    }
    return tabs;
  }
  const dragger = new RowDragger({
    scroller: () => scroller,
    onview: (v) => (drag = v),
    ondrop: (key, target) => {
      if (!single || !onmove) return;
      const group = groups[0];
      const ids = tabOrder(group.tabs).map((t) => t.id).filter((id) => id !== key);
      const at = ids.indexOf(target.key);
      if (at < 0) return;
      const index = at + (target.side === 'after' ? 1 : 0);
      onmove(group.workspace.key, key, index);
      const tab = group.tabs.find((t) => t.id === key);
      announcement = `${tab?.label ?? 'Tab'} moved to position ${index + 1} of ${ids.length + 1}`;
    },
  });
  $effect(() => () => dragger.destroy());
  const restoreFocus = rememberFocus();
  onMount(() => dialog.showModal());
  onDestroy(restoreFocus);

  const currentOf = (group: TabGroup) =>
    group.tabs.find((t) => t.id === (activeKey === group.workspace.key && activeTab ? activeTab : group.workspace.focus));
  const keyOf = (workspaceKey: string, tab: string) => `${workspaceKey}/${tab}`;
  function open(workspaceKey: string, tab: string) { onclose(); onopen(workspaceKey, tab); }
  function rename(workspaceKey: string, tab: TabInfo) { onclose(); onrename?.(workspaceKey, tab); }
  function remove(workspaceKey: string, tab: TabInfo) {
    if (confirming !== keyOf(workspaceKey, tab.id)) { confirming = keyOf(workspaceKey, tab.id); return; }
    confirming = null; actions = null;
    onclosetab?.(workspaceKey, tab);
  }
  function newTab(workspaceKey: string) { onclose(); onnew?.(workspaceKey); }
  function closeWorkspace(workspaceKey: string) {
    if (closing !== workspaceKey) { closing = workspaceKey; return; }
    onclose(); oncloseworkspace?.(workspaceKey);
  }
</script>

<dialog bind:this={dialog} aria-label="Tabs" oncancel={(e) => { e.preventDefault(); onclose(); }} onclick={(e) => { if (e.target === dialog) onclose(); }}>
  <header>
    <div class="who">
      {#if single}
        <strong>{groups[0].workspace.name}</strong>{#if groups[0].workspace.serverName}<span>{groups[0].workspace.serverName}</span>{/if}
      {:else}
        <strong>Tabs</strong>
      {/if}
    </div>
    <button type="button" class="secondary close" aria-label="Close the tab menu" onclick={onclose}>✕</button>
  </header>
  <div class="sr" role="status" aria-live="polite">{announcement}</div>
  <div class="filters" role="group" aria-label="Filter tabs">
    <button type="button" class="secondary" aria-pressed={filter === 'all'} onclick={() => (filter = 'all')}>All</button>
    <button type="button" class="secondary" aria-pressed={filter === 'recent'} onclick={() => (filter = 'recent')}>Recent</button>
    <button type="button" class="secondary" aria-pressed={filter === 'expiring'} onclick={() => (filter = 'expiring')}>Cache expiring</button>
  </div>
  <div class="list" bind:this={scroller}>
    {#each groups as group (group.workspace.key)}
      {@const tabs = shownTabs(group)}
      <section data-group={group.workspace.key} aria-label={`Tabs of ${group.workspace.name}`}>
        {#if !single}<h3>{group.workspace.name} <span>{group.workspace.serverName}</span></h3>{/if}
        {#each tabs as tab (tab.id)}
          {@const active = currentOf(group)?.id === tab.id}
          {@const dragging = drag?.key === tab.id}
          <div class="row" data-row={tab.id} data-list={group.workspace.key} class:active class:dragging
            style:transform={dragging ? `translateY(${drag?.dy ?? 0}px)` : undefined}
            class:drop-before={drag?.target?.key === tab.id && drag?.target?.side === 'before'}
            class:drop-after={drag?.target?.key === tab.id && drag?.target?.side === 'after'}>
            {#if single && onmove && tabs.length > 1}
              <button type="button" class="grip" data-grip aria-label={`Reorder ${tab.label}. Drag, or use the arrow keys`}
                onpointerdown={(e) => dragger.down(e, tab.id)} onpointermove={(e) => dragger.move(e)}
                onpointerup={(e) => dragger.up(e)} onpointercancel={(e) => dragger.up(e, true)}
                onkeydown={(e) => dragger.key(e, tab.id)} oncontextmenu={(e) => e.preventDefault()}>
                <svg viewBox="0 0 12 18" width="12" height="18" aria-hidden="true" fill="currentColor"><circle cx="3" cy="3" r="1.5" /><circle cx="9" cy="3" r="1.5" /><circle cx="3" cy="9" r="1.5" /><circle cx="9" cy="9" r="1.5" /><circle cx="3" cy="15" r="1.5" /><circle cx="9" cy="15" r="1.5" /></svg>
              </button>
            {/if}
            <button type="button" class="tab" aria-current={active ? 'true' : undefined} onclick={() => open(group.workspace.key, tab.id)}>
              <AgentIcon command={tabIconCommand(tab)} agent={tabIconAgent(tab)} size={22} />
              <span class="text"><span class="label">{tab.label}</span><span class="path">{tabPath(group.workspace.dir, tab)}</span></span>
              {#if stateOf(group.workspace.key, tab.id)}
                {@const s = stateOf(group.workspace.key, tab.id)}
                <span class="state" data-state={s?.state} class:needs={s?.state === 'needs_input'} class:working={s?.state === 'working'} class:failed={s?.state === 'error'}>{tabStateWord(s?.state)}</span>
              {/if}
              {#if active}<span class="here" aria-label="current tab">●</span>{/if}
            </button>
            {#if group.editable !== false && (onrename || onclosetab)}
              <button type="button" class="more" aria-label={`Actions for ${tab.label}`} aria-expanded={actions === keyOf(group.workspace.key, tab.id)}
                onclick={() => (actions = actions === keyOf(group.workspace.key, tab.id) ? null : keyOf(group.workspace.key, tab.id))}>⋮</button>
            {/if}
          </div>
          {#if actions === keyOf(group.workspace.key, tab.id)}
            <div class="actions" role="group" aria-label={`Actions for ${tab.label}`}>
              {#if group.editable !== false && onrename}<button type="button" onclick={() => rename(group.workspace.key, tab)}>Rename</button>{/if}
              {#if group.editable !== false && onclosetab}
                <button type="button" class="danger" onclick={() => remove(group.workspace.key, tab)}>
                  {confirming === keyOf(group.workspace.key, tab.id) ? 'Delete this tab?' : 'Delete'}
                </button>
                {#if confirming === keyOf(group.workspace.key, tab.id)}
                  <button type="button" class="secondary" onclick={() => (confirming = null)}>Cancel</button>
                {/if}
              {/if}
            </div>
          {/if}
        {:else}
          <p class="none">No tabs</p>
        {/each}
        {#if group.editable !== false && onnew}<button type="button" class="new" onclick={() => newTab(group.workspace.key)}>+ New window</button>{/if}
      </section>
    {/each}
  </div>
  {#if single && oncloseworkspace}
    <div class="footer">
      {#if closing === groups[0].workspace.key}
        <p role="alert">Close <strong>{groups[0].workspace.name}</strong>? Everything running in it ends.</p>
        <button type="button" class="danger" onclick={() => closeWorkspace(groups[0].workspace.key)}>Close workspace</button>
        <button type="button" class="secondary" onclick={() => (closing = null)}>Cancel</button>
      {:else}
        <button type="button" class="danger" onclick={() => closeWorkspace(groups[0].workspace.key)}>Close workspace</button>
      {/if}
    </div>
  {/if}
</dialog>

<style>
  dialog { margin: 56px auto auto; width: min(100% - 16px, 460px); max-height: calc(var(--app-height, 100dvh) - 72px); padding: 0; overflow: hidden; display: flex; flex-direction: column; border: 1px solid var(--border); border-radius: 12px; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0006; }
  header { display: flex; align-items: center; gap: 6px; padding: 8px; border-bottom: 1px solid var(--border); }
  .who { flex: 1; display: flex; gap: 8px; align-items: baseline; min-width: 0; }
  .who strong, .who span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .who span { color: var(--fg-muted); font-size: 0.8rem; }
  .close { min-height: 44px; min-width: 44px; padding: 0; }
  .filters { display: flex; gap: 4px; padding: 6px 8px 0; }
  .filters button { min-height: 44px; padding: 0 10px; font-size: 0.8rem; }
  .filters button[aria-pressed='true'] { border-color: var(--accent); color: var(--accent); }
  .list { overflow-y: auto; padding: 4px; }
  .state { flex: 0 0 auto; padding: 2px 6px; border-radius: 999px; border: 1px solid var(--border); color: var(--fg-muted); font-size: 0.7rem; white-space: nowrap; }
  .state.needs { border-color: var(--accent); color: var(--accent); font-weight: 600; }
  .state.working { color: var(--fg); }
  .state.failed { border-color: var(--danger); color: var(--danger); }
  section { display: flex; flex-direction: column; gap: 2px; }
  h3 { margin: 8px 6px 2px; font-size: 0.8rem; font-weight: 600; color: var(--fg-muted); }
  h3 span { font-weight: 400; }
  .row { position: relative; display: flex; align-items: stretch; gap: 2px; }
  .row.active .tab { border-color: var(--accent); }
  .row.dragging { z-index: 3; opacity: 0.95; filter: drop-shadow(0 4px 10px #0006); }
  .drop-before::before, .drop-after::after { content: ''; position: absolute; left: 0; right: 0; height: 3px; border-radius: 2px; background: var(--accent); z-index: 2; }
  .drop-before::before { top: -3px; }
  .drop-after::after { bottom: -3px; }
  .tab { flex: 1; min-width: 0; min-height: 48px; display: flex; align-items: center; gap: 8px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .text { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .label, .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .path { font-size: 0.75rem; color: var(--fg-muted); }
  .here { color: var(--accent); }
  .grip, .more { flex: 0 0 44px; width: 44px; min-height: 48px; display: flex; align-items: center; justify-content: center; background: transparent; color: var(--fg-muted); border: 0; padding: 0; }
  .grip { touch-action: none; user-select: none; cursor: grab; }
  .actions { display: flex; gap: 4px; padding: 2px 4px 6px; }
  .actions button { min-height: 44px; flex: 1; }
  .new { min-height: 48px; margin: 4px 4px 8px; }
  .footer { display: flex; gap: 6px; flex-wrap: wrap; padding: 8px; border-top: 1px solid var(--border); }
  .footer p { flex: 1 1 100%; margin: 0 0 4px; }
  .footer button { min-height: 44px; flex: 1; }
  .danger { background: var(--danger); color: var(--bg); }
  .none { padding: 12px; color: var(--fg-muted); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
</style>
