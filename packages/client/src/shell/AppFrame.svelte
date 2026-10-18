<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import BottomBar from './BottomBar.svelte';
  import SideNav from './SideNav.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import ShortcutHelp from './ShortcutHelp.svelte';
  import type { Destination } from '../lib/router';
  import { navGroups } from '../lib/navlist';
  import { buildEntries, isMac, matchShortcut, SHORTCUTS, shortcutsOn, type PaletteEntry } from '../lib/palette';
  import { edgeSwipeBack } from '../lib/edgeswipe';
  import type { AgentMap } from '../lib/agents';
  import type { HubServer, HubSession } from '../lib/hub';

  /**
   * The arrangement around a screen (decision 0101). Phone: content, then the bottom bar (hidden while the soft keyboard
   * is open inside a session). From 1024 px: sidebar, content, and an optional right pane. The palette and the shortcut
   * list are the same at both widths. The frame never sizes the app: AppShell does (soft-keyboard-never-hides-input).
   */
  let { current, onnavigate, servers, sessions, agents, activeKey = null, chrome = true, inSession = false, keyboardOpen = false, wide = $bindable(false), rightOpen = $bindable(false), helpOpen = $bindable(false),
    canCreate = true, onopen, onserver, onaction = () => {}, onshortcut = () => {}, onback, children, right }: {
    current: Destination; onnavigate: (d: Destination) => void; servers: HubServer[]; sessions: HubSession[]; agents: AgentMap; activeKey?: string | null; chrome?: boolean;
    inSession?: boolean; keyboardOpen?: boolean; wide?: boolean; rightOpen?: boolean; helpOpen?: boolean; canCreate?: boolean; onopen: (key: string) => void;     onserver: (id: string) => void;
    onaction?: (id: string) => void; onshortcut?: (id: string) => void; onback?: () => void; children: Snippet; right?: Snippet;
  } = $props();

  /** Sidebar width in px, persisted so a reload restores it. Clamped so the terminal stays usable. */
  const SIDEBAR_KEY = 'termenbox_sidebar_width';
  const SIDEBAR_MIN = 200;
  const SIDEBAR_MAX = 520;
  let sidebarWidth = $state(typeof localStorage !== 'undefined' ? Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Number(localStorage.getItem(SIDEBAR_KEY)) || 260)) : 260);
  let dragging = $state(false);
  const saveSidebarWidth = () => { try { localStorage.setItem(SIDEBAR_KEY, String(sidebarWidth)); } catch {} };

  /** Right pane width in px, persisted so a reload restores it. Clamped so the terminal stays usable. */
  const RIGHT_KEY = 'termenbox_right_width';
  const RIGHT_MIN = 240;
  const RIGHT_MAX = 520;
  let rightWidth = $state(typeof localStorage !== 'undefined' ? Math.min(RIGHT_MAX, Math.max(RIGHT_MIN, Number(localStorage.getItem(RIGHT_KEY)) || 320)) : 320);
  let rightDragging = $state(false);
  const saveRightWidth = () => { try { localStorage.setItem(RIGHT_KEY, String(rightWidth)); } catch {} };

  const mac = isMac();
  const groups = $derived(navGroups(servers, sessions, agents));
  const needsYou = $derived(groups.reduce((n, g) => n + g.sessions.filter((s) => s.state === 'needs-you').length, 0));
  const entries = $derived(buildEntries({ servers, sessions, agents, wide, rightOpen, canCreate }));
  const hint = $derived(mac ? SHORTCUTS[0].mac : SHORTCUTS[0].other);
  let paletteOpen = $state(false);
  let returnTo: HTMLElement | null = $state(null);

  /** Pointer-drag resize of the sidebar. Clamped so the terminal keeps room. */
  const startSidebarResize = (e: PointerEvent) => {
    e.preventDefault();
    dragging = true;
    const startX = e.clientX;
    const startWidth = sidebarWidth;
    const move = (ev: PointerEvent) => {
      let next = startWidth + (ev.clientX - startX);
      next = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, next));
      if (next !== sidebarWidth) { sidebarWidth = next; saveSidebarWidth(); }
    };
    const stop = () => {
      dragging = false;
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop, { once: true });
    window.addEventListener('pointercancel', stop, { once: true });
  };
  /** Keyboard resize while the separator has focus: arrows step, Home resets. */
  const resizeKeydown = (e: KeyboardEvent) => {
    if (e.key === 'ArrowLeft') { e.preventDefault(); sidebarWidth = Math.max(SIDEBAR_MIN, sidebarWidth - 20); saveSidebarWidth(); }
    else if (e.key === 'ArrowRight') { e.preventDefault(); sidebarWidth = Math.min(SIDEBAR_MAX, sidebarWidth + 20); saveSidebarWidth(); }
    else if (e.key === 'Home') { e.preventDefault(); sidebarWidth = 260; saveSidebarWidth(); }
    else if (e.key === 'End') { sidebarWidth = SIDEBAR_MAX; saveSidebarWidth(); }
  };

  const startRightResize = (e: PointerEvent) => {
    e.preventDefault();
    rightDragging = true;
    const startX = e.clientX;
    const startWidth = rightWidth;
    const move = (ev: PointerEvent) => {
      let next = startWidth + (ev.clientX - startX);
      next = Math.max(RIGHT_MIN, Math.min(RIGHT_MAX, next));
      if (next !== rightWidth) { rightWidth = next; saveRightWidth(); }
    };
    const stop = () => {
      rightDragging = false;
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop, { once: true });
    window.addEventListener('pointercancel', stop, { once: true });
  };
  const rightResizeKeydown = (e: KeyboardEvent) => {
    if (e.key === 'ArrowLeft') { e.preventDefault(); rightWidth = Math.max(RIGHT_MIN, rightWidth - 20); saveRightWidth(); }
    else if (e.key === 'ArrowRight') { e.preventDefault(); rightWidth = Math.min(RIGHT_MAX, rightWidth + 20); saveRightWidth(); }
    else if (e.key === 'Home') { e.preventDefault(); rightWidth = 320; saveRightWidth(); }
    else if (e.key === 'End') { rightWidth = RIGHT_MAX; saveRightWidth(); }
  };

  function openPalette() {
    const a = document.activeElement;
    returnTo = a instanceof HTMLElement && a !== document.body ? a : null;
    paletteOpen = true;
  }
  function run(e: PaletteEntry) {
    const [kind, ...rest] = e.id.split(':');
    const arg = rest.join(':');
    if (kind === 'dest') onnavigate(arg as Destination);
    else if (kind === 'session') onopen(arg);
    else if (kind === 'server') onserver(arg);
    else if (arg === 'toggle-right') rightOpen = !rightOpen;
    else if (arg === 'shortcuts') helpOpen = true;
    else onaction(arg);
  }

  onMount(() => {
    const mq = matchMedia('(min-width: 1024px)');
    const media = () => (wide = mq.matches);
    media();
    mq.addEventListener('change', media);
    // Capture phase: the terminal's own key handler would otherwise see the shortcut first. Only the registered
    // shortcuts are taken; every other key is left alone. The palette is taken everywhere (even in the terminal);
    // pane and decision shortcuts pass through when the terminal has focus, so they never clash with terminal input.
    const keydown = (e: KeyboardEvent) => {
      if (!chrome || !$shortcutsOn) return;
      const id = matchShortcut(e, mac);
      if (!id) return;
      const active = document.activeElement;
      const inTerminal = active?.closest('.xterm, .xterm-helper-textarea');
      if (id !== 'palette' && inTerminal) return;
      e.preventDefault();
      e.stopPropagation();
      if (id === 'palette') { if (paletteOpen) paletteOpen = false; else openPalette(); }
      else onshortcut(id);
    };
    document.addEventListener('keydown', keydown, true);
    return () => { mq.removeEventListener('change', media); document.removeEventListener('keydown', keydown, true); };
  });

  let swipe: { id: number; x: number; y: number } | null = null;
  const bar = $derived(chrome && !wide && !(inSession && keyboardOpen));
</script>

<div class="flex min-h-0 min-w-0 flex-1" data-shell="frame" data-wide={wide} data-bar={bar}>
  {#if chrome && wide}
    <SideNav style="width:{sidebarWidth}px;min-width:{sidebarWidth}px;max-width:{sidebarWidth}px;flex-shrink:0" {current} {onnavigate} {groups} {activeKey} {onopen} {onserver} onnew={canCreate ? () => onaction('new-session') : undefined} onsearch={openPalette} searchHint={hint} {rightOpen} onright={() => (rightOpen = !rightOpen)} />
    <div
      class="sidebar-resize"
      role="separator"
      aria-orientation="vertical"
      aria-valuenow={sidebarWidth}
      aria-label="Resize sidebar"
      tabindex="0"
      data-dragging={dragging}
      style="width:6px;cursor:col-resize;z-index:20;user-select:none;background:{dragging ? 'var(--accent)' : 'var(--border)'};"
      onpointerdown={startSidebarResize}
      onkeydown={resizeKeydown}
    ></div>
  {/if}
  <div class="flex min-h-0 min-w-0 flex-1 flex-col">
    <div class="relative flex min-h-0 min-w-0 flex-1">
      <div class="flex min-h-0 min-w-0 flex-1 flex-col" data-shell="main">{@render children()}</div>
      {#if chrome && wide && rightOpen && right}
        <div
          class="right-pane-resize"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize right pane"
          tabindex="0"
          data-dragging={rightDragging}
          style="width:6px;cursor:col-resize;z-index:20;user-select:none;background:{rightDragging ? 'var(--accent)' : 'var(--border)'};"
          onpointerdown={startRightResize}
          onkeydown={rightResizeKeydown}
        ></div>
        <aside aria-label="Right pane" data-shell="right-pane" style="width:{rightWidth}px;min-width:{rightWidth}px;max-width:{rightWidth}px;flex-shrink:0" class="flex shrink-0 flex-col overflow-hidden border-l border-line bg-panel">{@render right()}</aside>
      {/if}
      {#if onback && inSession && !wide}
        <!-- Swipe-back: a drag from the left edge opens the session list. Not a control: taps here fall through to nothing. -->
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

<style>
  .sidebar-resize:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .right-pane-resize:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
</style>
