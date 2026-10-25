<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { rememberFocus } from './lib/focus';
  import AgentIcon from './AgentIcon.svelte';
  import { shortPath, type WorkspaceWindows } from './lib/windows';
  import type { HubWorkspace } from './lib/hub';

  export let workspace: HubWorkspace;
  export let data: WorkspaceWindows | undefined;
  /** False when the server cannot list windows (an old server): the menu then only offers Detach/Close. */
  export let supported = true;
  export let onselect: (index: number) => void;
  export let onnewwindow: () => void;
  /** False when the server is older than the message: the button would do nothing, so say why instead. */
  export let canNewWindow = true;
  export let onclosewindow: (index: number) => void;
  export let ondetach: () => void;
  export let oncloseworkspace: () => void;
  export let onclose: () => void;

  let dialog: HTMLDialogElement;
  /** A window whose ✕ was tapped once: it asks again before closing anything that is running. */
  let confirming: number | null = null;
  const SHELLS = new Set(['bash', 'zsh', 'fish', 'sh', 'dash', 'ash', 'ksh', 'tcsh', 'csh', 'nu']);
  const isShell = (cmd: string) => SHELLS.has(cmd.replace(/^-/, ''));
  const restoreFocus = rememberFocus(); // before the dialog opens: closing it must not leave the terminal blurred
  onMount(() => dialog.showModal());
  onDestroy(restoreFocus);

  $: windows = data?.windows ?? [];
  $: last = windows.length === 1;

  function closeTapped(index: number, command: string) {
    // An idle shell closes at once; anything running (an agent, an editor) and the last window ask first.
    if (confirming === index || (isShell(command) && !last)) { confirming = null; onclosewindow(index); }
    else confirming = index;
  }
</script>

<dialog bind:this={dialog} aria-label="Windows of {workspace.name}" on:cancel|preventDefault={onclose} on:click={(e) => { if (e.target === dialog) onclose(); }}>
  <header>
    <div class="who"><strong>{workspace.name}</strong>{#if workspace.serverName}<span>{workspace.serverName}</span>{/if}</div>
    <button class="secondary" on:click={() => { onclose(); ondetach(); }}>Detach</button>
    <button class="danger" on:click={() => { onclose(); oncloseworkspace(); }}>Close workspace</button>
  </header>
  {#if supported}
    <ul role="list">
      {#each windows as w (w.index)}
        <li class:active={w.active}>
          <button class="row" on:click={() => { onclose(); onselect(w.index); }}>
            <AgentIcon command={w.command} agent={w.agent} size={26} />
            <span class="text"><span class="cmd">{w.command || w.name}</span><span class="path">{shortPath(w.path)}</span></span>
            {#if w.active}<span class="here" aria-label="current window">●</span>{/if}
          </button>
          <button class="x" class:ask={confirming === w.index} aria-label={confirming === w.index ? `Confirm closing window ${w.index}` : `Close window ${w.index}`} on:click={() => closeTapped(w.index, w.command)}>
            {confirming === w.index ? (last ? 'Ends workspace' : 'Close?') : '✕'}
          </button>
        </li>
      {:else}
        <li class="none">Loading windows…</li>
      {/each}
    </ul>
    {#if canNewWindow}
      <button class="new" on:click={() => { onclose(); onnewwindow(); }}>+ New window</button>
    {:else}
      <p class="none">Opening a window from here needs a newer server. Update it from its host.</p>
    {/if}
  {:else}
    <p class="none">This workspace manager has no windows to list.</p>
  {/if}
</dialog>

<style>
  dialog { margin: 56px auto auto; width: min(100% - 16px, 420px); max-height: calc(var(--app-height, 100dvh) - 72px); padding: 0; overflow-y: auto; border: 1px solid var(--border); border-radius: 12px; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0006; }
  header { display: flex; align-items: center; gap: 6px; padding: 8px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
  .who { flex: 1 1 100%; display: flex; gap: 8px; align-items: baseline; min-width: 0; }
  .who strong, .who span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .who span { color: var(--fg-muted); font-size: 0.8rem; }
  ul { list-style: none; margin: 0; padding: 4px; }
  li { display: flex; gap: 4px; align-items: stretch; }
  li.active .row { border-color: var(--accent); }
  .row { flex: 1; min-width: 0; min-height: 48px; display: flex; align-items: center; gap: 10px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); }
  .text { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .cmd, .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .path { font-size: 0.75rem; color: var(--fg-muted); }
  .x { min-width: 48px; min-height: 48px; background: var(--bg); color: var(--fg-muted); border: 1px solid var(--border); }
  .x.ask { color: var(--bg); background: var(--danger); border-color: var(--danger); font-size: 0.75rem; }
  .danger { background: var(--danger); color: var(--bg); }
  header button { min-height: 44px; }
  .new { display: block; width: calc(100% - 8px); min-height: 48px; margin: 0 4px 8px; }
  .here { color: var(--accent); }
  .none { padding: 12px; color: var(--fg-muted); list-style: none; }
  li { margin-bottom: 4px; }
</style>
