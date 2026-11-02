// Copying rejoins the rows of one logical line. xterm joins two rows only when the second is marked wrapped, but
// programs in tmux (Claude Code) and tmux itself repaint every row with cursor moves, so the marks are missing and a
// plain copy would end every visible row with a newline. One function decides, for every copy path.

/** One buffer row: `text` is the part to copy, `full` the whole row (default `text`), which the join test reads. */
export interface Row {
  text: string;
  full?: string;
  wrapped?: boolean;
}

/** The slice of xterm's terminal the selection copy needs, so a test can stand in for it. */
export interface CopyTerm {
  cols: number;
  buffer: {
    active: { getLine(y: number): { isWrapped: boolean; translateToString(trimRight?: boolean): string } | undefined };
  };
  getSelection(): string;
  getSelectionPosition?(): { start: { x: number; y: number }; end: { x: number; y: number } } | undefined;
}

const MARKER = /^\s*(?:[-*•●]|\d+[.)])\s+/;
const BULLET = /^\s*(?:[-*•●]|\d+[.)])\s/;

/** The column where a row's text starts, after its list marker if it has one. */
function textStart(full: string): number {
  const m = MARKER.exec(full);
  return m ? m[0].length : full.length - full.trimStart().length;
}

/** Row `next` continues row `cur`: it starts at the same text column and `cur` would not have fitted its first word. */
function continues(cur: string, next: string, cols: number): boolean {
  const used = cur.trimEnd().length;
  const word = /\S+/.exec(next)?.[0];
  if (!used || !word || BULLET.test(next)) return false;
  return textStart(cur) === textStart(next) && used + 1 + word.length >= cols;
}

const blanks = (s: string) => s.replace(/[ \t]+$/, '');

/** The rows as text: rows that belong to one line are joined with a space (or directly when marked wrapped). */
export function joinRows(rows: Row[], cols: number): string {
  let out = '';
  rows.forEach((row, i) => {
    const prev = rows[i - 1];
    if (!prev) {
      out = row.text;
    } else if (row.wrapped) {
      out += row.text; // xterm's own join: the previous row is kept whole, trailing blanks included
    } else if (continues(prev.full ?? prev.text, row.full ?? row.text, cols)) {
      out = blanks(out) + ' ' + row.text.trimStart();
    } else {
      out = blanks(out) + '\n' + row.text;
    }
  });
  return out.replace(/\s+$/, '');
}

/** The selected text with the rows of one line joined; the plain `getSelection()` when the position is unknown. */
export function selectedText(term: CopyTerm): string {
  const pos = term.getSelectionPosition?.();
  if (!pos) return term.getSelection();
  const { start, end } = pos;
  if (end.y < start.y || (end.y === start.y && end.x <= start.x)) return term.getSelection();
  const rows: Row[] = [];
  for (let y = start.y; y <= end.y; y++) {
    const line = term.buffer.active.getLine(y);
    if (!line) return term.getSelection();
    const full = line.translateToString(false).padEnd(term.cols, ' ').slice(0, term.cols);
    const from = y === start.y ? start.x : 0;
    const to = y === end.y ? end.x : term.cols;
    rows.push({ text: full.slice(from, to), full, wrapped: y > start.y && line.isWrapped });
  }
  return joinRows(rows, term.cols);
}

/** Rows of an xterm buffer as `Row`s, whole and untrimmed, for `bufferText`. */
export function bufferRow(buf: CopyTerm['buffer']['active']): (i: number) => Row | undefined {
  return (i) => {
    const line = buf.getLine(i);
    return line && { text: line.translateToString(false), wrapped: line.isWrapped };
  };
}
