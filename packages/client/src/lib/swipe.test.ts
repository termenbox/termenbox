import { describe, expect, it, vi } from 'vitest';
import { attachInputMirror, attachSwipeSupport, DICTATION_SETTLE_MS, isWordKey, replacementData, replacementKeys, replacementKeysFromWords, syncTextarea } from './swipe';
import { WordTracker } from './terminal/wordTracker';

/** The hidden textarea, with just the DOM surface attachInputMirror uses. */
interface MirrorField {
  value: string;
  range: [number, number];
  setSelectionRange(start: number, end: number): void;
  addEventListener(type: string, fn: (e?: any) => void): void;
  removeEventListener(type: string, fn: (e?: any) => void): void;
  emit(type: string, e?: { stopped?: boolean }): void;
  count(type: string): number;
}

function field(value = ''): MirrorField {
  const listeners = new Map<string, Set<(e?: any) => void>>();
  return {
    value,
    range: [-1, -1],
    setSelectionRange(start, end) {
      this.range = [start, end];
    },
    addEventListener(type, fn) {
      if (!listeners.has(type)) listeners.set(type, new Set());
      listeners.get(type)!.add(fn);
    },
    removeEventListener(type, fn) {
      listeners.get(type)?.delete(fn);
    },
    emit(type, e) {
      for (const fn of listeners.get(type) ?? []) {
        fn(e);
        if (e?.stopped) break; // stopImmediatePropagation
      }
    },
    count(type) {
      return listeners.get(type)?.size ?? 0;
    },
  };
}

const asArea = (f: MirrorField) => f as unknown as HTMLTextAreaElement;
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

describe('replacementKeys', () => {
  it('replays the sequence captured on an iPhone: "hello" swiped, "he\'ll" chosen', () => {
    // beforeinput insertReplacementText data="he'll" val="hello" sel=0-5
    expect(replacementKeys('hello', 0, 5, "he'll")).toBe("\x7f\x7f\x7f\x7f\x7fhe'll");
  });

  it('removes only what the keyboard selected and counts characters, not UTF-16 units', () => {
    expect(replacementKeys('say hello', 4, 9, 'world')).toBe('\x7f\x7f\x7f\x7f\x7fworld');
    expect(replacementKeys('😀a', 0, 3, 'b')).toBe('\x7f\x7fb');
    expect(replacementKeys('abc', 2, 0, 'x')).toBe('\x7f\x7fx'); // reversed selection
  });

  it('just types the text when nothing was selected', () => {
    expect(replacementKeys('hello', 5, 5, 'x')).toBe('x');
  });
});

describe('replacementData', () => {
  const base = { inputType: 'insertReplacementText', data: "he'll", isComposing: false, dataTransfer: null };
  it('accepts replacement events and ignores everything else', () => {
    expect(replacementData(base)).toBe("he'll");
    expect(replacementData({ ...base, inputType: 'insertText' })).toBeNull();
    expect(replacementData({ ...base, isComposing: true })).toBeNull();
    expect(replacementData({ ...base, data: null })).toBeNull();
  });
});

describe('isWordKey', () => {
  it('recognises a swiped word reported as the key (captured on the Russian iOS keyboard)', () => {
    expect(isWordKey('чем')).toBe(true);
    expect(isWordKey('Чем')).toBe(true);
    expect(isWordKey('hello')).toBe(true);
  });

  it('leaves real keys alone', () => {
    for (const key of ['з', 'a', ' ', '😀', 'Enter', 'Backspace', 'ArrowLeft', 'Tab', 'Dead', 'Unidentified', 'Process', 'Shift', 'F1', 'F12']) {
      expect(isWordKey(key), key).toBe(false);
    }
  });
});

describe('replacing two words at once', () => {
  it('deletes both words and types the replacement', () => {
    // the keyboard offered "api works" for the phrase "app works"; the field holds both words
    expect(replacementKeys('app works', 0, 9, 'api works')).toBe('\x7f'.repeat(9) + 'api works');
  });
});

describe('replacementKeysFromWords', () => {
  const typed = (t: string) => {
    const w = new WordTracker();
    w.feed(t);
    return w;
  };

  it('replaces a typed prefix (прв -> привет)', () => {
    expect(replacementKeysFromWords(typed('прв'), 'привет')).toBe('\x7f\x7f\x7fпривет');
  });

  it('replaces a capitalised prefix and a swipe correction', () => {
    expect(replacementKeysFromWords(typed('Прив'), 'привет')).toBe('\x7f'.repeat(4) + 'привет');
    expect(replacementKeysFromWords(typed('hello'), "he'll")).toBe('\x7f'.repeat(5) + "he'll");
  });

  it('replaces two words for a two-word replacement and keeps earlier words', () => {
    expect(replacementKeysFromWords(typed('say app works'), 'api works')).toBe('\x7f'.repeat(9) + 'api works');
    expect(replacementKeysFromWords(typed('say hello'), 'world')).toBe('\x7f'.repeat(5) + 'world');
  });

  it('types the whitespace after the word again', () => {
    expect(replacementKeysFromWords(typed('прв '), 'привет')).toBe('\x7f'.repeat(4) + 'привет ');
  });

  it('only types the data when nothing was typed', () => {
    expect(replacementKeysFromWords(new WordTracker(), 'x')).toBe('x');
  });
});

describe('syncTextarea', () => {
  const field = (value: string) => {
    const f = { value, range: [-1, -1], setSelectionRange(a: number, b: number) { this.range = [a, b]; } };
    return f;
  };

  it('replaces stale text with the word being typed and puts the caret at its end', () => {
    const f = field('утромпа'); // captured while typing "па"
    expect(syncTextarea(f, 'па')).toBe(true);
    expect(f.value).toBe('па');
    expect(f.range).toEqual([2, 2]);
  });

  it('leaves a field alone when it is already right (no caret jumps)', () => {
    const f = field('па');
    expect(syncTextarea(f, 'па')).toBe(false);
    expect(f.range).toEqual([-1, -1]);
  });

  it('leaves a selection alone so the keyboard\'s replacement can read it', () => {
    const f = { ...field('прв'), selectionStart: 0, selectionEnd: 3 };
    expect(syncTextarea(f, '')).toBe(false);
    expect(f.value).toBe('прв');
    expect(f.range).toEqual([-1, -1]);
  });

  it('empties the field between words', () => {
    const f = field('паспорт');
    syncTextarea(f, '');
    expect(f.value).toBe('');
  });
});

describe('input mirror', () => {
  it('mirrors the phrase that was already sent, with the caret at its end', async () => {
    const area = field('stale');
    const words = new WordTracker();
    words.feed('hello ');
    attachInputMirror(asArea(area), words).sync();
    await flush();
    expect(area.value).toBe('hello ');
    expect(area.range).toEqual([6, 6]);
  });

  it('defers the write to a task so a beforeinput default action runs first', async () => {
    const area = field('hello');
    const words = new WordTracker();
    words.feed("he'll");
    const mirror = attachInputMirror(asArea(area), words);
    mirror.sync();
    await Promise.resolve();
    expect(area.value).toBe('hello');
    await flush();
    expect(area.value).toBe("he'll");
    expect(area.range).toEqual([5, 5]);
    mirror.dispose();
  });

  it('cancels a pending write on dispose', async () => {
    const area = field('hello');
    const words = new WordTracker();
    words.feed("he'll");
    const mirror = attachInputMirror(asArea(area), words);
    mirror.sync();
    mirror.dispose();
    await flush();
    expect(area.value).toBe('hello');
  });

  it('leaves the field alone on input and keyup: only a send may mirror', async () => {
    const area = field('typed by the keyboard');
    const words = new WordTracker();
    const mirror = attachInputMirror(asArea(area), words);
    area.emit('input');
    area.emit('keyup');
    await flush();
    expect(area.value).toBe('typed by the keyboard');
    mirror.dispose();
  });

  it('does not touch the field during composition, then mirrors the phrase after it', async () => {
    const area = field();
    const words = new WordTracker();
    words.feed('hello ');
    const mirror = attachInputMirror(asArea(area), words);
    area.value = 'hello w';
    area.emit('compositionstart');
    mirror.sync();
    await flush();
    expect(area.value).toBe('hello w');
    area.emit('compositionend');
    mirror.sync();
    await flush();
    expect(area.value).toBe('hello ');
  });

  it('does not mirror on compositionend: only a send may', async () => {
    const area = field('hello w');
    const words = new WordTracker();
    words.feed('hello ');
    const mirror = attachInputMirror(asArea(area), words);
    area.emit('compositionstart');
    area.value = 'hello wo';
    area.emit('compositionend');
    await flush();
    expect(area.value).toBe('hello wo');
    mirror.sync();
    await flush();
    expect(area.value).toBe('hello ');
  });

  it('keeps a composed character until xterm’s deferred Android read (issue #250)', async () => {
    const area = field();
    const words = new WordTracker();
    words.feed('hello ');
    const mirror = attachInputMirror(asArea(area), words);
    area.value = words.phrase; // what the field held before this composition
    area.emit('compositionstart');
    area.value += 'w'; // the IME commits the character
    area.emit('input'); // Chrome Android fires input before compositionend
    let sent = '';
    // xterm.js 5.3 reads CompositionHelper._finalizeComposition() in a setTimeout; a microtask would beat it.
    await new Promise<void>((resolve) => {
      setTimeout(() => {
        sent = area.value.substring(words.phrase.length);
        if (sent) words.feed(sent);
        mirror.sync();
        resolve();
      }, 0);
    });
    await flush();
    expect(sent).toBe('w');
    expect(area.value).toBe('hello w');
  });

  it('keeps a keyCode 229 character until xterm’s deferred diff (issue #250)', async () => {
    const area = field();
    const words = new WordTracker();
    words.feed('hi ');
    const mirror = attachInputMirror(asArea(area), words);
    area.value = words.phrase;
    const before = area.value;
    area.value += 'a'; // no composition events, only the 229 path's textarea change
    area.emit('input');
    area.emit('keyup');
    let sent = '';
    await new Promise<void>((resolve) => {
      setTimeout(() => {
        sent = area.value.replace(before, '');
        if (sent) words.feed(sent);
        mirror.sync();
        resolve();
      }, 0);
    });
    await flush();
    expect(sent).toBe('a');
    expect(area.value).toBe('hi a');
  });

  it('mirrors Cyrillic, emoji and backspace with the caret in UTF-16 units', async () => {
    const area = field();
    const words = new WordTracker();
    const mirror = attachInputMirror(asArea(area), words);
    words.feed('привет ');
    mirror.sync();
    await flush();
    expect(area.value).toBe('привет ');
    expect(area.range).toEqual([7, 7]);
    words.feed('😀');
    mirror.sync();
    await flush();
    expect(area.value).toBe('привет 😀'); // the last two words, as iOS suggestions see them
    expect(area.range).toEqual([9, 9]);
    words.feed('\x7f');
    mirror.sync();
    await flush();
    expect(area.value).toBe('привет ');
    expect(area.range).toEqual([7, 7]);
  });

  it('empties the field after Enter', async () => {
    const area = field();
    const words = new WordTracker();
    words.feed('done');
    const mirror = attachInputMirror(asArea(area), words);
    words.feed('\r');
    mirror.sync();
    await flush();
    expect(area.value).toBe('');
  });

  it('removes both composition listeners and adds no input/keyup listeners', () => {
    const area = field();
    const mirror = attachInputMirror(asArea(area), new WordTracker());
    expect(area.count('compositionstart')).toBe(1);
    expect(area.count('compositionend')).toBe(1);
    expect(area.count('input')).toBe(0);
    expect(area.count('keyup')).toBe(0);
    mirror.dispose();
    expect(area.count('compositionstart')).toBe(0);
    expect(area.count('compositionend')).toBe(0);
  });
});

describe('dictation buffering', () => {
  const setup = () => {
    vi.useFakeTimers();
    const area = field();
    const sent: string[] = [];
    const words = new WordTracker();
    attachSwipeSupport(asArea(area), (k) => sent.push(k), words);
    /** Fires an event the way the browser would; true when it reached xterm (nothing stopped it). */
    const fire = (type: string, init: Record<string, unknown> = {}) => {
      const e = { target: area, stopped: false, stopImmediatePropagation() { this.stopped = true; }, ...init };
      area.emit(type, e);
      return !e.stopped;
    };
    return { area, sent, fire };
  };

  it('holds back composition interim chunks and types the final text once', () => {
    const { area, sent, fire } = setup();
    fire('compositionstart');
    area.value = 'why would I swipe it out';
    for (const data of ['why', 'why would', 'why would I swipe it out']) {
      expect(fire('beforeinput', { inputType: 'insertCompositionText', data, isComposing: true })).toBe(false);
      expect(fire('input', { inputType: 'insertCompositionText', data, isComposing: true })).toBe(false);
    }
    expect(fire('compositionupdate')).toBe(false);
    expect(sent).toEqual([]);
    fire('compositionend');
    expect(sent).toEqual(['why would I swipe it out']);
    expect(area.value).toBe(''); // put back, so xterm's own read finds nothing
  });

  it('replaces a growing insertText stream by its last chunk', () => {
    const { sent, fire } = setup();
    // the first chunk is indistinguishable from a swiped word: xterm types it
    expect(fire('beforeinput', { inputType: 'insertText', data: 'why', isComposing: false })).toBe(true);
    for (const data of ['why would', 'why would I swipe']) {
      expect(fire('beforeinput', { inputType: 'insertText', data, isComposing: false })).toBe(false);
      expect(fire('input', { inputType: 'insertText', data })).toBe(false);
    }
    expect(sent).toEqual(['\x7f\x7f\x7f']); // "why" taken back, nothing else typed yet
    vi.advanceTimersByTime(DICTATION_SETTLE_MS);
    expect(sent).toEqual(['\x7f\x7f\x7f', 'why would I swipe']);
    // a chunk that does not extend ends the burst
    fire('beforeinput', { inputType: 'insertText', data: 'why', isComposing: false });
    fire('beforeinput', { inputType: 'insertText', data: 'why it', isComposing: false });
    expect(fire('beforeinput', { inputType: 'insertText', data: 'out', isComposing: false })).toBe(true);
    expect(sent.slice(2)).toEqual(['\x7f\x7f\x7f', 'why it']);
    vi.useRealTimers();
  });

  it('commits held text when a key is pressed', () => {
    const { sent, fire } = setup();
    fire('beforeinput', { inputType: 'insertText', data: 'why', isComposing: false });
    fire('beforeinput', { inputType: 'insertText', data: 'why would', isComposing: false });
    fire('keydown', { key: 'x' });
    expect(sent).toEqual(['\x7f\x7f\x7f', 'why would']);
    vi.useRealTimers();
  });

  it('lets a single insertText and one-character composition through untouched', () => {
    const { sent, fire } = setup();
    expect(fire('beforeinput', { inputType: 'insertText', data: 'hello', isComposing: false })).toBe(true);
    expect(fire('input', { inputType: 'insertText', data: 'hello' })).toBe(true);
    vi.advanceTimersByTime(DICTATION_SETTLE_MS * 2);
    fire('compositionstart');
    expect(fire('beforeinput', { inputType: 'insertCompositionText', data: 'p', isComposing: true })).toBe(true);
    expect(fire('beforeinput', { inputType: 'insertCompositionText', data: 'pa', isComposing: true })).toBe(true);
    expect(fire('compositionupdate')).toBe(true);
    fire('compositionend');
    expect(sent).toEqual([]);
    vi.useRealTimers();
  });
});
