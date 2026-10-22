<script lang="ts">
  import type { ActionId, Diagnosis } from '../lib/addServer';

  /** One cause, one primary action; everything else sits in the closed fold (GUARDRAIL errors-fit-one-screen). */
  export let diagnosis: Diagnosis;
  export let onaction: (id: ActionId) => void = () => {};
  /** The result panels that already have their own primary button show the cause and the fold only. */
  export let showAction = true;
</script>

<div class="fail" role="alert" data-testid="failure" data-cause={diagnosis.cause}>
  <strong>{diagnosis.title}</strong>
  {#if showAction}
    <button on:click={() => onaction(diagnosis.action.id)} data-testid="failure-action" style="width: 100%;">{diagnosis.action.label}</button>
  {/if}
  <details data-testid="failure-details">
    <summary>Details</summary>
    <p>{diagnosis.detail}</p>
    <slot />
  </details>
</div>

<style>
  .fail { display: flex; flex-direction: column; gap: 8px; border: 1px solid var(--danger); border-radius: 10px; padding: 10px 12px; }
  summary { cursor: pointer; color: var(--fg-muted); font-size: 0.8rem; }
  details p { color: var(--fg-muted); font-size: 0.8rem; margin: 6px 0; overflow-wrap: anywhere; }
</style>
