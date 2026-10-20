<script lang="ts">
  import TabStrip from './TabStrip.svelte';
  import { needsYouCount } from './lib/tabs';
  import type { AgentMap } from './lib/agents';
  import type { HubServer, HubSession } from './lib/hub';
  import type { WindowMap } from './lib/windows';
  export let sessions: HubSession[];
  export let agents: AgentMap;
  export let servers: HubServer[];
  export let activeKey: string | null;
  export let held = false;
  export let onopen: (key: string) => void;
  export let onactions: (key: string) => void;
  export let windows: WindowMap = {};
  export let expiring: Set<string> = new Set();
  /** The selected server's link: its state and the quality text (`direct · 42 ms`) the terminal's status bar used to show. */
  export let link: { ok: boolean; text: string } | null = null;
  export let onlink: () => void = () => {};
  export let onmenu: () => void;
  export let ontabmenu: (key: string) => void = () => {};
  export let onpeek: (key: string) => void = () => {};
  export let onsessions: () => void;
  /** Whether what the Sessions button opens is open: the docked sidebar on the desktop, the sheet on a phone. */
  export let expanded: boolean | undefined = undefined;
  export let controls: string | undefined = undefined;
  $: count = needsYouCount(Object.fromEntries(sessions.filter(s => agents[s.key]).map(s => [s.key, agents[s.key]])));
</script>
<header class="topbar">
  <button class="secondary menu-button" aria-label="Menu" on:click={onmenu}>☰</button>
  <TabStrip {sessions} {agents} {servers} {activeKey} {held} {windows} {expiring} {onopen} {onactions} onmenu={ontabmenu} {onpeek} />
  {#if link}<button class="secondary link" aria-label={`Connection: ${link.text}`} title={link.text} on:click={onlink}><span class="dot" class:ok={link.ok}></span></button>{/if}
  <button class="secondary sessions-button" aria-label={`Sessions${count ? ', ' + count + ' need you' : ''}`} aria-expanded={expanded} aria-controls={controls} on:click={onsessions}>Sessions<span class="count" class:quiet={!count} aria-hidden="true">{count || ''}</span></button>
</header>
<style>
  .topbar { display: flex; align-items: center; gap: 6px; min-height: 48px; padding: 2px 6px; background: var(--bg-secondary); border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .menu-button, .sessions-button { min-height: 44px; min-width: 44px; flex-shrink: 0; touch-action: manipulation; }
  .menu-button { padding: 0 12px; }
  .link { padding: 0 10px; }
  .dot { display: inline-block; width: 10px; height: 10px; border-radius: 50%; background: var(--danger); }
  .dot.ok { background: var(--success); }
  .sessions-button { padding: 0 8px; display: flex; align-items: center; gap: 4px; }
  .count { border-radius: 10px; width: 28px; padding: 1px 2px; background: var(--danger); color: var(--bg); font-size: 0.75rem; }
  .count.quiet { visibility: hidden; }
</style>
