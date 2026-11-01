// Remembers the word being typed at the terminal's prompt, from the bytes we send. iOS decides what a tapped
// suggestion replaces from the text in the hidden input, which xterm keeps empty or stale; when the keyboard
// offers "утром" for a typed "у", iOS inserts the whole suggestion as new text (captured on an iPhone, fact 0027)
// and the typed letters stay behind. Knowing the word lets us delete it first.

const BACKSPACE = new Set(['\x7f', '\b']);

/** One word that arrived in a single piece, with the spaces the keyboard put around it (`hello `, ` hello`). */
const WHOLE_WORD = /^\s*(\S+?)(\s*)$/u;

/** Characters after which a word goes on without a space: `device-`, `src/`, `$HOME`, `--opt=`. */
const JOINERS = new Set(['-', '_', '/', '\\', '(', '[', '{', '@', '~', '$', '=']);

/** A word the keyboard inserted whole (swipe typing, a tapped suggestion), and the spaces it left after it. */
interface Inserted {
  chars: string[];
  trailing: number;
}

/** The last two words with the whitespace between and after them; what the keyboard may offer to replace as one phrase. */
const PHRASE = /(?:\S+[ \t]+)?\S+[ \t]*$|[ \t]+$/u;

/** Plain text of one line is kept this long at most. */
const LINE_CAP = 200;

export class WordTracker {
  private word: string[] = [];
  /** Plain text typed on this line since the last Enter, escape sequence or reset. */
  private line: string[] = [];
  private inserted: Inserted | null = null;

  get current(): string {
    return this.word.join('');
  }

  /**
   * How many backspaces one Backspace press stands for. A word that came in whole is taken back whole, but a
   * space the keyboard added after it goes first and alone, so a comma or full stop can follow the word
   * ("hello " -> Backspace -> "hello" -> type "," ; a second Backspace instead removes "hello").
   */
  backspaceCount(): number {
    const w = this.inserted;
    if (!w) return 1;
    return w.trailing > 0 ? 1 : w.chars.length;
  }

  /**
   * iOS puts a space before a swiped word when the text before the caret ends in a character it thinks ends a word. After
   * `device-` or `src/` the word continues, so the space is wrong. Drops the leading space of a whole inserted word when
   * the word being typed ends in a joining character; a space typed on its own is never touched.
   */
  dropSpuriousSpace(data: string): string {
    const last = this.word[this.word.length - 1];
    if (!last || !JOINERS.has(last) || !data.startsWith(' ')) return data;
    const m = WHOLE_WORD.exec(data);
    return m && [...m[1]].length > 1 ? data.slice(1) : data;
  }

  /**
   * What the hidden input should hold: the last two words, not only the one being typed. iOS offers a replacement for
   * a phrase ("app works" -> "api works") by selecting it in the field; if the field held just the last word, the
   * selection could not cover the first one and the replacement deleted too little.
   */
  get phrase(): string {
    const m = PHRASE.exec(this.line.join(''));
    return m ? m[0] : '';
  }

  /**
   * The text a replacement of `data` covers, when the field gives no selection to read: the last as many words as
   * `data` has (at most two, the phrase the mirror shows), without the whitespace after them, which is returned
   * apart so it can be typed again after the replacement (the caret sits behind it).
   */
  replaced(data: string): { text: string; trailing: string } {
    const phrase = this.phrase;
    const trailing = /[ \t]*$/u.exec(phrase)![0];
    const body = phrase.slice(0, phrase.length - trailing.length);
    const want = Math.min(2, data.trim().split(/\s+/u).length);
    const words = body.split(/([ \t]+)/u); // words and their separators alternate
    const text = words.slice(-(2 * want - 1)).join('');
    return { text, trailing };
  }

  /** The script of the last letter typed on this line; null when there is none (an empty line, digits only). */
  get script(): 'latin' | 'cyrillic' | null {
    for (let i = this.line.length - 1; i >= 0; i--) {
      const c = this.line[i];
      if (/\p{Script=Cyrillic}/u.test(c)) return 'cyrillic';
      if (/\p{Script=Latin}/u.test(c)) return 'latin';
    }
    return null;
  }

  /** Forget that the last word came in whole (a paste, or anything that is not the keyboard's own insertion). */
  clearInsert(): void {
    this.inserted = null;
  }

  /** Feed every byte string sent to the terminal, in order. */
  feed(data: string): void {
    if (data.startsWith('\x1b')) return this.reset(); // cursor keys and other escape sequences move the cursor
    let rest = '';
    for (const ch of data) {
      if (BACKSPACE.has(ch) && !rest) {
        this.backspace();
        this.line.pop();
      } else {
        rest += ch;
        if (ch === ' ') {
          this.endWord(); // a space ends the word but the line goes on
          this.line.push(ch);
        } else if (ch < ' ' || BACKSPACE.has(ch)) {
          this.reset(); // Enter, Tab and other control characters end the line
        } else {
          this.word.push(ch);
          this.line.push(ch);
        }
      }
    }
    if (this.line.length > LINE_CAP) this.line.splice(0, this.line.length - LINE_CAP);
    if (rest) this.noteInsert(rest);
  }

  private backspace(): void {
    const w = this.inserted;
    if (!w) {
      this.word.pop();
      return;
    }
    if (w.trailing > 0) {
      w.trailing--;
      if (w.trailing === 0) this.word = [...w.chars]; // the cursor is back at the end of the word
    } else {
      this.word.pop();
      w.chars.pop();
      if (!w.chars.length) this.inserted = null;
    }
  }

  /** `text` is what followed any backspaces of one send: remember it when it is a single word of two or more characters. */
  private noteInsert(text: string): void {
    const m = WHOLE_WORD.exec(text);
    this.inserted = m && [...m[1]].length > 1 && ![...m[1]].some((c) => c < ' ' || BACKSPACE.has(c)) ? { chars: [...m[1]], trailing: m[2].length } : null;
    if (this.inserted && this.inserted.trailing > 0) this.word = [];
  }

  private endWord(): void {
    this.word = [];
    this.inserted = null;
  }

  reset(): void {
    this.endWord();
    this.line = [];
  }

  /**
   * How many characters to delete before typing `incoming`, when it is a suggestion that completes the word being
   * typed ("утро" for "у"): the length of the word, else 0. Case is ignored ("Утром" completes "у").
   */
  completionPrefix(incoming: string): number {
    const typed = this.word.length;
    if (typed === 0 || [...incoming].length <= typed) return 0;
    return incoming.toLowerCase().startsWith(this.current.toLowerCase()) ? typed : 0;
  }
}
