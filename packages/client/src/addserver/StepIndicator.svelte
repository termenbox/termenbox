<script lang="ts">
  import { STEPS, type Step } from '../lib/addServer';

  export let step: Step;
  const names: Record<Step, string> = { install: 'Install', connect: 'Connect', confirm: 'Confirm', done: 'Done' };
  $: at = STEPS.indexOf(step);
</script>

<ol class="steps" aria-label="Steps" data-testid="steps">
  {#each STEPS as st, i}
    <li aria-current={i === at ? 'step' : undefined} class:done={i < at} class:now={i === at}>{i + 1} {names[st]}</li>
  {/each}
</ol>

<style>
  .steps { display: flex; gap: 4px; list-style: none; padding: 0; margin: 0 0 10px; font-size: 0.75rem; color: var(--fg-muted); }
  li { flex: 1; text-align: center; padding: 4px 0; border-bottom: 2px solid var(--border); white-space: nowrap; }
  li.done { border-color: var(--fg-muted); }
  li.now { color: var(--fg); border-color: var(--accent, var(--fg)); font-weight: 600; }
</style>
