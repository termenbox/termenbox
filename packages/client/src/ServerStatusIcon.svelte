<script lang="ts">
  import type { HubServer } from './lib/hub';
  /**
   * The server's state as one icon, left of its name (decision 0101). The word is in `aria-label` and `title`, never in
   * the row: connecting is a turning arc, connected a filled dot, checking a triangle with !, reconnecting an arrow,
   * unreachable a broken link, closed a circle with ×, paused a circle with two bars.
   */
  let { state, name = '' }: { state: HubServer['state']; name?: string } = $props();
  const WORDS: Record<HubServer['state'], string> = {
    connecting: 'Connecting', connected: 'Connected', checking: 'Checking', reconnecting: 'Reconnecting',
    unreachable: 'Unreachable', closed: 'Closed', paused: 'Paused',
  };
  const label = $derived(`${name ? name + ': ' : ''}${WORDS[state]}`);
</script>

<span class="state state-{state}" role="img" aria-label={label} title={label}>
  {#if state === 'connected'}
    <span class="dot filled" aria-hidden="true"></span>
  {:else if state === 'connecting'}
    <span class="spinner" aria-hidden="true"></span>
  {:else if state === 'checking'}
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"><path d="M8 1.8 15 14H1Z" /><path d="M8 6v4" stroke-linecap="round" /><circle cx="8" cy="12" r=".7" fill="currentColor" stroke="none" /></svg>
  {:else if state === 'reconnecting'}
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M13.5 6.5A6 6 0 0 0 3 5M2.5 9.5A6 6 0 0 0 13 11" /><path d="M13.5 2.5v4h-4M2.5 13.5v-4h4" /></svg>
  {:else if state === 'unreachable'}
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M2 8h4M10 8h4M3 5.5v5M13 5.5v5" /><path d="m2.5 13.5 11-11" /></svg>
  {:else if state === 'closed'}
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><circle cx="8" cy="8" r="6.2" /><path d="m6 6 4 4m0-4-4 4" /></svg>
  {:else}
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><circle cx="8" cy="8" r="6.2" /><path d="M6.5 5.5v5M9.5 5.5v5" /></svg>
  {/if}
</span>

<style>
  .state { display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; flex-shrink: 0; }
  .dot { width: 10px; height: 10px; border-radius: 50%; border: 1.5px solid currentColor; box-sizing: border-box; }
  .dot.filled { background: currentColor; }
  .spinner { width: 12px; height: 12px; border-radius: 50%; border: 2px solid color-mix(in oklab, currentColor 30%, transparent); border-top-color: currentColor; animation: turn 0.9s linear infinite; }
  .state-connected { color: var(--success); }
  .state-connecting, .state-checking, .state-reconnecting { color: var(--warn); }
  .state-unreachable, .state-closed { color: var(--danger); }
  .state-paused { color: var(--fg-muted); }
  @keyframes turn { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spinner { animation: none; } }
</style>
