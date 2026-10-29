<script lang="ts">
  import TabStrip from './TabStrip.svelte';
  import TabBar from './TabBar.svelte';
  import { needsYouCount } from './lib/tabs';
  import type { AgentMap } from './lib/agents';
  import type { HubServer, HubWorkspace } from './lib/hub';
  import type { TabInfo } from './lib/protocol';
  import type { WindowMap } from './lib/windows';
  /**
   * The top edge (decisions 0101, 0147). With a tab-capable server it is the workspace's tabs: one plaque on a phone,
   * the strip and + on a wide screen; on the Workspaces page it is one plaque that opens every workspace's tabs. An
   * older server keeps the old workspace strip and its Workspaces button.
   */
  export let workspaces: HubWorkspace[];
  export let agents: AgentMap;
  export let servers: HubServer[];
  export let activeKey: string | null;
  export let held = false;
  export let onopen: (key: string) => void;
  export let onactions: (key: string) => void;
  export let windows: WindowMap = {};
  export let expiring: Set<string> = new Set();
  export let link: { ok: boolean; text: string } | null = null;
  export let onlink: () => void = () => {};
  export let onmenu: () => void;
  export let ontabmenu: (key: string) => void = () => {};
  export let onpeek: (key: string) => void = () => {};
  export let onworkspaces: () => void;
  export let expanded: boolean | undefined = undefined;
  export let controls: string | undefined = undefined;
  export let strip = true;
  export let onnew: (() => void) | undefined = undefined;
  /** The new path: the open workspace and its tabs. */
  export let tabsCapable = false;
  export let workspace: HubWorkspace | null = null;
  export let tabs: TabInfo[] = [];
  export let activeTab: string | null = null;
  export let page = false;
  export let wide = false;
  export let ontabopen: (tab: string) => void = () => {};
  export let ontabmenuopen: () => void = () => {};
  export let onnewtab: (() => void) | undefined = undefined;
  $: count = needsYouCount(Object.fromEntries(workspaces.filter(s => agents[s.key]).map(s => [s.key, agents[s.key]])));
</script>
<header class="topbar">
  <button class="secondary menu-button" aria-label="Menu" onclick={onmenu}>☰</button>
  {#if tabsCapable}
    {#if page || !workspace}
      <button type="button" class="secondary page-tabs" data-testid="page-tabs" aria-haspopup="dialog" onclick={ontabmenuopen}>
        <span class="label">Tabs</span><span class="chevron" aria-hidden="true">▾</span>
      </button>
    {:else}
      <TabBar {workspace} {tabs} {activeTab} mobile={!wide} onopen={ontabopen} onmenu={ontabmenuopen} />
      {#if wide && onnewtab}<button class="secondary new-tab" data-testid="strip-new-tab" aria-label="New tab" title="New tab" onclick={onnewtab}>+</button>{/if}
    {/if}
  {:else if strip}
    <TabStrip {workspaces} {agents} {servers} {activeKey} {held} {windows} {expiring} {onopen} {onactions} onmenu={ontabmenu} {onpeek} />
    {#if onnew}<button class="secondary new-workspace" data-testid="strip-new" aria-label="New workspace" title="New workspace" onclick={onnew}>+</button>{/if}
  {:else}<span class="spacer" data-shell="strip-hidden"></span>{/if}
  {#if link}<button class="secondary link" aria-label={`Connection: ${link.text}`} title={link.text} onclick={onlink}><span class="dot" class:ok={link.ok}></span></button>{/if}
  {#if tabsCapable && wide}
    <button class="secondary sidebar-toggle" aria-label="Sidebar" title="Sidebar" onclick={onworkspaces}>
      <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6"><rect x="2.5" y="3.5" width="15" height="13" rx="2" /><path d="M7.5 3.5v13" /></svg>
    </button>
  {:else if !tabsCapable}
    <button class="secondary workspaces-button" aria-label={`Workspaces${count ? ', ' + count + ' need you' : ''}`} aria-expanded={expanded} aria-controls={controls} onclick={onworkspaces}>Workspaces<span class="count" class:quiet={!count} aria-hidden="true">{count || ''}</span></button>
  {/if}
</header>
<style>
  .topbar { display: flex; align-items: center; gap: 6px; min-height: 48px; padding: 2px 6px; background: var(--bg-secondary); border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .menu-button, .workspaces-button, .new-tab, .sidebar-toggle, .page-tabs { min-height: 44px; min-width: 44px; flex-shrink: 0; touch-action: manipulation; }
  .menu-button { padding: 0 12px; }
  .new-tab, .new-workspace { min-height: 44px; min-width: 44px; flex-shrink: 0; padding: 0 8px; font-size: 1.35rem; line-height: 1; touch-action: manipulation; }
  .sidebar-toggle { padding: 0; display: flex; align-items: center; justify-content: center; }
  .page-tabs { display: flex; align-items: center; gap: 6px; flex: 0 1 auto; min-width: 0; }
  .page-tabs .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chevron { color: var(--fg-muted); font-size: 0.7rem; }
  .spacer { flex: 1 1 0; min-width: 0; }
  .link { padding: 0 10px; }
  .dot { display: inline-block; width: 10px; height: 10px; border-radius: 50%; background: var(--danger); }
  .dot.ok { background: var(--success); }
  .workspaces-button { padding: 0 8px; display: flex; align-items: center; gap: 4px; }
  .count { border-radius: 10px; width: 28px; padding: 1px 2px; background: var(--danger); color: var(--bg); font-size: 0.75rem; }
  .count.quiet { visibility: hidden; }
</style>
