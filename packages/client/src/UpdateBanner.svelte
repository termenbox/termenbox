<script lang="ts">
  import type { HubServer } from './lib/hub';
  import { nextStep, type UpdateAction } from './lib/updateState';

  /** Connected servers with their updater state; the banner is about the first one with a release on offer. */
  export let servers: HubServer[];
  export let now: number;
  export let onupdate: (id: string) => void;
  export let onpostpone: (id: string) => void;
  export let onskip: (id: string) => void;
  export let onopen: (id: string) => void;

  let dismissed = '';
  $: target = servers.find(s => s.state === 'connected' && s.updateAvailable && !s.updating && s.update?.phase !== 'installing');
  $: signature = target ? `${target.id}@${target.updateAvailable}@${target.update?.deadline ?? 0}` : '';
  $: show = !!target && dismissed !== signature;
  $: can = (a: UpdateAction) => !!target?.updateCapabilities?.includes(a);
  $: name = target?.server.server_name || 'Server';
</script>

<div class="live" aria-live="polite">
  {#if show && target}
    <div class="banner" role="status" data-testid="update-banner">
      <button type="button" class="what" title="Open Updates" on:click={() => onopen(target.id)}><strong>{name}</strong> · {nextStep(target.update, target.updateAvailable ?? '', now, can)}</button>
      {#if can('postpone') && target.update?.can_postpone}<button type="button" class="secondary" on:click={() => onpostpone(target.id)}>Postpone</button>{/if}
      {#if can('skip')}<button type="button" class="secondary" on:click={() => onskip(target.id)}>Skip</button>{/if}
      <button type="button" class="open" on:click={() => onupdate(target.id)}>Update now</button>
      <button type="button" class="secondary dismiss" aria-label="Dismiss" on:click={() => { dismissed = signature; }}>×</button>
    </div>
  {/if}
</div>

<style>
  .banner { position: absolute; top: 0; left: 0; right: 0; box-sizing: border-box; display: flex; align-items: center; gap: 6px; padding: 2px 8px; min-height: 44px; background: var(--bg-secondary); border-bottom: 2px solid var(--warn); font-size: 0.85rem; }
  .what { background: none; border: 0; color: inherit; text-align: left; font: inherit; padding: 0; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  button { min-height: 40px; min-width: 44px; padding: 0 10px; }
</style>
