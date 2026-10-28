<script lang="ts">
  import { bundleFileName, incidentPlan } from './lib/diagnostics';

  /** The bundle once the server has sent every part; null while it is asked for. */
  export let text: string | null;
  /** Why there is none (a non-owner, an older server), shown instead of the bundle. */
  export let error = '';
  export let onclose: () => void;

  let what = '';

  function download() {
    if (text === null) return;
    const url = URL.createObjectURL(new Blob([text], { type: 'text/plain' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = bundleFileName();
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  }

  function openIssue() {
    if (text === null) return;
    const plan = incidentPlan(text, what);
    window.open(plan.url, '_blank', 'noopener');
    onclose();
  }
</script>

<div class="backdrop" role="dialog" aria-modal="true" aria-label="Server diagnostics">
  <div class="card">
    <h3>Server diagnostics</h3>
    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if text === null}
      <p role="status">Asking the server…</p>
    {:else}
      <p>Read this first. It comes from your server: recent log lines, the newest crash file, versions and health. Certificates, secrets, tokens, addresses and ports are removed; terminal content is never included. Nothing is sent until you tap a button below.</p>
      <textarea class="diagnostics" bind:value={text} rows="10" spellcheck="false" aria-label="Diagnostics text"></textarea>
      <label>What were you doing?<textarea bind:value={what} rows="2" placeholder="Optional" spellcheck="true"></textarea></label>
    {/if}
    <div class="buttons">
      <button class="secondary" on:click={onclose}>{text === null ? 'Cancel' : 'Close'}</button>
      {#if text !== null}
        <button class="secondary" on:click={download}>Download</button>
        <button on:click={openIssue}>Open GitHub issue</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: absolute;
    inset: 0;
    z-index: 1500;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(var(--safe-top) + 12px) 12px 12px;
    background: rgba(0, 0, 0, 0.6);
  }
  .card {
    width: 100%;
    max-width: 520px;
    max-height: 100%;
    overflow-y: auto;
    padding: 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  p { margin: 8px 0; font-size: 0.8rem; color: var(--fg-muted); }
  .error { color: var(--danger, var(--fg)); }
  label { display: block; margin-top: 8px; font-size: 0.8rem; color: var(--fg-muted); }
  textarea {
    width: 100%;
    font: 12px/1.4 monospace;
    color: var(--fg);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px;
    font-size: 16px; /* no iOS zoom on focus */
  }
  label textarea { margin-top: 2px; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 10px; }
</style>
