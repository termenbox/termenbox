import { describe, expect, it } from 'vitest';
import { isOpenClick, linkAt, trimUrl, urlInText, type LineBuffer } from './links';

function buffer(rows: string[], cols: number, wrapped: boolean[] = []): LineBuffer {
  return {
    getLine: (y) => (y < 0 || y >= rows.length ? undefined : { isWrapped: wrapped[y] ?? false, translateToString: () => rows[y].padEnd(cols) }),
  };
}

describe('trimUrl', () => {
  it('drops sentence punctuation', () => {
    expect(trimUrl('https://a.dev/x.')).toBe('https://a.dev/x');
    expect(trimUrl('https://a.dev/x),')).toBe('https://a.dev/x');
  });
  it('keeps a bracket that closes one inside the address', () => {
    expect(trimUrl('https://en.wikipedia.org/wiki/Foo_(bar)')).toBe('https://en.wikipedia.org/wiki/Foo_(bar)');
  });
});

describe('urlInText', () => {
  it('finds the address under the offset only', () => {
    const t = 'open https://a.dev/x?y=1 now';
    expect(urlInText(t, 0)).toBeNull();
    expect(urlInText(t, 5)).toBe('https://a.dev/x?y=1');
    expect(urlInText(t, 25)).toBeNull();
  });
  it('ignores a bare scheme', () => {
    expect(urlInText('see http:// here', 6)).toBeNull();
  });
});

describe('linkAt', () => {
  it('finds a link on one row', () => {
    expect(linkAt(buffer(['go https://a.dev/p ok'], 30), 30, 0, 8)).toBe('https://a.dev/p');
  });
  it('joins a link that wraps, from either row', () => {
    const b = buffer(['visit https://example.com/lo', 'ng/path/here now'], 28, [false, true]);
    expect(linkAt(b, 28, 0, 10)).toBe('https://example.com/long/path/here');
    expect(linkAt(b, 28, 1, 3)).toBe('https://example.com/long/path/here');
    expect(linkAt(b, 28, 1, 14)).toBeNull();
  });
  it('does not join rows that did not wrap', () => {
    const b = buffer(['https://a.dev/x', 'tail'], 15);
    expect(linkAt(b, 15, 1, 1)).toBeNull();
  });
});

describe('isOpenClick', () => {
  it('opens on a primary click with Ctrl or Cmd held, and only then', () => {
    expect(isOpenClick({ button: 0, ctrlKey: true, metaKey: false })).toBe(true);
    expect(isOpenClick({ button: 0, ctrlKey: false, metaKey: true })).toBe(true);
    expect(isOpenClick({ button: 0, ctrlKey: false, metaKey: false })).toBe(false);
    expect(isOpenClick({ button: 2, ctrlKey: true, metaKey: false })).toBe(false);
  });
});
