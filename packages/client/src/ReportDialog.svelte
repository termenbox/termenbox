<script lang="ts">
  import { describeProblem, reportPlan } from './lib/report';

  export let text: string;
  export let onclose: () => void;

  let steps = '';
  let actual = '';
  let expected = '';

  async function open() {
    const plan = reportPlan(text, describeProblem({ steps, actual, expected }));
    if (plan.copy) {
      try {
        await navigator.clipboard.writeText(plan.copy);
      } catch {
        /* the text stays on screen to copy by hand */
      }
    }
    window.open(plan.url, '_blank', 'noopener');
    onclose();
  }
</script>

<div class="backdrop" role="dialog" aria-modal="true" aria-label="Report a problem">
  <div class="card">
    <h3>Report a problem</h3>
    <p>Describe it here first: GitHub opens over the app, so you will not be able to look back at it.</p>
    <label>Steps to reproduce<textarea bind:value={steps} rows="3" placeholder="1. Open a workspace&#10;2. Tap the Workspaces tab" spellcheck="true"></textarea></label>
    <label>Actual result<textarea bind:value={actual} rows="2" placeholder="What happened" spellcheck="true"></textarea></label>
    <label>Expected result<textarea bind:value={expected} rows="2" placeholder="What should have happened" spellcheck="true"></textarea></label>
    <p>This opens a new issue on GitHub. Read and edit the diagnostics below first: it has the app version, browser, screen numbers, connection state and recent error messages. Certificates, secrets, tokens and addresses are removed; terminal content is never included.</p>
    <textarea class="diagnostics" bind:value={text} rows="6" spellcheck="false"></textarea>
    <div class="buttons">
      <button class="secondary" on:click={onclose}>Cancel</button>
      <button on:click={open}>Open GitHub issue</button>
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
  p {
    margin: 8px 0;
    font-size: 0.8rem;
    color: var(--fg-muted);
  }
  label { display: block; margin-top: 8px; font-size: 0.8rem; color: var(--fg-muted); }
  label textarea { margin-top: 2px; }
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
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 10px;
  }
</style>
