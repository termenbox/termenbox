<script lang="ts">
  import { KEY_BY_ID, type KeyDef, type KeyProfile, type Mods } from './lib/keys';
  import { keyRowConfig, layoutRow, moreKeys, needsMore, resolveRow, rowMode } from './lib/keyrow';
  import { press } from './lib/press';

  let {
    mods = $bindable({ ctrl: false, alt: false, shift: false }),
    onkey,
    /** Fn opens the on-screen function keyboard in place of the system keyboard. */
    fnOpen = false,
    onfn = () => {},
    /** `tmux` adds the sticky prefix key, labelled with the prefix as printed on a keycap (`^B`). */
    profile = 'shell',
    prefixArmed = $bindable(false),
    prefixLabel = '',
    /** The multiline-input key: `newlineHint` names the combination it sends (Alt+Enter for Claude Code). */
    onnewline = () => {},
    newlineHint = 'Alt+Enter',
    onkeyboard = () => {},
    /** The dictation key: shown only when the browser has a speech recognizer. */
    voiceSupported = false,
    listening = false,
    onvoice = () => {},
    /** The dictation language chip: Auto follows what was typed, EN and RU force one. */
    voiceLang = 'auto',
    onvoicelang = () => {},
    onfont = () => {},
    /** Reached from the compact bar on a desktop (the full row has them in the Fn panel). */
    onpaste = () => {},
    oncopy = () => {},
    onselect = () => {},
    bell = true,
    onbell = () => {},
    /** A fixed row (the ACP prompt's): these key ids in this order, nothing under More, and `label` names the toolbar. */
    keys: fixedKeys,
    label = 'Terminal keys',
  }: {
    mods?: Mods;
    onkey: (data: string) => void;
    fnOpen?: boolean;
    onfn?: () => void;
    profile?: KeyProfile;
    prefixArmed?: boolean;
    prefixLabel?: string;
    onnewline?: () => void;
    newlineHint?: string;
    onkeyboard?: () => void;
    voiceSupported?: boolean;
    listening?: boolean;
    onvoice?: () => void;
    voiceLang?: 'auto' | 'en' | 'ru';
    onvoicelang?: () => void;
    onfont?: (delta: number) => void;
    onpaste?: () => void;
    oncopy?: () => void;
    onselect?: () => void;
    bell?: boolean;
    onbell?: () => void;
    keys?: readonly string[];
    label?: string;
  } = $props();

  let width = $state(0);
  let moreOpen = $state(false);
  let finePointer = $state(false);
  $effect(() => {
    if (typeof matchMedia === 'undefined') return;
    const q = matchMedia('(pointer: fine) and (hover: hover)');
    finePointer = q.matches;
    const on = () => (finePointer = q.matches);
    q.addEventListener('change', on);
    return () => q.removeEventListener('change', on);
  });
  const mode = $derived(rowMode($keyRowConfig.desktop, finePointer));

  const rowIds = $derived(fixedKeys ? [...fixedKeys] : resolveRow($keyRowConfig, profile));
  const defs = $derived(rowIds.map((id) => KEY_BY_ID.get(id)).filter((k): k is KeyDef => !!k && (k.needs !== 'voice' || voiceSupported)));
  const layout = $derived(layoutRow(defs, width, fixedKeys ? false : needsMore(rowIds, [], profile, voiceSupported)));
  const hidden = $derived(fixedKeys ? layout.overflow : moreKeys(defs.map((k) => k.id), layout.overflow.map((k) => k.id), profile, voiceSupported));

  const langName = $derived(voiceLang === 'auto' ? 'follows what you type' : voiceLang === 'en' ? 'English' : 'Russian');

  interface View { label: string; name: string; pressed?: boolean; run: () => void; testid?: string }
  function view(k: KeyDef): View {
    const toggle = (m: 'ctrl' | 'alt' | 'shift') => () => (mods = { ...mods, [m]: !mods[m] });
    switch (k.id) {
      case 'prefix': return { label: prefixLabel || 'Prefix', name: prefixLabel ? `tmux prefix ${prefixLabel}` : 'tmux prefix', pressed: prefixArmed, run: () => (prefixArmed = !prefixArmed) };
      case 'newline': return { label: k.label, name: `New line without sending (${newlineHint})`, run: onnewline };
      case 'ctrl': return { label: k.label, name: k.name, pressed: mods.ctrl, run: toggle('ctrl') };
      case 'alt': return { label: k.label, name: k.name, pressed: mods.alt, run: toggle('alt') };
      case 'shift': return { label: k.label, name: k.name, pressed: !!mods.shift, run: toggle('shift') };
      case 'fn': return { label: k.label, name: k.name, pressed: fnOpen, run: onfn };
      case 'keyboard': return { label: k.label, name: k.name, run: onkeyboard };
      case 'voice': return { label: k.label, name: listening ? 'Stop dictation' : 'Dictate', pressed: listening, run: onvoice };
      case 'voicelang': return { label: voiceLang === 'auto' ? k.label : voiceLang.toUpperCase(), name: `Dictation language: ${langName}`, run: onvoicelang, testid: 'voice-lang' };
      case 'fontDown': return { label: k.label, name: k.name, run: () => onfont(-1) };
      case 'fontUp': return { label: k.label, name: k.name, run: () => onfont(1) };
      default: return { label: k.label, name: k.name, run: () => onkey(k.seq ?? '') };
    }
  }

  /** A modifier or toggle stays in the panel so it can be combined with the next key; anything else closes it. */
  const runFromPanel = (k: KeyDef, v: View) => () => {
    v.run();
    if (k.kind !== 'mod') moreOpen = false;
  };

  const cap =
    'relative h-11 min-w-0 rounded-control border border-line bg-bg p-0 text-[13px] font-medium text-fg select-none [-webkit-user-select:none] [touch-action:manipulation] ' +
    'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent-text aria-pressed:bg-accent aria-pressed:text-on-accent';
  const tool =
    'h-6 min-w-6 rounded-control border border-line bg-bg px-2 py-0 text-xs font-medium text-fg [touch-action:manipulation] ' +
    'focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent-text aria-pressed:bg-accent aria-pressed:text-on-accent';
</script>

{#if mode === 'compact'}
  <!-- A physical keyboard: no key row, one hint line, and the actions that have no key. -->
  <div class="flex h-7 items-center gap-1 border-t border-line bg-panel px-2" role="toolbar" aria-label="Terminal keys" data-mode="compact">
    <span class="min-w-0 flex-1 truncate text-xs text-soft">Ctrl+C, Esc, Tab and the arrows go straight to the terminal</span>
    <button type="button" class={tool} use:press={onpaste}>Paste</button>
    <button type="button" class={tool} use:press={oncopy}>Copy</button>
    <button type="button" class={tool} use:press={onselect}>Select</button>
    <button type="button" class={tool} aria-label="Smaller text" use:press={() => onfont(-1)}>A−</button>
    <button type="button" class={tool} aria-label="Larger text" use:press={() => onfont(1)}>A+</button>
    <button type="button" class={tool} aria-label={bell ? 'Mute bell' : 'Unmute bell'} aria-pressed={bell} use:press={onbell}>{bell ? '🔔' : '🔕'}</button>
    {#if voiceSupported}
      <button type="button" class={tool} aria-label={listening ? 'Stop dictation' : 'Dictate'} aria-pressed={listening} data-press-on="up" use:press={onvoice}>🎤</button>
    {/if}
  </div>
{:else}
  <!-- One row that fits the screen width, rendered from the configuration (Settings). Keys that do not fit, and keys taken off the row, are under More. -->
  <div class="relative">
    {#if moreOpen}
      <div class="absolute inset-x-0 bottom-full z-10 flex max-h-[40vh] flex-wrap content-start gap-2 overflow-y-auto border-t border-line bg-panel p-2 [touch-action:pan-y]" role="group" aria-label="More keys">
        {#each hidden as k (k.id)}
          {@const v = view(k)}
          <button type="button" class={cap + ' min-w-11 flex-none px-2'} aria-label={v.name} aria-pressed={k.kind === 'mod' ? v.pressed : undefined} data-press-on={k.id === 'voice' ? 'up' : undefined} use:press={runFromPanel(k, v)}>{v.label}</button>
        {/each}
        <button type="button" class={cap + ' min-w-11 flex-none px-3'} use:press={() => (moreOpen = false)}>Close</button>
      </div>
    {/if}
    <div class="flex gap-0.5 border-t border-line bg-panel px-1 py-[3px] [overflow:hidden] [contain:inline-size] [overscroll-behavior:contain] [touch-action:none]" role="toolbar" aria-label={label} data-mode="full" bind:clientWidth={width}>
      {#each layout.shown as k (k.id)}
        {@const v = view(k)}
        <button type="button" class={cap + ' flex-1 basis-0' + (k.small ? ' !text-xs' : '')} aria-label={v.name} aria-pressed={k.kind === 'mod' ? v.pressed : undefined} data-testid={v.testid} data-press-on={k.id === 'voice' ? 'up' : undefined} use:press={v.run}>{v.label}</button>
      {/each}
      {#if layout.more}
        <button type="button" class={cap + ' flex-1 basis-0'} aria-label="More keys" aria-expanded={moreOpen} aria-haspopup="true" use:press={() => (moreOpen = !moreOpen)}>⋯</button>
      {/if}
    </div>
  </div>
{/if}
