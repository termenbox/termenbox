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

/**
 * The web address under cell (`col`, `row`) of the buffer, following rows that wrap into each other. tmux and the TUIs
 * in it (Claude Code) repaint long lines without the wrap mark, so a row that is full to the last column while its text
 * ends inside an address also continues on a next row that starts with a non-space.
 */
export function linkAt(buffer: LineBuffer, cols: number, row: number, col: number): string | null {
  const rowText = (y: number) => (buffer.getLine(y)?.translateToString(false) ?? '').padEnd(cols, ' ').slice(0, cols);
  const fills = (y: number) => rowText(y)[cols - 1] !== ' ';
  const marked = (y: number) => !!buffer.getLine(y)?.isWrapped && !!buffer.getLine(y - 1);
  let first = row;
  while (first > 0 && row - first < LOOKBACK && (marked(first) || (buffer.getLine(first - 1) && fills(first - 1) && rowText(first)[0] !== ' '))) first--;
  let start = first;
  let text = '';
  for (let y = first; ; y++) {
    if (!buffer.getLine(y)) break;
    if (y > start && !marked(y) && !(OPEN_URL.test(text) && fills(y - 1) && rowText(y)[0] !== ' ')) {
      if (y > row) break;
      start = y;
      text = '';
    }
    text += rowText(y);
  }
  return urlInText(text, (row - start) * cols + col);
}

/** A click that opens the web address under it: Ctrl or Cmd held, so a plain click stays with the terminal or its app. */
export function isOpenClick(e: { button: number; ctrlKey: boolean; metaKey: boolean }): boolean {
  return e.button === 0 && (e.ctrlKey || e.metaKey);
}
