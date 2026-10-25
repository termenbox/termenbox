<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { FrozenOrder, chipFor } from './lib/cards';
  import { needsYou, tabIds } from './lib/tabs';
  import { representativeWindow, tabTexts, type TabText, type WindowMap } from './lib/windows';
  import AgentIcon from './AgentIcon.svelte';
  import type { AgentMap } from './lib/agents';
  import type { HubWorkspace, HubServer } from './lib/hub';

  export let workspaces: HubWorkspace[] = [];
  export let agents: AgentMap = {};
  export let servers: HubServer[] = [];
  export let activeKey: string | null = null;
  export let held = false;
  export let windows: WindowMap = {};
  /** Workspaces whose prompt cache goes cold within a minute: the tab gets a thin ring. */
  export let expiring: Set<string> = new Set();
  export let onopen: (key: string) => void;
  /** Tapping the tab that is already open lists the windows of its workspace. */
  export let onmenu: (key: string) => void = () => {};
  export let onpeek: (key: string) => void = () => {};
  export let onactions: (key: string) => void;

  const frozen = new FrozenOrder();
  let now = Date.now();
  let revision = 0;
  let shown: HubWorkspace[] = [];
  let live = new Set<string>();
  let strip: HTMLDivElement;
  let visibleActive: string | null = null;
  let press: { pointerId: number; key: string; x: number; y: number; timer: ReturnType<typeof setTimeout> } | null = null;
  let longPressed: string | null = null;
  let left = false;
  let right = false;

  // Labels and icons follow the shell's directory and the agent window, but never change under a finger.
  let texts: Record<string, TabText> = {};
  let shownWindows: WindowMap = {};
  $: if (!frozen.frozen(now) || !Object.keys(shownWindows).length) { shownWindows = windows; }
  $: texts = tabTexts(workspaces, shownWindows);

  let wasHeld = false;
  $: if (held !== wasHeld) { frozen.sheet(held); wasHeld = held; revision++; }
  $: {
    revision;
    const order = frozen.order(tabIds(workspaces, agents), {}, now);
    // A tab opened during a hold still gets a tab; a removed one stays in place (disabled) until the hold ends.
    if (activeKey && !order.includes(activeKey) && workspaces.some(s => s.key === activeKey)) order.push(activeKey);
    live = new Set(workspaces.map(s => s.key));
    const known = new Map([...shown, ...workspaces].map(s => [s.key, s]));
    shown = order.map(key => known.get(key)).filter((s): s is HubWorkspace => !!s);
  }
  $: if (strip && activeKey && activeKey !== visibleActive && !frozen.frozen(now)) scrollActive(activeKey, shown);

  async function scrollActive(key: string, _shown: HubWorkspace[]) {
    await tick();
    if (key !== activeKey || frozen.frozen(Date.now())) return;
    const tab = [...strip.querySelectorAll<HTMLButtonElement>('[role=tab]')].find(el => el.dataset.key === key);
    if (!tab) return;
    const a = tab.getBoundingClientRect(), b = strip.getBoundingClientRect();
    if (a.left < b.left) strip.scrollLeft -= b.left - a.left;
    else if (a.right > b.right) strip.scrollLeft += a.right - b.right;
    visibleActive = key;
    edges();
  }
  function edges() { if (strip) { left = strip.scrollLeft > 1; right = strip.scrollLeft + strip.clientWidth < strip.scrollWidth - 1; } }
  function down(e: PointerEvent, key: string) {
    frozen.touchDown(e.pointerId); revision++;
    if (key !== activeKey) onpeek(key); // the screen is fetched while the finger is still going down (decision 0084)
    cancelPress();
    longPressed = null;
    press = { pointerId: e.pointerId, key, x: e.clientX, y: e.clientY, timer: setTimeout(() => {
      longPressed = key; onactions(key); press = null;
    }, 550) };
  }
  function cancelPress() { if (press) clearTimeout(press.timer); press = null; }
  function move(e: PointerEvent) { if (press && press.pointerId === e.pointerId && Math.hypot(e.clientX - press.x, e.clientY - press.y) > 10) cancelPress(); }
  function up(e: PointerEvent) {
    if (!frozen.hasPointer(e.pointerId)) return;
    if (press?.pointerId === e.pointerId) cancelPress();
    if (e.type === 'pointercancel') longPressed = null;
    // Click follows pointerup: keep the target stable through that click.
    setTimeout(() => { frozen.touchUp(e.pointerId); revision++; }, 0);
  }
  onMount(() => {
    const timer = setInterval(() => { now = Date.now(); edges(); }, 250);
    const observer = new ResizeObserver(() => { visibleActive = null; now = Date.now(); edges(); }); observer.observe(strip); edges();
    window.addEventListener('pointerup', up); window.addEventListener('pointercancel', up);
    return () => { clearInterval(timer); observer.disconnect(); cancelPress(); window.removeEventListener('pointerup', up); window.removeEventListener('pointercancel', up); };
  });
</script>

<div class="tabs" class:left class:right>
  <div class="strip" bind:this={strip} role="tablist" aria-label="Workspaces across servers" on:scroll={() => { frozen.scrolled(Date.now()); cancelPress(); edges(); }}>
    {#each shown as workspace (workspace.key)}
      {@const agent = agents[workspace.key]}
      {@const chip = agent ? chipFor(agent, now) : { icon: '·', label: 'No status', tone: 'none' }}
      {@const server = servers.find(s => s.id === workspace.serverId)}
      {@const rep = representativeWindow(shownWindows[workspace.key])}
      {@const text = texts[workspace.key] ?? { label: workspace.name, hint: workspace.name }}
      <button type="button" role="tab" data-key={workspace.key} aria-selected={activeKey === workspace.key}
        aria-label={`${text.label}${servers.length > 1 ? ' · ' + workspace.serverName : ''} · ${agent ? chip.label : 'No status'}${server && server.state !== 'connected' ? ' · ' + server.state + (server.reason ? ': ' + server.reason : '') : ''}`}
        title={text.hint} class:ring={expiring.has(workspace.key)} class:active={activeKey === workspace.key} class:degraded={server?.state !== 'connected'} disabled={!live.has(workspace.key)}
        on:mousedown|preventDefault on:pointerdown={(e) => down(e, workspace.key)} on:pointermove={move}
        on:click={(e) => {
          const suppress = e.detail > 0 && longPressed === workspace.key;
          longPressed = null;
          if (suppress) return;
          if (workspace.key === activeKey) onmenu(workspace.key); else onopen(workspace.key);
        }}
        on:contextmenu|preventDefault={() => onactions(workspace.key)}>
        <span class="mark"><AgentIcon command={rep?.command} agent={rep ? rep.agent : agent?.agent} size={22} /><span class="state-icon state-{chip.tone}" aria-hidden="true">{chip.icon}</span></span><span class="workspace-name">{text.label}</span>
        {#if servers.length > 1}<span class="badge">{workspace.serverName}</span>{/if}
        <span class="need" aria-hidden="true">{needsYou(agent) ? '●' : ''}</span>
        <span class="sr">{chip.label}</span>
      </button>
    {/each}
    {#if !shown.length}<span class="empty">No workspaces</span>{/if}
  </div>
</div>

<style>
  .tabs { flex: 1; min-width: 0; position: relative; }
  .tabs::before, .tabs::after { content: ''; position: absolute; top: 0; bottom: 0; width: 14px; pointer-events: none; z-index: 1; opacity: 0; }
  .tabs::before { left: 0; background: linear-gradient(to right, var(--bg-secondary), transparent); }
  .tabs::after { right: 0; background: linear-gradient(to left, var(--bg-secondary), transparent); }
  .tabs.left::before, .tabs.right::after { opacity: 1; }
  .strip { display: flex; gap: 4px; overflow-x: auto; scrollbar-width: none; overscroll-behavior-x: contain; touch-action: pan-x; }
  .strip::-webkit-scrollbar { display: none; }
  button { flex: 0 0 auto; max-width: 100%; min-height: 44px; min-width: 44px; display: flex; align-items: center; gap: 6px; white-space: nowrap; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  button.active { border-color: var(--accent); }
  button.degraded { color: var(--fg-muted); border-style: dashed; }
  .workspace-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .badge { font-size: 0.65rem; background: var(--border); border-radius: 4px; padding: 2px 4px; max-width: 64px; overflow: hidden; text-overflow: ellipsis; flex-shrink: 0; }
  .mark { position: relative; display: inline-flex; flex-shrink: 0; }
  .state-icon { position: absolute; right: -6px; bottom: -6px; min-width: 14px; height: 14px; border-radius: 7px; background: var(--bg); border: 1px solid var(--border); font-size: 0.6rem; line-height: 12px; text-align: center; }
  .state-need { border-color: var(--danger); }
  button.ring { box-shadow: 0 0 0 2px var(--accent) inset; }
  .need { color: var(--danger); width: 8px; flex-shrink: 0; }
  .empty { padding: 12px; color: var(--fg-muted); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
</style>
