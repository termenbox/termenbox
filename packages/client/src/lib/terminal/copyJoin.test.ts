import { describe, expect, it } from 'vitest';
import { joinRows, selectedText, type CopyTerm, type Row } from './copyJoin';

const rows = (...t: string[]): Row[] => t.map((text) => ({ text }));

describe('joinRows', () => {
  it('joins a paragraph a program word-wrapped at 40 columns', () => {
    const r = rows('● The quick brown fox jumps over the', '  lazy dog and keeps running far away', '  from home.');
    expect(joinRows(r, 40)).toBe('● The quick brown fox jumps over the lazy dog and keeps running far away from home.');
  });

  it('keeps a real line end when the next word would have fitted', () => {
    expect(joinRows(rows('first line', 'second line'), 40)).toBe('first line\nsecond line');
  });

  it('keeps blank lines and ends a block there', () => {
    const r = rows('● The quick brown fox jumps over the', '  lazy dog.', '', '  Next paragraph');
    expect(joinRows(r, 40)).toBe('● The quick brown fox jumps over the lazy dog.\n\n  Next paragraph');
  });

  it('does not join a list item to the row above', () => {
    const r = rows('- The quick brown fox jumps over the', '- lazy dog and keeps running');
    expect(joinRows(r, 40)).toBe('- The quick brown fox jumps over the\n- lazy dog and keeps running');
  });

  it('joins a bullet with its deeper continuation', () => {
    expect(joinRows(rows('- The quick brown fox jumps over the', '  lazy dog'), 40)).toBe('- The quick brown fox jumps over the lazy dog');
  });

  it('does not join rows of different indent', () => {
    expect(joinRows(rows('The quick brown fox jumps over the', '    lazy dog'), 40)).toBe('The quick brown fox jumps over the\n    lazy dog');
  });

  it('joins a url cut at the full width', () => {
    expect(joinRows(rows('see https://example.com/a/very/long/path/x', 'yz/more'), 40)).toBe('see https://example.com/a/very/long/path/x yz/more');
  });

  it('joins rows marked wrapped without a separator', () => {
    expect(joinRows([{ text: 'abcd' }, { text: 'efgh', wrapped: true }], 4)).toBe('abcdefgh');
  });

  it('judges by the whole row when the selection starts mid-line', () => {
    const r: Row[] = [
      { text: 'brown fox jumps over the', full: '● The quick brown fox jumps over the' },
      { text: '  lazy dog', full: '  lazy dog' },
    ];
    expect(joinRows(r, 40)).toBe('brown fox jumps over the lazy dog');
  });
});

function fake(lines: string[], cols: number, start: [number, number], end: [number, number]): CopyTerm {
  return {
    cols,
    buffer: { active: { getLine: (y) => (lines[y] === undefined ? undefined : { isWrapped: false, translateToString: () => lines[y] }) } },
    getSelection: () => 'raw',
    getSelectionPosition: () => ({ start: { x: start[0], y: start[1] }, end: { x: end[0], y: end[1] } }),
  };
}

describe('selectedText', () => {
  const lines = ['● The quick brown fox jumps over the', '  lazy dog and keeps running far away', '  from home.'];
  it('joins the selected rows', () => {
    expect(selectedText(fake(lines, 40, [0, 0], [40, 2]))).toBe('● The quick brown fox jumps over the lazy dog and keeps running far away from home.');
  });
  it('cuts the first and last row at the selection', () => {
    expect(selectedText(fake(lines, 40, [2, 0], [6, 1]))).toBe('The quick brown fox jumps over the lazy');
  });
  it('falls back to the plain selection without a position', () => {
    const t = fake(lines, 40, [0, 0], [1, 0]);
    t.getSelectionPosition = () => undefined;
    expect(selectedText(t)).toBe('raw');
  });
});
