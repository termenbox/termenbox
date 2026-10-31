<script lang="ts">
  import { onMount } from 'svelte';
  import type { HubServer } from './lib/hub';
  import type { UpdateAction } from './lib/updateState';
  import { checkIsStale, lastChecked, nextStep } from './lib/updateState';

  /** The server as the hub last published it; the card follows it live, so a check's outcome appears here. */
  export let status: HubServer | undefined;
  export let name: string;
  export let onclose: () => void;
  export let oncheck: () => void;
  export let onupdate: () => void;
  export let onpostpone: () => void;
  export let onskip: () => void;

  let now = Date.now();
  onMount(() => {
    const tick = setInterval(() => { now = Date.now(); }, 30_000);
    return () => clearInterval(tick);
  });

  $: connected = status?.state === 'connected';
  $: can = (a: UpdateAction) => connected && !!status?.updateCapabilities?.includes(a);
  $: info = status?.update;
  $: available = status?.updateAvailable ?? '';
  $: installing = !!status?.updating || info?.phase === 'installing';
  $: stale = checkIsStale(info, now);
  $: version = status?.version ? `v${status.version.split('+')[0]}` : '';
</script>

<div class="backdrop" role="dialog" aria-modal="true" aria-label={`Updates of ${name}`} data-testid="updates-dialog">
  <div class="card">
    <h3>Updates · {name}</h3>
    {#if !connected}
      <p role="status">Connect to this server to see its updates.</p>
    {:else}
      <p class="version" data-testid="updates-version">{version ? `Running ${version}` : 'Running an unknown version'}</p>
      <p class="next" role="status" data-testid="updates-next">{nextStep(info, available, now, can)}</p>
      <p class="checked" data-testid="updates-checked">
        {#if status?.checking}Checking…{:else}{lastChecked(info, now)}{/if}
      </p>
      {#if info?.error}<p class="error" role="alert" data-testid="updates-error">The last check failed: {info.error}</p>{/if}
      {#if stale}<p class="warn" role="alert" data-testid="updates-stale">This server has not checked for releases in over 72 hours.</p>{/if}
    {/if}
    <div class="buttons">
      {#if can('skip') && available && !installing}<button class="secondary" on:click={onskip} data-testid="updates-skip">Skip this version</button>{/if}
      {#if can('postpone') && info?.can_postpone && !installing}<button class="secondary" on:click={onpostpone} data-testid="updates-postpone">Postpone</button>{/if}
      {#if can('check')}<button class="secondary" disabled={status?.checking || installing} on:click={oncheck} data-testid="updates-check">{status?.checking ? 'Checking…' : 'Check now'}</button>{/if}
      {#if connected && available}<button disabled={installing} on:click={onupdate} data-testid="updates-now">{installing ? 'Updating…' : 'Update now'}</button>{/if}
      <button class="secondary" on:click={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 1500; display: flex; align-items: center; justify-content: center; padding: calc(var(--safe-top, 0px) + 12px) 12px 12px; background: rgba(0, 0, 0, 0.6); }
  .card { width: 100%; max-width: 420px; max-height: 100%; overflow-y: auto; padding: 16px; background: var(--bg-secondary); border: 1px solid var(--border); border-radius: 10px; }
  p { margin: 8px 0; font-size: 0.85rem; color: var(--fg-muted); }
  .next { color: var(--fg); }
  .error { color: var(--danger, var(--fg)); }
  .warn { color: var(--warn); }
  .buttons { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin-top: 12px; }
  button { min-height: 44px; min-width: 44px; }
</style>
