<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import BottomBar from './BottomBar.svelte';
  import SideNav from './SideNav.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import ShortcutHelp from './ShortcutHelp.svelte';
  import Resizer from './Resizer.svelte';
  import type { Destination } from '../lib/router';
  import { navGroups } from '../lib/navlist';
  import { buildEntries, isMac, matchShortcut, shortcutAllowed, SHORTCUTS, shortcutsOn, type PaletteEntry } from '../lib/palette';
  import { commitLayout, layout, MIN_CONTENT, patchLayout, sidebarWidthFor, SIDEBAR, toggleSidebar, toggleSplit, WIDE_QUERY } from '../lib/panes';
  import { edgeSwipeBack } from '../lib/edgeswipe';
  import type { AgentMap } from '../lib/agents';
  import type { HubServer, HubWorkspace } from '../lib/hub';
  import type { TabAgentState } from '../lib/protocol';

  /**
   * The arrangement around a screen (decision 0101). Phone (720 px and below): content, then the bottom bar (hidden while
   * the soft keyboard is open inside a workspace). Wider: a docked sidebar you can hide and resize (saved per device) beside
   * the content; with the sidebar hidden the bottom bar carries the destinations. The palette and the shortcut list are the
   * same at both widths. The frame never sizes the app: AppShell does (soft-keyboard-never-hides-input).
   */
  let { current, onnavigate, servers, workspaces, agents, tabStates = {}, activeKey = null, chrome = true, inWorkspace = false, keyboardOpen = false, wide = $bindable(false), helpOpen = $bindable(false),
    canCreate = true, onopen, onserver, onrename, oncloseworkspace, onmove, onaction = () => {}, onshortcut = () => {}, onback, children }: {
    current: Destination; onnavigate: (d: Destination) => void; servers: HubServer[]; workspaces: HubWorkspace[]; agents: AgentMap; tabStates?: Record<string, TabAgentState>; activeKey?: string | null; chrome?: boolean;
    inWorkspace?: boolean; keyboardOpen?: boolean; wide?: boolean; helpOpen?: boolean; canCreate?: boolean; onopen: (key: string) => void; onserver: (id: string) => void;
    onrename?: (key: string, name: string) => void; oncloseworkspace?: (key: string) => void; onmove?: (key: string, target: { key: string; side: 'before' | 'after' }) => void; onaction?: (id: string) => void; onshortcut?: (id: string) => void; onback?: () => void; children: Snippet;
  } = $props();

  let viewport = $state(typeof innerWidth === 'number' ? innerWidth : 1280);
  const sidebarWidth = $derived(sidebarWidthFor($layout.sidebarWidth, viewport));
  const sidebarMax = $derived(Math.max(SIDEBAR.min, Math.min(SIDEBAR.max, viewport - MIN_CONTENT)));
  const sidebarShown = $derived(chrome && wide && $layout.sidebarOpen);

  const mac = isMac();
  const groups = $derived(navGroups(servers, workspaces, agents, tabStates));
  const needsYou = $derived(groups.reduce((n, g) => n + g.workspaces.filter((s) => s.state === 'needs-you').length, 0));
  const entries = $derived(buildEntries({ servers, workspaces, agents, wide, sidebarOpen: $layout.sidebarOpen, splitOpen: $layout.splitOpen, canCreate }));
  const hint = $derived(mac ? SHORTCUTS[0].mac : SHORTCUTS[0].other);
  let paletteOpen = $state(false);
  let returnTo: HTMLElement | null = $state(null);

  function openPalette() {
    const a = document.activeElement;
    returnTo = a instanceof HTMLElement && a !== document.body ? a : null;
    paletteOpen = true;
  }
  function run(e: PaletteEntry) {
    const [kind, ...rest] = e.id.split(':');
    const arg = rest.join(':');
    if (kind === 'dest') onnavigate(arg as Destination);
    else if (kind === 'workspace') onopen(arg);
    else if (kind === 'server') onserver(arg);
    else if (arg === 'toggle-sidebar') toggleSidebar();
    else if (arg === 'toggle-split') toggleSplit();
    else if (arg === 'shortcuts') helpOpen = true;
    else onaction(arg);
  }

  onMount(() => {
    const mq = matchMedia(WIDE_QUERY);
    const media = () => (wide = mq.matches);
    const measure = () => (viewport = innerWidth);
    media();
    mq.addEventListener('change', media);
    addEventListener('resize', measure);
    // Capture phase: the terminal's own key handler would otherwise see the shortcut first. Only the registered
    // shortcuts are taken; every other key is left alone. The palette (Ctrl/Cmd+K) is taken everywhere, even in the
    // terminal. The others (Alt/Option plus a letter) act only when nothing is being typed into, so a field, the
    // composer or the terminal never loses a key (shortcutAllowed).
    const keydown = (e: KeyboardEvent) => {
      if (!chrome || !$shortcutsOn) return;
      const id = matchShortcut(e, mac);
      if (!id || !shortcutAllowed(id, document.activeElement)) return;
      if (id === 'toggle-panes' && !wide) return;
      e.preventDefault();
      e.stopPropagation();
      if (id === 'palette') { if (paletteOpen) paletteOpen = false; else openPalette(); }
      else if (id === 'toggle-panes') toggleSplit();
      else onshortcut(id);
    };
    document.addEventListener('keydown', keydown, true);
    return () => { mq.removeEventListener('change', media); removeEventListener('resize', measure); document.removeEventListener('keydown', keydown, true); };
  });

  let swipe: { id: number; x: number; y: number } | null = null;
  const bar = $derived(chrome && !sidebarShown && !(inWorkspace && keyboardOpen));
</script>

<div class="flex min-h-0 min-w-0 flex-1" data-shell="frame" data-wide={wide} data-bar={bar} data-sidebar={sidebarShown ? 'open' : 'closed'}>
  {#if sidebarShown}
    <SideNav id="workspace-sidebar" width={sidebarWidth} {current} {onnavigate} {groups} {activeKey} {onopen} {onserver} {onrename} {oncloseworkspace} {onmove} onnew={canCreate ? () => onaction('new-workspace') : undefined} onsearch={openPalette} searchHint={hint}
      splitOpen={$layout.splitOpen} onsplit={toggleSplit} />
    <Resizer label="Sidebar width" controls="workspace-sidebar" value={sidebarWidth} min={SIDEBAR.min} max={sidebarMax} def={Math.min(SIDEBAR.def, sidebarMax)} step={SIDEBAR.step}
      onchange={(w) => patchLayout({ sidebarWidth: w }, false)} oncommit={commitLayout} />
  {/if}
  <div class="flex min-h-0 min-w-0 flex-1 flex-col">
    <div class="relative flex min-h-0 min-w-0 flex-1">
      <div class="flex min-h-0 min-w-0 flex-1 flex-col" data-shell="main">{@render children()}</div>
      {#if onback && inWorkspace && !wide}
        <!-- Swipe-back: a drag from the left edge opens the workspace list. Not a control: taps here fall through to nothing. -->
        <div aria-hidden="true" data-shell="edge" class="absolute inset-y-0 left-0 z-40 w-5 touch-none"
          onpointerdown={(e) => (swipe = { id: e.pointerId, x: e.clientX, y: e.clientY })}
          onpointerup={(e) => { if (swipe?.id === e.pointerId && edgeSwipeBack(swipe, { x: e.clientX, y: e.clientY })) onback?.(); swipe = null; }}
          onpointercancel={() => (swipe = null)}></div>
      {/if}
    </div>
    {#if bar}<BottomBar {current} {onnavigate} {needsYou} />{/if}
  </div>
  {#if chrome}
    <CommandPalette bind:open={paletteOpen} {entries} onrun={run} {returnTo} />
    {#if helpOpen}<ShortcutHelp mac={mac} enabled={$shortcutsOn} onclose={() => (helpOpen = false)} />{/if}
  {/if}
</div>
