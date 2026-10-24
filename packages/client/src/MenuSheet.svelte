<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { NativeSelect } from './lib/ui/native-select';
  import { rememberFocus } from './lib/focus';
  import { settings, changeSettings, SKINS, type Skin } from './lib/settings';
  import { audioState, previewChime } from './lib/bell';
  import { CHIMES } from './lib/chimes';
  import { canInstall, promptInstall, isIosBrowser } from './lib/pwa';
  import { updateAvailable } from './lib/update';
  import { APP_VERSION } from './lib/appVersion';
  import KeyRowEditor from './KeyRowEditor.svelte';

  export let user: string;
  export let avatar = '';
  export let serverVersion = '';
  export let onservers: () => void;
  export let onreport: () => void;
  /** Unread inbox items, and the way into the inbox. */
  export let unread = 0;
  export let oninbox: () => void = () => {};
  /** Web Push for the server shown now: null when the browser cannot do it. */
  export let push: { serverName: string; configured: boolean; on: boolean; denied: boolean } | null = null;
  export let onpush: (enable: boolean) => void = () => {};
  export let onlogout: () => void;
  export let onclose: () => void;
  let dialog: HTMLDialogElement;
  let showSettings = false;
  /** iOS Safari has no vibration API: the switch is not offered where it would do nothing. */
  const canVibrate = typeof navigator !== 'undefined' && typeof navigator.vibrate === 'function';
  const initialRenderer = $settings.renderer;
  let broken = false;
  const restoreFocus = rememberFocus(); // before the dialog opens: closing it must not leave the terminal blurred
  onDestroy(restoreFocus);
  onMount(() => { dialog.showModal(); });
  function choose(action: () => void) { onclose(); action(); }
</script>

<dialog bind:this={dialog} aria-label="Menu" on:cancel|preventDefault={onclose} on:click={(e) => { if (e.target === dialog) onclose(); }}>
  <div class="menu">
    <div class="row"><h2>Termenbox</h2><button class="secondary" aria-label="Close menu" on:click={onclose}>×</button></div>
    <button class="secondary" on:click={() => choose(onservers)}>Servers</button>
    <button class="secondary" on:click={() => choose(oninbox)}>Inbox{#if unread > 0} ({unread} unread){/if}</button>
    {#if push}
      {#if !push.configured}
        <p>Notifications for {push.serverName}: on the server run <code>termenbox-server --setup-push</code>, restart it, then come back here. Needs the app added to the Home Screen on iPhone.</p>
      {:else if push.denied}
        <p>Notifications are blocked for this app in the browser settings.</p>
      {:else}
        <button class="secondary" on:click={() => onpush(!push.on)}>{push.on ? 'Turn off notifications' : 'Turn on notifications'} · {push.serverName}</button>
        <p>A phone follows one server at a time. The notice names the session and says what happened, never what the agent wrote.</p>
      {/if}
    {/if}
    <button class="secondary" aria-expanded={showSettings} on:click={() => showSettings = !showSettings}>Settings</button>
    {#if showSettings}
      <label>Cache TTL <NativeSelect value={$settings.cacheTtlMs} onchange={(e) => changeSettings({ cacheTtlMs: Number(e.currentTarget.value) as 300000 | 3600000 })}><option value="300000">5 minutes</option><option value="3600000">1 hour</option></NativeSelect></label>
      <label>Keep warm text <input value={$settings.keepWarm} placeholder="empty hides the button" maxlength="200" autocapitalize="off" on:change={(e) => changeSettings({ keepWarm: e.currentTarget.value })} /></label>
      <label class="check"><input type="checkbox" checked={$settings.alertSound} on:change={(e) => changeSettings({ alertSound: e.currentTarget.checked })} /> Play a sound when a session needs you</label>
      {#if $settings.alertSound}
        <label>Sound <NativeSelect value={$settings.alertSoundId} onchange={(e) => changeSettings({ alertSoundId: e.currentTarget.value })}>{#each CHIMES as c}<option value={c.id}>{c.label}</option>{/each}</NativeSelect></label>
        <div class="row"><button class="secondary" on:click={() => void previewChime($settings.alertSoundId, 'ask')}>Preview: asks</button><button class="secondary" on:click={() => void previewChime($settings.alertSoundId, 'done')}>Preview: finished</button></div>
        {#if $audioState === 'locked'}<p>Sound locked: tap anywhere to enable it.</p>{/if}
      {/if}
      {#if canVibrate}<label class="check"><input type="checkbox" checked={$settings.alertVibrate} on:change={(e) => changeSettings({ alertVibrate: e.currentTarget.checked })} /> Vibrate when a session needs you</label>{/if}
      <label>Theme <NativeSelect value={$settings.theme} onchange={(e) => changeSettings({ theme: e.currentTarget.value as 'dark' | 'light' | 'system' })}><option value="dark">Dark</option><option value="light">Light</option><option value="system">System</option></NativeSelect></label>
      <label>Skin <NativeSelect value={$settings.skin} onchange={(e) => changeSettings({ skin: e.currentTarget.value as Skin })}>{#each SKINS as skin}<option value={skin}>{skin[0].toUpperCase() + skin.slice(1)}</option>{/each}</NativeSelect></label>
      <label>Terminal renderer <NativeSelect value={$settings.renderer} onchange={(e) => changeSettings({ renderer: e.currentTarget.value as 'webgl' | 'dom' })}><option value="webgl">WebGL (fast)</option><option value="dom">Plain (if the screen flickers or goes blank)</option></NativeSelect></label>
      {#if $settings.renderer !== initialRenderer}<p>Applies the next time a session opens.</p>{/if}
      <KeyRowEditor />
    {/if}
    <button class="secondary" on:click={() => choose(onreport)}>Report a problem</button>
    {#if $canInstall}<button class="secondary" on:click={() => choose(promptInstall)}>Install app</button>
    {:else if isIosBrowser()}<p>Install: Share → Add to Home Screen.</p>
    {:else}<p>Install is available from your browser’s app menu.</p>{/if}
    <button class="secondary" disabled={!$updateAvailable} on:click={() => location.reload()}>{$updateAvailable ? 'Update available' : 'App is up to date'}</button>
    <p class="ver">Termenbox {APP_VERSION}{#if serverVersion} · server {serverVersion}{/if}</p>
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
