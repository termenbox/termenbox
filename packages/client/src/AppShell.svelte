<script lang="ts">
  import { onMount } from 'svelte';
  import { DEBUG_KEY, debugEnabled, debugText, viewportBox, type Box } from './lib/viewport';

  /** In a workspace the shell is pinned to what is visible; elsewhere the page scrolls normally. */
  export let pinned = false;

  let debug = debugEnabled(location.search, localStorage.getItem(DEBUG_KEY));
  let box: Box = viewportBox(null, window);
  let overlay = '';
  const savedKb = Number(localStorage.getItem('termenbox_kb_height'));
  if (savedKb) document.documentElement.style.setProperty('--kb-height', `${savedKb}px`);

  function measure() {
    const vv = window.visualViewport;
    box = viewportBox(vv, window);
    document.documentElement.style.setProperty('--app-height', `${box.height}px`);
    if (box.keyboard) {
      // The Fn keyboard takes the system keyboard's place at the same height.
      const kb = Math.round(Math.min(340, Math.max(240, window.innerHeight - box.height - box.top)));
      document.documentElement.style.setProperty('--kb-height', `${kb}px`);
      try {
        localStorage.setItem('termenbox_kb_height', String(kb));
      } catch {
        /* private mode */
      }
    }
    if (debug) overlay = debugText(vv, window, box);
  }

  onMount(() => {
    const vv = window.visualViewport;
    const toggle = () => {
      debug = debugEnabled(location.search, localStorage.getItem(DEBUG_KEY));
      measure();
    };
    window.addEventListener('termenbox-debug', toggle);
    measure();
    // iOS fires `scroll` (not `resize`) when it pans the visual viewport to show the focused field.
    vv?.addEventListener('resize', measure);
    vv?.addEventListener('scroll', measure);
    window.addEventListener('resize', measure);
    window.addEventListener('orientationchange', measure);
    return () => {
      window.removeEventListener('termenbox-debug', toggle);
      vv?.removeEventListener('resize', measure);
      vv?.removeEventListener('scroll', measure);
      window.removeEventListener('resize', measure);
      window.removeEventListener('orientationchange', measure);
    };
  });

  $: document.documentElement.classList.toggle('shell-pinned', pinned);
</script>

<div
  class="shell"
  class:pinned
  class:keyboard={box.keyboard}
  style={pinned ? `top: ${box.top}px; left: ${box.left}px; width: ${box.width}px; height: ${box.height}px;` : ''}
>
  <slot />
  {#if debug}<pre class="debug">{overlay}</pre>{/if}
</div>

<style>
  .shell {
    position: relative; /* overlays (toast, dialogs, debug) are placed inside the shell, not the layout viewport */
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    /* Under black-translucent the page draws behind the status bar, so every screen keeps clear of it. */
    padding: var(--safe-top) env(safe-area-inset-right) 0 env(safe-area-inset-left);
    box-sizing: border-box;
  }
  .shell.pinned {
    position: fixed;
    overflow: hidden;
  }
  /* With the keyboard open its own area replaces the home-indicator inset. */
  .shell.pinned:not(.keyboard) {
    padding-bottom: env(safe-area-inset-bottom);
  }
  :global(html.shell-pinned),
  :global(html.shell-pinned body) {
    overflow: hidden;
    overscroll-behavior: none;
  }
  .debug {
    position: absolute;
    top: var(--safe-top);
    right: 0;
    z-index: 2000;
    margin: 0;
    padding: 4px 6px;
    font: 11px/1.3 monospace;
    color: #fff;
    background: rgba(0, 0, 0, 0.75);
    pointer-events: none;
    white-space: pre;
  }
</style>
