<script lang="ts">
  import { onMount } from 'svelte';
  import { Dialog } from './lib/ui';
  import { describeCandidates, pairedServer, previewPair, type PairLink, type PairResult } from './lib/pair';
  import type { ServerListItem } from './lib/protocol';

  /** A pairing link from `termenbox-server --print-pairing` (decision 0123). */
  export let link: PairLink;
  export let onclose: () => void;
  export let onadd: (server: ServerListItem) => void;

  let result: PairResult | null = null;
  let name = '';

  onMount(async () => {
    result = await previewPair(link);
    if (result.ok) name = result.preview.name;
  });

  function confirm() {
    if (result?.ok) onadd(pairedServer(result.preview, name.trim() || result.preview.name));
  }
</script>

<Dialog title="Pair a server" {onclose}>
  {#if !result}
    <p class="note">Checking the link…</p>
  {:else if !result.ok}
    <div class="bad" role="alert" data-testid="pair-invalid">
      <strong>Invalid link</strong>
      <p>
        {#if result.reason === 'expired'}
          This pairing link has expired. Run <code>termenbox-server --print-pairing</code> on the server for a new one.
        {:else}
          This pairing link did not verify: it is damaged, cut short or was changed on the way. Nothing was added. Run <code>termenbox-server --print-pairing</code> on the server and scan or open the new link.
        {/if}
      </p>
      <button on:click={onclose} style="width: 100%;">Close</button>
    </div>
  {:else}
    <div class="preview" role="group" aria-label="What you are pairing" data-testid="pair-preview">
      <strong>Check that this is your server</strong>
      <dl>
        <dt>Name</dt>
        <dd data-testid="pair-name">{result.preview.name}</dd>
        <dt>Fingerprint</dt>
        <dd class="mono" data-testid="pair-fingerprint">{result.preview.fingerprint.replace('sha-256 ', '')}</dd>
        <dt>Reachable by</dt>
        <dd>{describeCandidates(result.preview.candidates)}</dd>
      </dl>
      <label for="pair-name-input" class="label">Name on this device</label>
      <input id="pair-name-input" bind:value={name} maxlength="40" style="width: 100%; margin-bottom: 12px;" />
      <p class="note">Compare the fingerprint with the one the server printed. The link is signed by the server's own key, which proves nothing about who made it: add it only if it came from you.{#if result.preview.claim} It also claims the server for you when you connect.{/if}</p>
      <button on:click={confirm} style="width: 100%;" data-testid="pair-confirm">Add and connect</button>
    </div>
  {/if}
</Dialog>

<style>
  .preview, .bad { border: 1px solid var(--border); border-radius: 10px; padding: 12px; margin: 0 0 12px; }
  .bad { border-color: var(--danger); }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; margin: 8px 0 12px; font-size: 0.875rem; }
  dt { color: var(--fg-muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .mono { font-family: monospace; font-size: 0.75rem; }
  .label { display: block; font-size: 0.875rem; margin-bottom: 6px; color: var(--fg-muted); }
  .note { color: var(--fg-muted); font-size: 0.8rem; margin: 0 0 12px; }
  p { margin: 8px 0; }
</style>
