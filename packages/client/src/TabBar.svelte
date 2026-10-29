<script lang="ts">
  import AgentIcon from './AgentIcon.svelte';
  import { liveTab, tabIconAgent, tabIconCommand, tabOrder, tabPath } from './lib/workspaceTabs';
  import type { TabInfo } from './lib/protocol';
  import type { HubWorkspace } from './lib/hub';
  /**
   * The tabs of the open workspace (decision 0149). A phone has one plaque with the current tab; a wide screen has the
   * strip. Opening a tab that is not current switches to it; the current tab opens the tab menu, so a rename or delete is
   * never a one-tap on the tab itself.
   */
  let { workspace, tabs, activeTab, mobile = false, onopen, onmenu }: {
    workspace: HubWorkspace;
    tabs: TabInfo[];
    activeTab?: string | null;
    mobile?: boolean;
    onopen: (tab: string) => void;
    onmenu: () => void;
  } = $props();
  const ordered = $derived(tabOrder(tabs));
  const current = $derived(liveTab(ordered, activeTab, workspace.focus));
</script>

{#if mobile}
  <button type="button" class="plaque" data-testid="tab-plaque" aria-haspopup="dialog"
    aria-label={`Tabs of ${workspace.name}${current ? ': ' + current.label : ''}`}
    onclick={onmenu} oncontextmenu={(e) => { e.preventDefault(); onmenu(); }}>
    {#if current}
      <AgentIcon command={tabIconCommand(current)} agent={tabIconAgent(current)} size={20} />
      <span class="label">{current.label}</span>
      <span class="chevron" aria-hidden="true">▾</span>
    {:else}
      <span class="label">No tabs</span>
    {/if}
  </button>
{:else}
  <div class="strip" role="tablist" aria-label={`Tabs of ${workspace.name}`} data-testid="tab-strip">
    {#each ordered as tab (tab.id)}
      <button type="button" role="tab" data-tab={tab.id} aria-selected={current?.id === tab.id}
        class:active={current?.id === tab.id} title={tabPath(workspace.dir, tab)}
        aria-label={`${tab.label}${current?.id === tab.id ? ', current tab' : ''}`}
        onclick={() => (current?.id === tab.id ? onmenu() : onopen(tab.id))}
        oncontextmenu={(e) => { e.preventDefault(); onmenu(); }}>
        <AgentIcon command={tabIconCommand(tab)} agent={tabIconAgent(tab)} size={18} />
        <span class="label">{tab.label}</span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .plaque { display: flex; align-items: center; gap: 8px; min-height: 44px; min-width: 44px; max-width: 100%; padding: 0 10px; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 8px; flex: 0 1 auto; }
  .plaque .label, .strip .label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chevron { color: var(--fg-muted); font-size: 0.7rem; }
  .strip { flex: 1 1 auto; display: flex; gap: 4px; overflow-x: auto; scrollbar-width: none; overscroll-behavior-x: contain; touch-action: pan-x; min-width: 0; }
  .strip::-webkit-scrollbar { display: none; }
  .strip button { flex: 0 0 auto; max-width: 220px; min-height: 44px; min-width: 44px; display: flex; align-items: center; gap: 6px; white-space: nowrap; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 8px; padding: 0 10px; }
  .strip button.active { border-color: var(--accent); }
  .strip button.active .label { font-weight: 600; }
</style>
