<script lang="ts">
  import { Switch } from 'bits-ui';
  import HookPreview from '../HookPreview.svelte';
  import PrivacyView from './PrivacyView.svelte';
  import AppearanceView from './AppearanceView.svelte';
  import type { HookRow } from '../lib/hooks';
  import type { HistoryDeletion } from '../lib/history';
  import type { PushSettings } from '../lib/protocol';
  import { fromForm, mutable, toForm, toggle, type PushForm } from '../lib/pushSettings';
  import { setShortcuts, shortcutsOn, SHORTCUTS, isMac } from '../lib/palette';
  /** The Settings destination: where the account, the servers and the app's own switches live. Preferences that exist
   *  today (theme, skin, cache, renderer) stay in the menu until they move here. */
  let { user, hooks = [], onhook, onservers, push = null, onpush, onshortcuts, onreport, onlogout, ondeletedata, historyServers, ondeletehistory, pushSettings, pushSaving = false, onpushsettings }: {
    user: string; hooks?: HookRow[]; onhook?: (choice: 'preview' | 'enable' | 'remove' | 'again', row: HookRow) => void; onservers: () => void; push?: { serverName: string; configured: boolean; on: boolean; denied: boolean } | null; onpush?: (enable: boolean) => void; onshortcuts: () => void; onreport: () => void; onlogout: () => void; ondeletedata: () => Promise<number>;
    historyServers: number; ondeletehistory: () => Promise<HistoryDeletion>;
    /** U11: the server's escalation settings (absent until it answers, or from a server that has none), and a save under way. */
    pushSettings?: PushSettings; pushSaving?: boolean; onpushsettings?: (settings: PushSettings) => void;
  } = $props();
  let privacy = $state(false);
  let appearance = $state(false);
  const row = 'flex h-14 w-full items-center justify-between gap-3 rounded-card border border-line bg-panel px-4 text-left text-base text-fg';
  /** The row whose Enable is waiting for the user to read the exact change. */
  let confirming = $state<string | null>(null);
  const idOf = (h: HookRow) => `${h.serverId}|${h.agent}`;
  const btn = 'min-h-11 rounded-card px-4 text-base font-semibold disabled:opacity-60';
  const key = isMac() ? SHORTCUTS[0].mac : SHORTCUTS[0].other;
  // The form follows the server's answer: a save that was rejected shows what is in force again.
  let form = $state<PushForm>({ waitMin: 10, maxMin: 60, multiplier: 2, quiet: '', muted: [] });
  $effect(() => { if (pushSettings) form = toForm(pushSettings); });
  const agents = $derived(mutable([...new Set(hooks.map((h) => h.agent))], form.muted));
  const field = 'min-h-11 w-full rounded-card border border-line bg-raised px-3 text-base text-fg';
  const save = () => onpushsettings?.(fromForm(form, -new Date().getTimezoneOffset()));
</script>

{#if privacy}
  <PrivacyView ondelete={ondeletedata} {historyServers} {ondeletehistory} onback={() => (privacy = false)} {onlogout} />
{:else if appearance}
  <AppearanceView {push} {onpush} onback={() => (appearance = false)} />
{:else}
<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4" data-shell="settings">
  <h1 class="text-title font-bold">Settings</h1>
  <p class="text-base text-soft">Signed in as {user}</p>
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
  {#if pushSettings}
    <section class="flex flex-col gap-2" aria-label="Push reminders" data-settings-push>
      <h2 class="text-sm font-semibold text-soft">Push reminders</h2>
      <form class="flex flex-col gap-3 rounded-card border border-line bg-panel p-4" onsubmit={(e) => { e.preventDefault(); save(); }}>
        <p class="text-sm text-soft">A question that waits is pushed again, then at growing gaps, until you answer it on any device.</p>
        <div class="grid grid-cols-2 gap-3">
          <label class="flex flex-col gap-1 text-sm text-soft" for="push-wait">Remind me after (minutes)
            <input id="push-wait" type="number" inputmode="numeric" min="1" max="10080" step="1" class={field} bind:value={form.waitMin} />
          </label>
          <label class="flex flex-col gap-1 text-sm text-soft" for="push-max">Never wait longer than (minutes)
            <input id="push-max" type="number" inputmode="numeric" min="1" max="10080" step="1" class={field} bind:value={form.maxMin} />
          </label>
        </div>
        <label class="flex flex-col gap-1 text-sm text-soft" for="push-mult">Each gap is this many times the last
          <input id="push-mult" type="number" inputmode="decimal" min="1.1" max="10" step="0.1" class={field} bind:value={form.multiplier} />
        </label>
        <label class="flex flex-col gap-1 text-sm text-soft" for="push-quiet">Quiet hours, your local time (22:00-07:00; empty for none)
          <input id="push-quiet" type="text" inputmode="numeric" placeholder="22:00-07:00" autocomplete="off" class={field} bind:value={form.quiet} />
        </label>
        {#if agents.length}
          <div class="flex flex-col gap-2" role="group" aria-label="Muted agents">
            <span class="text-sm text-soft">Mute an agent</span>
            <div class="flex flex-wrap gap-2">
              {#each agents as a (a)}
                <button type="button" class="min-h-11 rounded-card px-4 text-base font-semibold {form.muted.includes(a) ? 'bg-accent text-on-accent' : 'bg-raised text-fg'}" aria-pressed={form.muted.includes(a)} onclick={() => (form.muted = toggle(form.muted, a))}>{a}</button>
              {/each}
            </div>
          </div>
        {/if}
        <button type="submit" class="{btn} bg-accent text-on-accent" disabled={pushSaving}>{pushSaving ? 'Saving…' : 'Save'}</button>
      </form>
    </section>
  {/if}
  <button type="button" class={row} onclick={onservers}><span>Servers</span><span aria-hidden="true" class="text-soft">›</span></button>
  <button type="button" class={row} onclick={() => (appearance = true)}><span>Appearance, notifications and more</span><span aria-hidden="true" class="text-soft">›</span></button>
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
