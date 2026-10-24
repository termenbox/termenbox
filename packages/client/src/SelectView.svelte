<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { copyText } from './lib/clipboard';

  export let text: string;
  export let onclose: () => void;
  /** Starts with everything selected (select-all from the terminal); the history may still be on its way. */
  export let all = false;
  export let loading = false;

  let box: HTMLPreElement;
  let root: HTMLDivElement;
  let status = '';
  onMount(() => { root.focus(); box.scrollTo(0, box.scrollHeight); });
  /** Selects the text of the page's pre and nothing else, so Ctrl+A here is the same as in the terminal. */
  function selectAll() {
    const range = document.createRange();
    range.selectNodeContents(box);
    const s = getSelection();
    s?.removeAllRanges();
    s?.addRange(range);
  }
  // The text arrives when the server has sent the history: the selection and the view follow it.
  $: if (box && text !== undefined) void tick().then(() => { if (all) selectAll(); box.scrollTo(0, box.scrollHeight); });
  function key(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && (e.key === 'a' || e.code === 'KeyA')) { e.preventDefault(); selectAll(); }
  }

  async function copyAll() {
    status = (await copyText(text)) ? 'Copied' : 'Copy failed';
    setTimeout(() => (status = ''), 2000);
  }
</script>

<!-- Touch screens cannot select text drawn by the terminal, so this shows the same text as plain, selectable
     text: long-press a word, drag the handles, copy. -->
<div bind:this={root} class="select" role="dialog" aria-label="Select text" tabindex="-1" on:keydown={key}>
  <div class="bar">
    <span>{loading ? 'Loading the whole history…' : all ? 'All selected. Copy, or scroll and adjust.' : 'Long-press to select'} {status}</span>
    <button type="button" on:click={copyAll}>Copy all</button>
    <button type="button" class="secondary" on:click={onclose}>Close</button>
  </div>
  <pre bind:this={box}>{text}</pre>
</div>

<style>
  .select {
    position: absolute;
    inset: 0;
    z-index: 1200;
    display: flex;
    flex-direction: column;
    background: var(--bg);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    font-size: 0.8rem;
  }
  .bar span {
    flex: 1;
    min-width: 0;
    color: var(--fg-muted);
  }
  pre {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 8px;
    overflow: auto;
    font: 12px/1.35 'JetBrains Mono', Menlo, monospace;
    color: var(--fg);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    -webkit-user-select: text;
    user-select: text;
    -webkit-touch-callout: default;
  }
</style>
