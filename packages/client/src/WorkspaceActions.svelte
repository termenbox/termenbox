<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { rememberFocus } from './lib/focus';
  import { renameOutcome } from './lib/rename';
  import { movedText, type Move, type Placement } from './lib/order';
  import type { HubWorkspace } from './lib/hub';
  export let workspace: HubWorkspace;
  /** Closing a workspace is not possible on every manager. */
  export let canClose = true;
  export let ondetach: () => void;
  export let oncloseworkspace: () => void;
  export let onclose: () => void;
  /** Sets the name shown for the workspace; the sheet closes with it. Missing: no Rename. */
  export let onrename: ((name: string) => void) | undefined = undefined;
  /** Moves the workspace in the user's own order (inside its server) and says where it ended up. Missing: no Move buttons. */
  export let onmove: ((to: Move) => Placement | null) | undefined = undefined;
  /** Where the workspace stands now among its server's; the Move buttons that cannot move are disabled. */
  export let placement: Placement | null = null;
  /** Opened from "Close workspace" in the windows menu: go straight to the question. */
  export let confirmFirst = false;
  let dialog: HTMLDialogElement;
  /** Close workspace asks first, naming the workspace and its server (guardrail kill-session-needs-confirmation). */
  let confirming = confirmFirst;
  let renaming = false;
  let draft = workspace.name;
  let announcement = '';
  $: first = !placement || placement.position <= 1;
  $: last = !placement || placement.position >= placement.count;
  /** An empty name keeps the field open; an unchanged one just closes. */
  function save() {
    const out = renameOutcome(draft, workspace.name);
    if (out.kind === 'empty') return;
    onclose();
    if (out.kind === 'rename') onrename?.(out.name);
  }
  function move(to: Move) {
    const p = onmove?.(to);
    if (p) announcement = movedText(workspace.name, p);
  }
  // A touch screen puts the caret where the finger tapped once the field has focus: select the name again on the next frame.
  const focusSelect = (el: HTMLInputElement) => {
    el.focus(); el.select();
    requestAnimationFrame(() => { if (document.activeElement === el) el.setSelectionRange(0, el.value.length); });
  };
  const restoreFocus = rememberFocus();
  onMount(() => dialog.showModal());
  onDestroy(restoreFocus);
</script>
<dialog bind:this={dialog} class:renaming aria-label="Workspace actions" on:cancel|preventDefault={onclose}>
  <h2>{workspace.name}</h2><p>{workspace.serverName}</p>
  <div class="sr" role="status" aria-live="polite">{announcement}</div>
  {#if confirming}
    <p role="alert">Close <strong>{workspace.name}</strong> on {workspace.serverName}? Everything running in it ends.</p>
    <button class="danger" on:click={() => { onclose(); oncloseworkspace(); }}>Close workspace</button>
    <button class="secondary" on:click={() => (confirmFirst ? onclose() : (confirming = false))}>Cancel</button>
  {:else if renaming}
    <form on:submit|preventDefault={save}>
      <input use:focusSelect bind:value={draft} aria-label="Workspace name" aria-invalid={!draft.trim()} maxlength="64" autocomplete="off" autocapitalize="off" spellcheck="false" enterkeyhint="done" data-testid="workspace-rename" />
      <button type="submit" disabled={!draft.trim()}>Save name</button>
      <button type="button" class="secondary" on:click={() => (renaming = false)}>Cancel</button>
    </form>
  {:else}
    {#if onrename}<button on:click={() => { draft = workspace.name; renaming = true; }} data-testid="workspace-rename-item">Rename…</button>{/if}
    {#if onmove && placement && placement.count > 1}
      <div class="move" role="group" aria-label={`Order, position ${placement.position} of ${placement.count}`}>
        <button on:click={() => move('up')} disabled={first} data-testid="workspace-move-up">Move up</button>
        <button on:click={() => move('down')} disabled={last} data-testid="workspace-move-down">Move down</button>
        <button on:click={() => move('top')} disabled={first} data-testid="workspace-move-top">To top</button>
        <button on:click={() => move('bottom')} disabled={last} data-testid="workspace-move-bottom">To bottom</button>
      </div>
      <p class="where" data-testid="workspace-position">Position {placement.position} of {placement.count} on {workspace.serverName}</p>
    {/if}
    <button on:click={() => { onclose(); ondetach(); }}>Detach (keeps it running)</button>
    {#if canClose}<button class="danger" on:click={() => (confirming = true)}>Close workspace…</button>{/if}
    <button class="secondary" on:click={onclose}>Cancel</button>
  {/if}
</dialog>
<style>
  dialog { margin: auto; padding: 16px; width: 360px; max-width: calc(100% - 32px); max-height: calc(100% - 32px); overflow-y: auto; border: 1px solid var(--border); border-radius: 12px; background: var(--bg-secondary); color: var(--fg); }
  /* Renaming: the soft keyboard takes the lower part of a phone, so the field sits at the top, where it stays in view. */
  dialog.renaming { margin-top: 16px; margin-bottom: auto; }
  dialog::backdrop { background: #0008; }
  h2, p { overflow-wrap: anywhere; margin-bottom: 12px; }
  button { display: block; width: 100%; min-height: 44px; min-width: 44px; margin-bottom: 8px; }
  button:disabled { opacity: 0.45; }
  .move { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-bottom: 4px; }
  .move button { margin-bottom: 0; }
  .where { font-size: 0.8rem; color: var(--fg-muted); margin: 4px 0 12px; }
  input { display: block; width: 100%; min-height: 44px; margin-bottom: 8px; padding: 0 12px; font-size: 16px; color: var(--fg); background: var(--bg); border: 1px solid var(--border); border-radius: 8px; }
  .danger { background: var(--danger); color: var(--bg); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
</style>
