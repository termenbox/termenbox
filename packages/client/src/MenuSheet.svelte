<script lang="ts">
  import { backdropClose } from './lib/backdropClose';
  import { onMount, onDestroy } from 'svelte';
  import { rememberFocus } from './lib/focus';
  import { APP_VERSION } from './lib/appVersion';

  export let user: string;
  export let avatar = '';
  export let onservers: () => void;
  /** Settings is a destination of its own; the menu only leads there. */
  export let onsettings: () => void;
  export let onreport: () => void;
  export let ondiagnostics: () => void = () => {};
  /** Unread inbox items, and the way into the inbox. */
  export let unread = 0;
  export let oninbox: () => void = () => {};
  export let onlogout: () => void;
  export let onclose: () => void;
  let dialog: HTMLDialogElement;
  let broken = false;
  const restoreFocus = rememberFocus(); // before the dialog opens: closing it must not leave the terminal blurred
  onDestroy(restoreFocus);
  onMount(() => { dialog.showModal(); });
  function choose(action: () => void) { onclose(); action(); }
</script>

<dialog bind:this={dialog} aria-label="Menu" on:cancel|preventDefault={onclose} use:backdropClose={() => { onclose(); }}>
  <div class="menu">
    <div class="row"><h2>Termenbox</h2><button class="secondary" aria-label="Close menu" on:click={onclose}>×</button></div>
    <button class="secondary" on:click={() => choose(onservers)}>Servers</button>
    <button class="secondary" on:click={() => choose(oninbox)}>Inbox{#if unread > 0} ({unread} unread){/if}</button>
    <button class="secondary" on:click={() => choose(onsettings)}>Settings</button>
    <button class="secondary" on:click={() => choose(onreport)}>Report a problem</button>
    <button class="secondary" on:click={() => choose(ondiagnostics)}>Server diagnostics</button>
    <p class="ver">Termenbox {APP_VERSION}</p>
    <div class="account">{#if avatar && !broken}<img src={avatar} alt="" width="28" height="28" referrerpolicy="no-referrer" on:error={() => broken = true} />{/if}<span>@{user}</span></div>
    <button class="secondary" on:click={() => choose(onlogout)}>Log out</button>
  </div>
</dialog>

<style>
  dialog { margin: auto auto 0; width: min(100%, 420px); max-height: calc(var(--app-height, 100dvh) - 16px); overflow-y: auto; padding: 0; border: 1px solid var(--border); border-radius: 16px 16px 0 0; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0008; }
  .menu { padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  .row { display: flex; justify-content: space-between; align-items: center; }
  button { min-height: 44px; min-width: 44px; }
  .check { display: flex; align-items: center; gap: 8px; min-height: 44px; }
  button { text-align: left; }
  label { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  p { font-size: 0.8rem; color: var(--fg-muted); }
  .account { display: flex; align-items: center; gap: 8px; padding: 8px; }
  img { border-radius: 50%; }
</style>
