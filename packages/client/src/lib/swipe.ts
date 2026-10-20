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
  const onBeforeInput = (e: Event) => {
    const ev = e as InputEvent;
    const data = replacementData(ev);
    if (data !== null) return out(replacementKeys(area.value, area.selectionStart, area.selectionEnd, data));
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
  };
}

/** What the hidden input needs to look like: exactly the word being typed, caret at the end. */
export interface MirrorTarget {
  value: string;
  setSelectionRange(start: number, end: number): void;
}

/** Makes `field` show `word`; returns whether it had to change. */
export function syncTextarea(field: MirrorTarget, word: string): boolean {
  if (field.value === word) return false;
  field.value = word;
  field.setSelectionRange(word.length, word.length);
  return true;
}

/** The hidden input mirror: shows the phrase that already reached the terminal. */
export interface InputMirror {
  /** Shows the phrase that already reached the terminal; never touches the field during composition. */
  sync(): void;
  /** Removes the composition listeners. */
  dispose(): void;
}

/**
 * xterm never cleans its hidden input, so after a few words it holds stale text (captured: "утромпа" while typing
 * "па"). iOS builds suggestions from that text and answers differently depending on it: the whole word when the
 * field is empty, only the missing tail when it holds the typed prefix. Keeping the field equal to the word being
 * typed gives the keyboard the true context, so a tapped suggestion arrives as the tail or as a replacement of
 * exactly that word or the last two words, both of which are handled.
 *
 * This runs only after the bytes reached the terminal (the caller mirrors once it has fed `words`). It must not run
 * on `input`/`keyup`/`compositionend` itself: Android's soft keyboard reports keyCode 229 or composes, and xterm
 * reads the textarea later, in a `setTimeout` (CompositionHelper._finalizeComposition and _handleAnyTextareaChanges).
 * Writing on those events put the previous phrase back before xterm looked, so every character was read as empty
 * (issue #250). iOS sends most typing synchronously instead, which is why the same mirror never hurt there.
 */
export function attachInputMirror(area: HTMLTextAreaElement, words: WordTracker): InputMirror {
  let composing = false;
  const onStart = () => (composing = true);
  const onEnd = () => (composing = false);
  area.addEventListener('compositionstart', onStart);
  area.addEventListener('compositionend', onEnd);
  return {
    sync: () => {
      if (!composing) queueMicrotask(() => !composing && syncTextarea(area, words.phrase));
    },
    dispose: () => {
      area.removeEventListener('compositionstart', onStart);
      area.removeEventListener('compositionend', onEnd);
    },
  };
}
