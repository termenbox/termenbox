<script lang="ts">
  import type { ClaimPreview } from '../lib/onboard';
  import type { PairPreview } from '../lib/pair';
  import { describeCandidates } from '../lib/pair';

  /** Step 3: the 0117 preview of what is being added, and its name. Nothing is claimed until Add is tapped. */
  export let claim: ClaimPreview | null = null;
  export let pair: PairPreview | null = null;
  export let name: string;
  export let onconfirm: () => void;
</script>

<div class="preview" role="group" aria-label="What you are adding">
  <strong>This server is ready to add</strong>
  <dl>
    {#if claim}
      <dt>Runs sessions</dt>
      <dd>{claim.mode === 'container' ? 'in an isolated container' : `on the machine itself, as ${claim.run_user || 'its own user'}`}</dd>
      <dt>Version</dt>
      <dd>{claim.version}</dd>
    {:else if pair}
      <dt>Name</dt>
      <dd data-testid="pair-name">{pair.name}</dd>
      <dt>Fingerprint</dt>
      <dd class="mono" data-testid="pair-fingerprint">{pair.fingerprint.replace('sha-256 ', '')}</dd>
      <dt>Reachable by</dt>
      <dd>{describeCandidates(pair.candidates)}</dd>
    {/if}
  </dl>
  <label for="preview-name">Name</label>
  <input id="preview-name" bind:value={name} maxlength="40" style="width: 100%; margin-bottom: 8px;" />
  <p class="note">
    {#if pair}Compare the fingerprint with the one the server printed. {/if}{#if pair && !pair.claim}Tap Add to add it to this device.{:else}Nothing is claimed yet. Tap Add to become this server's owner; closing this leaves the link valid until it expires.{/if}
  </p>
  <button on:click={onconfirm} style="width: 100%;" data-testid="pair-confirm">Add</button>
</div>

<style>
  .preview { border: 1px solid var(--border); border-radius: 10px; padding: 12px; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; margin: 8px 0 12px; font-size: 0.875rem; }
  dt { color: var(--fg-muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .mono { font-family: monospace; font-size: 0.7rem; }
  label { display: block; font-size: 0.875rem; margin-bottom: 6px; color: var(--fg-muted); }
  .note { color: var(--fg-muted); font-size: 0.8rem; margin: 0 0 10px; }
</style>
