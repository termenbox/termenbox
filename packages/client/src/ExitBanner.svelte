<script lang="ts">
  /** One line about an unclean exit, and the way to the diagnostics. Fixed over the app so nothing under a finger moves. */
  export let message: string;
  export let onopen: () => void;
  export let ondismiss: () => void;
</script>

<div class="banner" role="status">
  <span>{message}</span>
  <button on:click={onopen}>Server diagnostics</button>
  <button class="secondary" aria-label="Dismiss" on:click={ondismiss}>×</button>
</div>

<style>
  .banner {
    position: fixed;
    left: 12px;
    right: 12px;
    bottom: calc(var(--safe-bottom, 0px) + 72px);
    z-index: 1400;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    font-size: 0.85rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  span { flex: 1; min-width: 0; }
</style>
