<script lang="ts">
  import { Switch } from 'bits-ui';
  import HookPreview from '../HookPreview.svelte';
  import PrivacyView from './PrivacyView.svelte';
  import type { HookRow } from '../lib/hooks';
  import type { HistoryDeletion } from '../lib/history';
  import { setShortcuts, shortcutsOn, SHORTCUTS, isMac } from '../lib/palette';
  /** The Settings destination: where the account, the servers and the app's own switches live. Preferences that exist
   *  today (theme, skin, cache, renderer) stay in the menu until they move here. */
  let { user, serverVersion = '', hooks = [], onhook, onservers, onmenu, onshortcuts, onreport, onlogout, ondeletedata, historyServers, ondeletehistory, pushSettings, onPushSettingsChange }: {
    user: string; serverVersion?: string; hooks?: HookRow[]; onhook?: (choice: 'preview' | 'enable' | 'remove' | 'again', row: HookRow) => void; onservers: () => void; onmenu: () => void; onshortcuts: () => void; onreport: () => void; onlogout: () => void; ondeletedata: () => Promise<number>;
    historyServers: number; ondeletehistory: () => Promise<HistoryDeletion>;
    pushSettings?: { initial_wait_secs: number; max_interval_secs: number; multiplier: number; quiet_hours?: string; muted_agents: string[] };
    onPushSettingsChange?: (settings: { initial_wait_secs: number; max_interval_secs: number; multiplier: number; quiet_hours?: string; muted_agents: string[] }) => void;
  } = $props();
  let privacy = $state(false);
  const row = 'flex h-14 w-full items-center justify-between gap-3 rounded-card border border-line bg-panel px-4 text-left text-base text-fg';
  /** The row whose Enable is waiting for the user to read the exact change. */
  let confirming = $state<string | null>(null);
  const idOf = (h: HookRow) => `${h.serverId}|${h.agent}`;
  const btn = 'min-h-11 rounded-card px-4 text-base font-semibold disabled:opacity-60';
  const key = isMac() ? SHORTCUTS[0].mac : SHORTCUTS[0].other;
  let saving = $state(false);
  let mutedAgentsInput = $state('');
  let allAgents: string[] = [];
  // Local copies of push settings for binding
  let localInitialWait = $state(600);
  let localMaxInterval = $state(3600);
  let localMultiplier = $state(2);
  let localQuietHours = $state('');
  
  // Sync local state with props
  $effect.pre(() => {
    if (pushSettings) {
      localInitialWait = pushSettings.initial_wait_secs;
      localMaxInterval = pushSettings.max_interval_secs;
      localMultiplier = pushSettings.multiplier;
      localQuietHours = pushSettings.quiet_hours ?? '';
    }
  });
  
  // Collect all known agents from hooks
  if (hooks) {
    for (const h of hooks) {
      if (!allAgents.includes(h.agent)) allAgents.push(h.agent);
    }
  }
  
  // Initialize mutedAgentsInput from pushSettings
  $effect.pre(() => {
    if (pushSettings?.muted_agents) {
      mutedAgentsInput = pushSettings.muted_agents.join(', ');
    }
  });
  
  async function savePushSettings() {
    if (!onPushSettingsChange) return;
    saving = true;
    const muted = mutedAgentsInput.split(',').map(s => s.trim()).filter(Boolean);
    onPushSettingsChange({
      initial_wait_secs: Math.max(60, localInitialWait),
      max_interval_secs: Math.max(60, localMaxInterval),
      multiplier: Math.max(1.1, Math.min(10, localMultiplier)),
      quiet_hours: localQuietHours || undefined,
      muted_agents: muted,
    });
    saving = false;
  }
</script>

{#if privacy}
  <PrivacyView ondelete={ondeletedata} {historyServers} {ondeletehistory} onback={() => (privacy = false)} {onlogout} />
{:else}
<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4" data-shell="settings">
  <h1 class="text-title font-bold">Settings</h1>
  <p class="text-base text-soft">Signed in as {user}{serverVersion ? ` · server ${serverVersion}` : ''}</p>
  {#if hooks.length}
    <section class="flex flex-col gap-2" aria-label="Agent hooks" data-settings-hooks>
      <h2 class="text-sm font-semibold text-soft">Agent hooks</h2>
      {#each hooks as h (idOf(h))}
        {@const on = h.info.state === 'connected'}
        <div class="flex flex-col gap-3 rounded-card border border-line bg-panel p-4" data-hook-row={h.agent} data-hook-state={h.info.state}>
          <div class="flex items-center justify-between gap-3">
            <span class="flex min-w-0 flex-col"><b class="text-base font-semibold">{h.label}</b><small class="truncate text-sm text-soft">{h.serverName}</small></span>
            <span class="shrink-0 text-base font-semibold {on ? 'text-ok' : 'text-soft'}" data-hook-status>{on ? 'On' : h.never ? 'Off · Never offered' : 'Off'}</span>
          </div>
          {#if h.info.error}<p class="text-sm text-bad" role="alert">{h.info.error}</p>{/if}
          {#if on}
            <button type="button" class="{btn} bg-raised text-fg" disabled={!!h.info.busy} onclick={() => onhook?.('remove', h)}>{h.info.busy === 'remove' ? 'Removing…' : 'Remove'}</button>
          {:else if confirming === idOf(h)}
            <HookPreview preview={h.info.preview} label={h.label} />
            <div class="grid grid-cols-2 gap-2">
              <button type="button" class="{btn} bg-accent text-on-accent" disabled={!h.info.preview || !!h.info.busy} onclick={() => { onhook?.('enable', h); confirming = null; }}>Enable</button>
              <button type="button" class="{btn} bg-raised text-fg" onclick={() => (confirming = null)}>Cancel</button>
            </div>
          {:else}
            <div class="flex flex-col gap-2">
              <button type="button" class="{btn} bg-raised text-fg" onclick={() => { confirming = idOf(h); onhook?.('preview', h); }}>Enable…</button>
              {#if h.never}<button type="button" class="{btn} bg-raised text-fg" onclick={() => onhook?.('again', h)}>Offer on Home again</button>{/if}
            </div>
          {/if}
        </div>
      {/each}
    </section>
  {/if}
    <section class="flex flex-col gap-2" aria-label="Push notifications" data-settings-push>
      <h2 class="text-sm font-semibold text-soft">Push notifications</h2>
      <div class="flex flex-col gap-3 rounded-card border border-line bg-panel p-4">
        <div class="grid grid-cols-2 gap-3">
          <div class="flex flex-col gap-1">
            <label class="text-sm text-soft">Re-notify after (minutes)</label>
            <input type="number" min="1" max="1440" step="1" class="input" bind:value={localInitialWait} onblur={() => localInitialWait = Math.max(60, localInitialWait)} />
          </div>
          <div class="flex flex-col gap-1">
            <label class="text-sm text-soft">Max interval (minutes)</label>
            <input type="number" min="1" max="10080" step="1" class="input" bind:value={localMaxInterval} onblur={() => localMaxInterval = Math.max(60, localMaxInterval)} />
          </div>
        </div>
        <div class="flex flex-col gap-1">
          <label class="text-sm text-soft">Multiplier</label>
          <input type="number" min="1.1" max="10" step="0.1" class="input" bind:value={localMultiplier} onblur={() => localMultiplier = Math.max(1.1, Math.min(10, localMultiplier))} />
        </div>
        <div class="flex flex-col gap-1">
          <label class="text-sm text-soft">Quiet hours (HH:MM-HH:MM, 24h, empty = none)</label>
          <input type="text" placeholder="22:00-07:00" class="input" bind:value={localQuietHours} />
        </div>
        <div class="flex flex-col gap-1">
          <label class="text-sm text-soft">Muted agents (comma-separated)</label>
          <input type="text" placeholder="claude,opencode" class="input" bind:value={mutedAgentsInput} />
          <div class="flex flex-wrap gap-1 text-xs text-soft">
            {#each allAgents as a}
              <button type="button" class="px-2 py-1 rounded bg-raised hover:bg-muted" onclick={() => {
                const arr = mutedAgentsInput.split(',').map(s => s.trim()).filter(Boolean);
                if (arr.includes(a)) mutedAgentsInput = arr.filter(x => x !== a).join(', ');
                else mutedAgentsInput = [...arr, a].join(', ');
              }}>{a}</button>
            {/each}
          </div>
        </div>
        <button type="button" class={btn} bg-accent text-on-accent self-start disabled={saving} onclick={savePushSettings}>
          {saving ? 'Saving…' : 'Save push settings'}
        </button>
      </div>
    </section>
    <button type="button" class={row} onclick={onservers}><span>Servers</span><span aria-hidden="true" class="text-soft">›</span></button>
  <button type="button" class={row} onclick={onmenu}><span>Appearance, notifications and more</span><span aria-hidden="true" class="text-soft">›</span></button>
  <div class={row}>
    <label id="shortcuts-label" for="shortcuts-switch"><span class="block">Keyboard shortcuts</span><span class="block text-sm text-soft">{key} opens the command palette. Off gives every key to the terminal.</span></label>
    <Switch.Root id="shortcuts-switch" aria-labelledby="shortcuts-label" checked={$shortcutsOn} onCheckedChange={setShortcuts}
      class="relative inline-flex h-11 w-16 shrink-0 items-center rounded-full bg-raised p-1.5 data-[state=checked]:bg-accent">
      <Switch.Thumb class="block size-8 rounded-full bg-soft transition-transform duration-(--duration-base) data-[state=checked]:translate-x-5 data-[state=checked]:bg-on-accent" />
    </Switch.Root>
  </div>
  <button type="button" class={row} onclick={onshortcuts}><span>Show the shortcut list</span><span aria-hidden="true" class="text-soft">›</span></button>
  <button type="button" class={row} onclick={() => (privacy = true)}><span>Privacy and your data</span><span aria-hidden="true" class="text-soft">›</span></button>
  <button type="button" class={row} onclick={onreport}><span>Report a problem</span><span aria-hidden="true" class="text-soft">›</span></button>
  <button type="button" class={row} onclick={onlogout}><span>Sign out</span></button>
</div>
{/if}
