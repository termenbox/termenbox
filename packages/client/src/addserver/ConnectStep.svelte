<script lang="ts">
  import type { ActionId, Connect } from '../lib/addServer';
  import Failure from './Failure.svelte';

  /** Step 2 (decision 0127): a live checklist, the camera in place at the top, a paste field for the link or secret. */
  export let connect: Connect;
  /** A code, link or secret has been read, so the checklist is past its first line. */
  export let codeRead: boolean;
  export let scanning: boolean;
  export let video: HTMLVideoElement | null = null;
  export let onscan: () => void;
  export let onstopscan: () => void;
  export let onfile: (e: Event) => void;
  export let ontext: (text: string) => void;
  export let onaction: (id: ActionId) => void;
  export let error = '';

  let text = '';
  const PHASES = ['code', 'looking', 'found', 'reading'] as const;
  const WORDS: Record<(typeof PHASES)[number], string> = { code: 'Code read', looking: 'Looking for the server', found: 'Found the server', reading: 'Reading what it is' };
  $: stage = codeRead ? Math.max(1, PHASES.indexOf(connect.phase)) : 0;
  const mark = (i: number, st: number) => (i < st ? '✓' : i === st ? '…' : '·');
</script>

<div class="scan">
  <video bind:this={video} muted playsinline data-testid="scan-video" style="display: {scanning ? 'block' : 'none'};"></video>
  {#if scanning}
    <button class="secondary" on:click={onstopscan} style="width: 100%;">Stop camera</button>
  {:else}
    <button on:click={onscan} style="width: 100%;" data-testid="scan-it">Server shows a QR? Scan it</button>
  {/if}
</div>

{#if connect.diagnosis}
  <Failure diagnosis={connect.diagnosis} {onaction}>
    <slot name="detail" />
  </Failure>
{:else if error}
  <p class="err" role="alert">{error}</p>
{/if}
{#if connect.notice === 'ws_fallback'}
  <p class="notice" role="status" data-testid="ws-notice">Connected through the WebSocket fallback because UDP did not work. Sessions work but can feel slower.</p>
{/if}

<ul class="checks" data-testid="checklist" aria-label="Progress">
  {#each PHASES as p, i}
    <li class:active={i === stage && !connect.diagnosis} class:done={i < stage}><span aria-hidden="true">{mark(i, stage)}</span> {WORDS[p]}</li>
  {/each}
</ul>

{#if connect.addresses.length}
  <details class="addrs" data-testid="addresses">
    <summary>Addresses tried ({connect.addresses.length})</summary>
    <ul>
      {#each connect.addresses as a}
        <li>{a.label}: {a.state === 'trying' ? 'trying…' : a.state === 'won' ? 'connected' : `failed${a.reason ? ` (${a.reason})` : ''}`}</li>
      {/each}
    </ul>
  </details>
{/if}

<form on:submit|preventDefault={() => text.trim() && ontext(text.trim())} class="paste">
  <label for="have-link">Paste the link, or the secret from your command</label>
  <div style="display: flex; gap: 8px;">
    <input id="have-link" bind:value={text} placeholder="https://… or a code" autocapitalize="off" autocomplete="off" spellcheck="false" style="flex: 1; min-width: 0;" />
    <button type="submit" class="secondary" disabled={!text.trim()}>Add</button>
  </div>
</form>
<label class="file">
  or choose a screenshot of the QR
  <input type="file" accept="image/*" on:change={onfile} style="display: none;" />
</label>

<style>
  .scan { margin-bottom: 10px; }
  video { width: 100%; max-height: 220px; object-fit: cover; border-radius: 8px; background: #000; margin-bottom: 8px; }
  .checks { list-style: none; padding: 0; margin: 10px 0; font-size: 0.875rem; color: var(--fg-muted); }
  .checks li { padding: 2px 0; }
  .checks li.done { color: var(--fg); }
  .checks li.active { color: var(--fg); font-weight: 600; }
  .checks span { display: inline-block; width: 1.2em; }
  .addrs { font-size: 0.8rem; color: var(--fg-muted); margin-bottom: 8px; }
  .addrs summary { cursor: pointer; }
  .addrs ul { margin: 6px 0 0; padding-left: 18px; }
  .paste label { display: block; font-size: 0.8rem; margin-bottom: 4px; color: var(--fg-muted); }
  .file { display: block; margin-top: 8px; font-size: 0.8rem; color: var(--fg-muted); text-decoration: underline; cursor: pointer; }
  .err { color: var(--danger); font-size: 0.875rem; margin: 8px 0; }
  .notice { font-size: 0.8rem; color: var(--fg-muted); border: 1px solid var(--border); border-radius: 8px; padding: 8px 10px; margin: 8px 0; }
</style>
