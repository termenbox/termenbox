<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { Composer } from './lib/ds';
  import KeyRow from './KeyRow.svelte';
  import { History, PROMPT_KEYS, complete, completions, enterCompletes, insertAt, keyEffect, type Command, type KeyEffect } from './lib/acpPrompt';
  import { keyRowConfig, rowMode } from './lib/keyrow';
  import { IOS_KEYBOARD_HINT, nextVoiceLang, parseVoiceLang, pickLanguage, recognizerFactory, Voice, type VoiceLang } from './lib/terminal/voice';

  /** The prompt line of an ACP workspace (U07b): one multi-line field above the key row, in the dock the soft keyboard
   *  never covers (guardrail soft-keyboard-never-hides-input: the shell is sized by AppShell and this sits at its
   *  bottom). Enter sends (`session/prompt`), Shift+Enter or the ↵ key breaks the line, Esc or the Stop key interrupts a
   *  turn, `/` offers the agent's own commands. Paste and the keyboard's dictation work in the field as in any field;
   *  the 🎤 key adds the browser's recognizer where there is one. Nothing here moves under a finger: the Stop key
   *  appears left of Send, and the completion list floats over the stream instead of pushing it. */
  let { value = $bindable(''), commands, working, disabled = false, blocked = '', prompts, onsend, oninterrupt, onchange }: {
    value?: string; commands: Command[]; working: boolean; disabled?: boolean; blocked?: string; prompts: string[];
    onsend: (text: string) => void; oninterrupt: () => void; onchange?: (text: string) => void;
  } = $props();

  let input = $state<HTMLInputElement | HTMLTextAreaElement | null>(null);
  let cursor = $state(0);
  let picked = $state(0);
  let closed = $state(false); // Esc closed the list for this word
  const history = new History([]);
  const sig = $derived(prompts.join('\u0000'));
  $effect(() => { sig; history.reset(untrack(() => prompts)); });

  const comp = $derived(closed ? null : completions(value, cursor, commands));
  const items = $derived(comp?.items ?? []);
  $effect(() => { comp?.query; picked = 0; });
  $effect(() => { value; if (!history.browsing) closed = false; onchange?.(value); });

  function place(text: string, at: number) {
    value = text;
    cursor = at;
    // Focus is the reader's to give: a key of the row never opens the keyboard by itself.
    queueMicrotask(() => input?.setSelectionRange(at, at));
  }
  function accept(c: Command | undefined = items[picked]) {
    if (!c) return;
    const r = complete(value, c);
    place(r.text, r.cursor);
  }
  function apply(e: KeyEffect) {
    switch (e.do) {
      case 'interrupt': oninterrupt(); break;
      case 'close-popup': closed = true; break;
      case 'accept': accept(); break;
      case 'move': picked = (picked + e.by + items.length) % items.length; break;
      case 'history': { const t = e.by < 0 ? history.previous(value) : history.next(); if (t !== null) place(t, t.length); break; }
      case 'clear': place('', 0); break;
      case 'insert': { const r = insertAt(value, input?.selectionStart ?? cursor, e.text); place(r.text, r.cursor); break; }
      case 'cursor': {
        const el = input; if (!el) break;
        const at = e.to === 'home' ? 0 : e.to === 'end' ? value.length : Math.max(0, Math.min(value.length, (el.selectionStart ?? 0) + (e.to === 'left' ? -1 : 1)));
        place(value, at); break;
      }
      case 'none': break;
    }
  }
  const keyState = () => ({ working, popup: items.length > 0, draft: value, cursor });
  const row = (seq: string) => apply(keyEffect(seq, keyState()));

  /** A hardware keyboard (or the phone's) in the field. */
  function onkeydown(e: KeyboardEvent) {
    if (e.isComposing) return;
    const el = input;
    const selecting = !!el && el.selectionStart !== el.selectionEnd;
    const seq = e.key === 'Escape' ? '\x1b' : e.key === 'Tab' && !e.shiftKey ? '\t'
      : e.key === 'ArrowUp' ? '\x1b[A' : e.key === 'ArrowDown' ? '\x1b[B'
      : e.key === 'c' && e.ctrlKey && !e.metaKey && !selecting ? '\x03' : '';
    if (e.key === 'Enter' && !e.shiftKey && items.length && enterCompletes(value, items[picked])) { e.preventDefault(); accept(); return; }
    if (!seq) return;
    // Up and Down walk the history only at the ends of the text, so they still move the cursor in a long prompt.
    if ((seq === '\x1b[A' || seq === '\x1b[B') && !items.length) {
      const at = el?.selectionStart ?? 0;
      const edge = seq === '\x1b[A' ? !value.slice(0, at).includes('\n') : !value.slice(at).includes('\n');
      if (!edge || selecting) return;
    }
    const effect = keyEffect(seq, keyState());
    if (effect.do === 'none') return;
    e.preventDefault();
    apply(effect);
  }

  // The cursor, for the completion list: it follows selection changes in the field.
  onMount(() => {
    const sync = () => { if (input && document.activeElement === input) cursor = input.selectionStart ?? 0; };
    document.addEventListener('selectionchange', sync);
    return () => document.removeEventListener('selectionchange', sync);
  });

  // Dictation: the browser's own recognizer where there is one; final text is typed into the field, never sent.
  const makeRecognizer = recognizerFactory();
  const VOICE_LANG_KEY = 'termenbox-voice-lang';
  let voiceLang = $state<VoiceLang>('auto');
  try { voiceLang = parseVoiceLang(localStorage.getItem(VOICE_LANG_KEY)); } catch { /* storage blocked: the choice lasts until reload */ }
  let listening = $state(false);
  let interim = $state('');
  let voiceError = $state('');
  const voice = makeRecognizer ? new Voice(makeRecognizer, () => pickLanguage(voiceLang, /[Ѐ-ӿ]/.test(value) ? 'cyrillic' : /[a-z]/i.test(value) ? 'latin' : null, navigator.language), {
    onText: (t) => { const r = insertAt(value, input?.selectionStart ?? value.length, t); place(r.text, r.cursor); },
    onInterim: (t) => (interim = t),
    onState: (on) => { listening = on; if (on) voiceError = ''; },
    onError: (m) => { voiceError = m; setTimeout(() => (voiceError = ''), 5000); },
    onFallback: () => { voiceError = IOS_KEYBOARD_HINT; setTimeout(() => (voiceError = ''), 8000); },
  }) : null;
  onMount(() => { const stop = () => voice?.stop(); window.addEventListener('pagehide', stop); return () => { window.removeEventListener('pagehide', stop); voice?.stop(); }; });
  function cycleLang() {
    voiceLang = nextVoiceLang(voiceLang);
    try { localStorage.setItem(VOICE_LANG_KEY, voiceLang); } catch { /* see above */ }
  }

  // A key row suits a touch screen; with a mouse and a keyboard the field is enough.
  let finePointer = $state(false);
  $effect(() => {
    if (typeof matchMedia === 'undefined') return;
    const q = matchMedia('(pointer: fine) and (hover: hover)');
    finePointer = q.matches;
    const on = () => (finePointer = q.matches);
    q.addEventListener('change', on);
    return () => q.removeEventListener('change', on);
  });
  const showKeys = $derived(rowMode($keyRowConfig.desktop, finePointer) === 'full');

  function send(text: string): boolean {
    if (!text.trim() || working || disabled || blocked) return false;
    onsend(text);
    history.reset();
    return true;
  }
</script>

<div class="relative">
  {#if items.length}
    <ul id="acp-commands" role="listbox" aria-label="Commands" data-testid="commands"
      class="absolute inset-x-3 bottom-full z-10 mb-1 max-h-[40vh] overflow-y-auto rounded-card border border-line bg-panel p-1 shadow-sheet [overscroll-behavior:contain]">
      {#each items as c, i (c.name)}
        <li role="option" aria-selected={i === picked} id="acp-cmd-{i}">
          <button type="button" tabindex="-1" onpointerdown={(e) => e.preventDefault()} onclick={() => accept(c)}
            class="flex min-h-11 w-full flex-col items-start justify-center gap-0 rounded-control px-3 py-1 text-start {i === picked ? 'bg-raised' : 'bg-transparent'} text-fg">
            <span class="font-mono text-base font-semibold">/{c.name}{#if c.hint} <span class="text-sm font-normal text-muted">{c.hint}</span>{/if}</span>
            {#if c.description}<span class="line-clamp-1 w-full text-xs font-normal text-soft">{c.description}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  {#if items.length}<span class="sr-only" role="status">{items.length} commands</span>{/if}
  {#if listening || voiceError}<div class="px-4 py-1 text-xs text-soft" role="status">{voiceError || interim || 'Listening…'}</div>{/if}
  <Composer bind:value bind:input multiline label="Prompt" placeholder={blocked || (working ? 'Working. ■ stops it.' : 'Prompt, or / for commands')}
    {disabled} sendDisabled={working || !!blocked} {onkeydown} onsend={send}
    fieldProps={{ 'aria-controls': items.length ? 'acp-commands' : undefined, 'aria-activedescendant': items.length ? `acp-cmd-${picked}` : undefined, 'aria-autocomplete': 'list' }}>
    {#snippet trailing()}
      {#if working}
        <button type="button" aria-label="Stop the agent" data-testid="stop" onclick={oninterrupt} onpointerdown={(e) => e.preventDefault()}
          class="grid size-11 place-items-center rounded-control bg-bad p-0 text-base text-white">■</button>
      {/if}
    {/snippet}
    {#snippet keys()}
      {#if showKeys}
        <KeyRow keys={PROMPT_KEYS} label="Prompt keys" onkey={row} onnewline={() => apply({ do: 'insert', text: '\n' })} voiceSupported={!!voice} {listening} onvoice={() => void voice?.toggle()} {voiceLang} onvoicelang={cycleLang} />
      {/if}
    {/snippet}
  </Composer>
</div>
