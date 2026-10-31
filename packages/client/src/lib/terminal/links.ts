// Tapping a web address in the terminal opens it in the browser. Agents print long URLs that wrap over several
// rows, so the address is looked up in the whole wrapped line, not just the tapped row.

/** The slice of xterm's buffer this needs, so a test can stand in for it. */
export interface LineBuffer {
  getLine(y: number): { isWrapped: boolean; translateToString(trimRight?: boolean): string } | undefined;
}

const URL_RE = /https?:\/\/[^\s<>"'`\\^{|}]+/g;
const TRAILING = /[.,;:!?'"\]}>]$/;

/** Drops the punctuation that ends a sentence rather than the address; keeps a ")" that closes a "(" inside it. */
export function trimUrl(url: string): string {
  let u = url;
  for (;;) {
    if (TRAILING.test(u)) u = u.slice(0, -1);
    else if (u.endsWith(')') && (u.match(/\(/g)?.length ?? 0) < (u.match(/\)/g)?.length ?? 0)) u = u.slice(0, -1);
    else return u;
  }
}

/** The web address covering character `offset` of `text`, if any. */
export function urlInText(text: string, offset: number): string | null {
  for (const m of text.matchAll(URL_RE)) {
    const url = trimUrl(m[0]);
    if (offset >= m.index && offset < m.index + url.length) return /^https?:\/\/[^/?#]+/.test(url) ? url : null;
  }
  return null;
}

const OPEN_URL = /https?:\/\/\S*$/;
/** Most rows looked back over for the start of a link that no row is marked as wrapping into. */
const LOOKBACK = 50;
/** How far from the right edge a row may stop and still have been cut by a wrap rather than ended. */
const EDGE = 4;

/**
 * The web address under cell (`col`, `row`) of the buffer, following rows that wrap into each other. Programs in tmux
 * (Claude Code) break long lines themselves, so those rows carry no wrap mark: they are indented, and stop a column or
 * two short of the edge. A row whose last word is an open address and which stops within `EDGE` columns of the edge
 * therefore continues on a next row with the same indent that holds a single word.
 */
export function linkAt(buffer: LineBuffer, cols: number, row: number, col: number): string | null {
  const raw = (y: number) => (buffer.getLine(y)?.translateToString(false) ?? '').padEnd(cols, ' ').slice(0, cols);
  const used = (y: number) => raw(y).trimEnd().length;
  const indent = (y: number) => raw(y).length - raw(y).trimStart().length;
  const marked = (y: number) => !!buffer.getLine(y)?.isWrapped && !!buffer.getLine(y - 1);
  /** Row `y` is a hard-wrapped continuation of row `y - 1`, judged by the two rows alone. */
  const alike = (y: number) =>
    !!buffer.getLine(y - 1) && used(y - 1) >= cols - EDGE && used(y) > indent(y) && indent(y) === indent(y - 1) && !/\s/.test(raw(y).trim());
  let first = row;
  while (first > 0 && row - first < LOOKBACK && (marked(first) || alike(first))) first--;
  let text = '';
  let start = first;
  let skip = 0; // columns of the clicked row left out of `text`: its indent
  let offset = 0; // where the clicked row's first kept cell sits in `text`
  for (let y = first; buffer.getLine(y); y++) {
    let lead = 0;
    if (y > start) {
      const soft = !marked(y) && alike(y) && OPEN_URL.test(text.trimEnd());
      if (!marked(y) && !soft) {
        if (y > row) break;
        start = y;
        text = '';
      } else if (soft) {
        text = text.trimEnd();
        lead = indent(y);
      }
    }
    if (y === row) {
      skip = lead;
      offset = text.length;
    }
    text += raw(y).slice(lead);
  }
  return col < skip ? null : urlInText(text, offset + col - skip);
}

/** A click that opens the web address under it: Ctrl or Cmd held, so a plain click stays with the terminal or its app. */
export function isOpenClick(e: { button: number; ctrlKey: boolean; metaKey: boolean }): boolean {
  return e.button === 0 && (e.ctrlKey || e.metaKey);
}
