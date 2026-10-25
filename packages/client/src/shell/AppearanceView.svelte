<script lang="ts">
  import { NativeSelect } from '../lib/ui/native-select';
  import { settings, changeSettings, SKINS, type Skin } from '../lib/settings';
  import { audioState, previewChime } from '../lib/bell';
  import { CHIMES } from '../lib/chimes';
  import { canInstall, promptInstall, isIosBrowser } from '../lib/pwa';
  import { updateAvailable } from '../lib/update';
  import { APP_VERSION } from '../lib/appVersion';
  import KeyRowEditor from '../KeyRowEditor.svelte';

  /** Settings > Appearance, notifications and more: the preferences of this device (theme, skin, sound, keys, renderer),
   *  the notifications switch of the server shown now, and the app's install and update. It is a page of Settings with a
   *  way back, not a second menu: nothing here leads to Settings again. */
  export let onback: () => void;
  /** Web Push for the server shown now: null when the browser cannot do it. */
  export let push: { serverName: string; configured: boolean; on: boolean; denied: boolean } | null = null;
  export let onpush: (enable: boolean) => void = () => {};
  /** iOS Safari has no vibration API: the switch is not offered where it would do nothing. */
  const canVibrate = typeof navigator !== 'undefined' && typeof navigator.vibrate === 'function';
  const initialRenderer = $settings.renderer;
</script>

<div class="page" data-shell="appearance">
  <div class="row"><button class="secondary" aria-label="Back to Settings" on:click={onback}>‹ Settings</button></div>
  <h1>Appearance, notifications and more</h1>
  {#if push}
      {#if !push.configured}
        <p>Notifications for {push.serverName}: on the server run <code>termenbox-server --setup-push</code>, restart it, then come back here. Needs the app added to the Home Screen on iPhone.</p>
      {:else if push.denied}
        <p>Notifications are blocked for this app in the browser settings.</p>
      {:else}
        <button class="secondary" on:click={() => onpush(!push.on)}>{push.on ? 'Turn off notifications' : 'Turn on notifications'} · {push.serverName}</button>
        <p>A phone follows one server at a time. The notice names the workspace and says what happened, never what the agent wrote.</p>
      {/if}
    {/if}
      <label>Cache TTL <NativeSelect value={$settings.cacheTtlMs} onchange={(e) => changeSettings({ cacheTtlMs: Number(e.currentTarget.value) as 300000 | 3600000 })}><option value="300000">5 minutes</option><option value="3600000">1 hour</option></NativeSelect></label>
      <label>Keep warm text <input value={$settings.keepWarm} placeholder="empty hides the button" maxlength="200" autocapitalize="off" on:change={(e) => changeSettings({ keepWarm: e.currentTarget.value })} /></label>
      <label class="check"><input type="checkbox" checked={$settings.alertSound} on:change={(e) => changeSettings({ alertSound: e.currentTarget.checked })} /> Play a sound when a workspace needs you</label>
      {#if $settings.alertSound}
        <label>Sound <NativeSelect value={$settings.alertSoundId} onchange={(e) => changeSettings({ alertSoundId: e.currentTarget.value })}>{#each CHIMES as c}<option value={c.id}>{c.label}</option>{/each}</NativeSelect></label>
        <div class="row"><button class="secondary" on:click={() => void previewChime($settings.alertSoundId, 'ask')}>Preview: asks</button><button class="secondary" on:click={() => void previewChime($settings.alertSoundId, 'done')}>Preview: finished</button></div>
        {#if $audioState === 'locked'}<p>Sound locked: tap anywhere to enable it.</p>{/if}
      {/if}
      {#if canVibrate}<label class="check"><input type="checkbox" checked={$settings.alertVibrate} on:change={(e) => changeSettings({ alertVibrate: e.currentTarget.checked })} /> Vibrate when a workspace needs you</label>{/if}
      <label>Theme <NativeSelect value={$settings.theme} onchange={(e) => changeSettings({ theme: e.currentTarget.value as 'dark' | 'light' | 'system' })}><option value="dark">Dark</option><option value="light">Light</option><option value="system">System</option></NativeSelect></label>
      <label>Skin <NativeSelect value={$settings.skin} onchange={(e) => changeSettings({ skin: e.currentTarget.value as Skin })}>{#each SKINS as skin}<option value={skin}>{skin[0].toUpperCase() + skin.slice(1)}</option>{/each}</NativeSelect></label>
      <label>Terminal renderer <NativeSelect value={$settings.renderer} onchange={(e) => changeSettings({ renderer: e.currentTarget.value as 'webgl' | 'dom' })}><option value="webgl">WebGL (fast)</option><option value="dom">Plain (if the screen flickers or goes blank)</option></NativeSelect></label>
      {#if $settings.renderer !== initialRenderer}<p>Applies the next time a workspace opens.</p>{/if}
      <KeyRowEditor />
    {#if $canInstall}<button class="secondary" on:click={() => void promptInstall()}>Install app</button>
    {:else if isIosBrowser()}<p>Install: Share → Add to Home Screen.</p>
    {:else}<p>Install is available from your browser’s app menu.</p>{/if}
    <button class="secondary" disabled={!$updateAvailable} on:click={() => location.reload()}>{$updateAvailable ? 'Update available' : 'App is up to date'}</button>
  <p class="ver">Termenbox {APP_VERSION}</p>
</div>

<style>
  .page { flex: 1; min-height: 0; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  h1 { font-size: 1.5rem; font-weight: 700; margin-bottom: 8px; }
  .row { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
  button { min-height: 44px; min-width: 44px; text-align: left; }
  .check { display: flex; align-items: center; gap: 8px; min-height: 44px; }
  label { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  p { font-size: 0.8rem; color: var(--fg-muted); }
</style>
