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

/** The web address under cell (`col`, `row`) of the buffer, following rows that wrap into each other. */
export function linkAt(buffer: LineBuffer, cols: number, row: number, col: number): string | null {
  let first = row;
  while (buffer.getLine(first)?.isWrapped && buffer.getLine(first - 1)) first--;
  let text = '';
  let offset = 0;
  for (let y = first; ; y++) {
    const line = buffer.getLine(y);
    if (!line || (y > first && !line.isWrapped)) break;
    if (y < row) offset += cols;
    text += line.translateToString(false).padEnd(cols, ' ').slice(0, cols);
  }
  return urlInText(text, offset + col);
}

/** A click that opens the web address under it: Ctrl or Cmd held, so a plain click stays with the terminal or its app. */
export function isOpenClick(e: { button: number; ctrlKey: boolean; metaKey: boolean }): boolean {
  return e.button === 0 && (e.ctrlKey || e.metaKey);
}
