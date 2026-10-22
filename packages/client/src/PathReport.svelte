<script lang="ts">
  import { allFailedAdvice, type PathFailure } from './lib/paths';
  /** Every path the last attempt tried, why each failed and one fix (decision 0123). */
  export let failures: PathFailure[];
  /** What to say beyond the list: a server that advertises no way in from outside says so and points to an overlay. */
  $: advice = allFailedAdvice(failures.map((f) => f.path));
</script>
{#if failures.length}
  <div class="paths" data-testid="all-failed" role="group" aria-label="Paths tried">
    <strong>Tried {failures.length === 1 ? '1 path' : `${failures.length} paths`}, none connected</strong>
    <ul>
      {#each failures as f}
        <li>
          <span class="path">{f.path.label} <code>{f.path.kind === 'ws' || f.path.kind === 'tunnel' ? f.path.addr : f.path.port ? (f.path.addr.includes(':') ? `[${f.path.addr}]` : f.path.addr) + ':' + f.path.port : f.path.addr}</code></span>
          <span class="why">{f.reason}</span>
          <span class="fix">{f.fix}</span>
        </li>
      {/each}
    </ul>
    {#if advice}<p class="advice">{advice}</p>{/if}
  </div>
{/if}
<style>
  .paths { margin-top: 8px; font-size: 0.8rem; color: var(--fg-muted); }
  ul { list-style: none; margin: 6px 0 0; padding: 0; display: grid; gap: 8px; }
  li { display: grid; gap: 2px; border-left: 2px solid var(--danger); padding-left: 8px; overflow-wrap: anywhere; }
  .path { color: var(--fg); }
  .fix { color: var(--fg); }
  .advice { margin-top: 8px; color: var(--fg); }
</style>
