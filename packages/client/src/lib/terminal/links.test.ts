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
  it('does not join rows that did not wrap and the link does not fill', () => {
    const b = buffer(['https://a.dev/x', 'tail'], 20);
    expect(linkAt(b, 20, 0, 1)).toBe('https://a.dev/x');
    expect(linkAt(b, 20, 1, 1)).toBeNull();
  });
  it('joins a link a program or tmux repainted over full rows without marking them wrapped', () => {
    const url = 'https://example.com/login?session=3f9c&redirect=%2Fdash&utm_source=termenbox&ref=beta14';
    const cols = 30;
    const rows = [url.slice(0, 30), url.slice(30, 60), url.slice(60)];
    const b = buffer(rows, cols);
    for (const [row, col] of [[0, 3], [1, 10], [2, 2]]) expect(linkAt(b, cols, row, col)).toBe(url);
  });
  it('stops joining at a row that does not start with the link', () => {
    const b = buffer(['see https://a.dev/aaaaaaaaaaaaaa', ' next words'], 32);
    expect(linkAt(b, 32, 0, 6)).toBe('https://a.dev/aaaaaaaaaaaaaa');
  });
});

describe('linkAt on rows Claude Code draws', () => {
  // Captured from a 194-column pane: the log holds the URL as one line, Claude Code breaks it into indented rows
  // that stop at column 193, with no wrap mark.
  const rows = [
    "  https://example.com/oauth/authorize?response_type=code&client_id=termenbox-test-0123456789abcdef&redirect_uri=https%3A%2F%2Fapp.example.com%2Fcallback%2Fgithub%3Fworkspace%3Dws-07bbf3bb7cf546",
    "  39be81cc9151e7ebbc%26tab%3D1&scope=read%3Auser+user%3Aemail+repo&state=9a8b7c6d5e4f30211a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw",
    "  -cM&code_challenge_method=S256&utm_source=termenbox&utm_medium=terminal&utm_campaign=long-link-test&utm_content=six-row-wrap-check&nonce=c0ffee1234567890deadbeefc0ffee1234567890&ref=beta16",
  ];
  const full = "https://example.com/oauth/authorize?response_type=code&client_id=termenbox-test-0123456789abcdef&redirect_uri=https%3A%2F%2Fapp.example.com%2Fcallback%2Fgithub%3Fworkspace%3Dws-07bbf3bb7cf54639be81cc9151e7ebbc%26tab%3D1&scope=read%3Auser+user%3Aemail+repo&state=9a8b7c6d5e4f30211a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256&utm_source=termenbox&utm_medium=terminal&utm_campaign=long-link-test&utm_content=six-row-wrap-check&nonce=c0ffee1234567890deadbeefc0ffee1234567890&ref=beta16";
  it('opens the whole address from any of its rows', () => {
    const b = buffer(rows, 194);
    for (const [row, col] of [[0, 2], [0, 100], [0, 190], [1, 2], [1, 190], [2, 5], [2, 100], [2, 189]]) expect(linkAt(b, 194, row, col), `${row}:${col}`).toBe(full);
  });
  it('does not take the indentation as part of the link', () => {
    expect(linkAt(buffer(rows, 194), 194, 1, 0)).toBeNull();
  });
  it('leaves a short word on the next row alone when the link row is not near the edge', () => {
    const b = buffer(['  see https://a.dev/x', '  Done'], 194);
    expect(linkAt(b, 194, 0, 8)).toBe('https://a.dev/x');
    expect(linkAt(b, 194, 1, 3)).toBeNull();
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
