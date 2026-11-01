import type { WordTracker } from './terminal/wordTracker';

// iOS swipe typing and autocorrect (plan X.1/X.2, fact 0027): the keyboard inserts a whole word with
// `insertText`, which xterm forwards. When the user then picks another suggestion, iOS fires
// `beforeinput` with `inputType: "insertReplacementText"`, the old word still selected in xterm's hidden
// textarea (e.g. value "hello", selection 0-5, data "he'll"). xterm ignores that event, so the terminal kept
// the old word and only the following space arrived. We turn it into what a person would have typed:
// delete the old word, then type the replacement.

const DEL = '\x7f';

/** Bytes for the terminal: one DEL per code point of the replaced text, then the replacement. */
export function replacementKeys(value: string, start: number, end: number, data: string): string {
  const removed = [...value.slice(Math.min(start, end), Math.max(start, end))].length;
  return DEL.repeat(removed) + data;
}

/**
 * Same, when the field has no selection to read. The mirror collapses the caret to the end of the field after
 * every write, so a replacement that arrives later (a swipe correction, a tapped "привет" for "прв") finds nothing
 * selected; what it replaces is then taken from the word tracker.
 */
export function replacementKeysFromWords(words: WordTracker, data: string): string {
  const { text, trailing } = words.replaced(data);
  return DEL.repeat([...text, ...trailing].length) + data + trailing;
}

/** The text of a replacement event, or null when the event is not one we handle. */
export function replacementData(e: Pick<InputEvent, 'inputType' | 'data' | 'isComposing' | 'dataTransfer'>): string | null {
  if (e.inputType !== 'insertReplacementText' || e.isComposing) return null; // composition has its own path in xterm
  const data = e.data ?? e.dataTransfer?.getData('text/plain') ?? '';
  return data || null;
}

// Key names the platform defines ("Enter", "ArrowLeft", "Dead", "Unidentified", "F12"); anything else longer than
// one character is not a key.
const NAMED_KEY = /^(?:[A-Z][A-Za-z]+|F\d{1,2})$/;

/**
 * The iOS Russian keyboard reports a swiped *word* as the key of a keydown (`key="чем"`, fact 0027). xterm takes
 * any keydown as "handled" and then ignores the `insertText` that carries the word, so the word is lost. A
 * keydown like that must not reach xterm; its input event then types the word, as it does for Latin swipes.
 */
export function isWordKey(key: string): boolean {
  return [...key].length > 1 && !NAMED_KEY.test(key);
}

/** A dictation chunk that nothing follows for this long is final. */
export const DICTATION_SETTLE_MS = 500;

const MODIFIER_KEYS = new Set(['Shift', 'Control', 'Alt', 'Meta', 'CapsLock']);

const length = (s: string) => [...s].length;

/**
 * iOS dictation (the keyboard's mic, not lib/terminal/voice.ts) reports its recognizer's interim results as input:
 * "why", "why would", "why would I swipe"; xterm typed every chunk, giving "whywhy wouldwhy would I swipe". Chunks
 * come in two shapes, both held back here until the text is final, then typed once through `out`:
 *  - composition: `isComposing` events whose first chunk is already more than one character (a keyboard composing
 *    a typed word starts with one, and xterm keeps handling that). xterm must not see these events; at
 *    `compositionend` the field is put back so xterm's own read of it finds nothing new.
 *  - plain `insertText` chunks, each extending the one before within the settle time. The first goes through as an
 *    ordinary insert (it cannot be told from a swiped word yet), so when the second extends it, the first is taken back.
 * Final means: a chunk that does not extend the last, an `insertFromComposition`, the end of the composition, a key
 * press, or silence for DICTATION_SETTLE_MS. A single `insertText` never gets held back.
 * The listeners are on `host` in the capture phase, ahead of xterm's own on the textarea. Returns their removal.
 */
function attachDictationBuffer(area: HTMLTextAreaElement, host: EventTarget, out: (keys: string) => void): () => void {
  let pending: string | null = null; // interim text held back, not yet typed
  let sent = ''; // the last whole chunk that went to xterm in this burst; the base of a possible extension
  let session: 'take' | 'pass' | null = null; // how the current composition is handled; decided by its first chunk
  let swallowInput = false;
  let snapshot = ''; // the field when the composition began
  let timer: ReturnType<typeof setTimeout> | undefined;

  const disarm = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  };
  const commit = () => {
    disarm();
    const text = pending;
    pending = null;
    if (text) out(text);
  };
  const arm = () => {
    disarm();
    timer = setTimeout(() => {
      timer = undefined;
      commit();
      sent = '';
    }, DICTATION_SETTLE_MS);
  };
  const hold = (e: Event, text: string) => {
    e.stopImmediatePropagation();
    pending = text;
    swallowInput = true;
    arm();
  };
  const extends_ = (base: string, data: string) => length(base) > 1 && length(data) > length(base) && data.startsWith(base);

  const onBeforeInput = (e: Event) => {
    if (e.target !== area) return;
    const ev = e as InputEvent;
    const data = ev.data ?? '';
    if (ev.isComposing || ev.inputType === 'insertCompositionText') {
      session ??= length(data) > 1 ? 'take' : 'pass';
      if (session === 'take') hold(e, data);
      return;
    }
    if (ev.inputType === 'insertFromComposition' && session === 'take') {
      hold(e, data || pending || '');
      commit();
      return;
    }
    if (ev.inputType !== 'insertText' || !data) return;
    const base = pending ?? sent;
    if (extends_(base, data)) {
      if (pending === null) out(DEL.repeat(length(sent))); // take back the first chunk, which xterm already typed
      sent = '';
      hold(e, data);
      return;
    }
    commit(); // the previous burst is over
    sent = length(data) > 1 ? data : '';
    if (sent) arm();
  };
  const onInput = (e: Event) => {
    if (e.target !== area || !(swallowInput || session === 'take')) return;
    e.stopImmediatePropagation();
    swallowInput = false;
  };
  const onCompositionStart = (e: Event) => {
    if (e.target !== area) return;
    commit();
    session = null;
    snapshot = area.value;
  };
  const onCompositionUpdate = (e: Event) => {
    if (e.target === area && session === 'take') e.stopImmediatePropagation(); // xterm would draw its composition view
  };
  const onCompositionEnd = (e: Event) => {
    if (e.target !== area) return;
    if (session === 'take') {
      area.value = snapshot; // xterm reads what the composition added, a task later; there must be none
      commit();
    }
    session = null;
    swallowInput = false;
  };
  const onKeyDown = (e: Event) => {
    if (e.target !== area || MODIFIER_KEYS.has((e as KeyboardEvent).key)) return;
    commit();
    sent = '';
  };

  const listeners: [string, (e: Event) => void][] = [
    ['keydown', onKeyDown],
    ['beforeinput', onBeforeInput],
    ['input', onInput],
    ['compositionstart', onCompositionStart],
    ['compositionupdate', onCompositionUpdate],
    ['compositionend', onCompositionEnd],
  ];
  for (const [type, fn] of listeners) host.addEventListener(type, fn, true);
  return () => {
    disarm();
    for (const [type, fn] of listeners) host.removeEventListener(type, fn, true);
  };
}

/**
 * Makes swipe typing work on the terminal's hidden input: drops word-valued keydowns before xterm sees them, and
 * turns "replace the word you just typed" into keystrokes (see above). `send` receives the bytes to type.
 * Returns the function that removes the listeners. Add other keyboards' quirks as further cases here.
 */
export function attachSwipeSupport(area: HTMLTextAreaElement, send: (keys: string) => void, words: WordTracker): () => void {
  // xterm listens on the textarea in the capture phase, so an ancestor is the only place to run first.
  const host: EventTarget = area.parentElement ?? area;
  const onKeyDown = (e: Event) => {
    if (e.target === area && isWordKey((e as KeyboardEvent).key)) e.stopPropagation();
  };
  const out = (keys: string) => {
    words.feed(keys);
    send(keys);
  };
  const stopDictation = attachDictationBuffer(area, host, out); // before the listeners below: it may stop their events
  const onBeforeInput = (e: Event) => {
    const ev = e as InputEvent;
    const data = replacementData(ev);
    if (data !== null) {
      // A selection the keyboard made is the truth; a collapsed one says nothing (the mirror put it there).
      return out(area.selectionStart !== area.selectionEnd ? replacementKeys(area.value, area.selectionStart, area.selectionEnd, data) : replacementKeysFromWords(words, data));
    }
    // A tapped suggestion that completes the typed word arrives as plain new text; delete the typed part first.
    // xterm types the text itself right after this event.
    if (ev.inputType === 'insertText' && !ev.isComposing && ev.data) {
      const n = words.completionPrefix(ev.data);
      if (n > 0) out('\x7f'.repeat(n));
    }
  };
  host.addEventListener('keydown', onKeyDown, true);
  area.addEventListener('beforeinput', onBeforeInput, true);
  return () => {
    host.removeEventListener('keydown', onKeyDown, true);
    area.removeEventListener('beforeinput', onBeforeInput, true);
    stopDictation();
  };
}

/** What the hidden input needs to look like: exactly the word being typed, caret at the end. */
export interface MirrorTarget {
  value: string;
  selectionStart?: number;
  selectionEnd?: number;
  setSelectionRange(start: number, end: number): void;
}

/** Makes `field` show `word`; returns whether it had to change. */
export function syncTextarea(field: MirrorTarget, word: string): boolean {
  if (field.value === word) return false;
  // Assigning the value collapses a selection, and one the keyboard just made (a word picked for replacement) must
  // survive until its beforeinput reads it; the next sync, after that event, puts the field right.
  if (field.selectionStart !== undefined && field.selectionStart !== field.selectionEnd) return false;
  field.value = word;
  field.setSelectionRange(word.length, word.length);
  return true;
}

/** The hidden input mirror: shows the phrase the send path has fed to the word tracker. */
export interface InputMirror {
  /** Shows that phrase; never touches the field during composition. The write is deferred to a task. */
  sync(): void;
  /** Removes the composition listeners and cancels a pending write. */
  dispose(): void;
}

/**
 * xterm never cleans its hidden input, so after a few words it holds stale text (captured: "утромпа" while typing
 * "па"). iOS builds suggestions from that text and answers differently depending on it: the whole word when the
 * field is empty, only the missing tail when it holds the typed prefix. Keeping the field equal to the word being
 * typed gives the keyboard the true context, so a tapped suggestion arrives as the tail or as a replacement of
 * exactly that word or the last two words, both of which are handled.
 *
 * The caller mirrors once it has fed `words`, so the field never gets ahead of the bytes. The write is deferred to
 * a task, not a microtask: a microtask queued from a `beforeinput` listener runs before the browser's default
 * action, and moving the caret there makes `insertReplacementText` apply at the wrong place ("he'llhe'll").
 * It must not run on `input`/`keyup`/`compositionend` itself: Android's soft keyboard reports keyCode 229 or
 * composes, and xterm reads the textarea later, in a `setTimeout` (CompositionHelper._finalizeComposition and
 * _handleAnyTextareaChanges). Writing on those events put the previous phrase back before xterm looked, so every
 * character was read as empty (issue #250). iOS sends most typing synchronously instead, which is why the same
 * mirror never hurt there.
 */
export function attachInputMirror(area: HTMLTextAreaElement, words: WordTracker): InputMirror {
  let composing = false;
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const onStart = () => (composing = true);
  const onEnd = () => (composing = false);
  const write = () => {
    timer = undefined;
    if (!disposed && !composing) syncTextarea(area, words.phrase);
  };
  area.addEventListener('compositionstart', onStart);
  area.addEventListener('compositionend', onEnd);
  return {
    sync: () => {
      if (disposed || composing || timer !== undefined) return;
      timer = setTimeout(write, 0);
    },
    dispose: () => {
      disposed = true;
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
      area.removeEventListener('compositionstart', onStart);
      area.removeEventListener('compositionend', onEnd);
    },
  };
}
