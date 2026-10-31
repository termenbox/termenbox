import type { OscParser } from './inputGuards';

/** The private OSC number a program in a session uses to ask the browser to open a web page (see `termenbox-open`). */
export const OPEN_OSC = 7770;

/**
 * The web address of an open request (`open;URL`), or null for anything else. Only `http(s)` is honoured and the address
 * carries no whitespace or control characters: a program in the terminal must not be able to open another scheme.
 */
export function openRequestUrl(data: string): string | null {
  if (!data.startsWith('open;')) return null;
  const url = data.slice('open;'.length);
  return /^https?:\/\/[^/?#\s]+\S*$/.test(url) && !/[\u0000-\u001f\u007f]/.test(url) ? url : null;
}

/** Lets a CLI on the server (`gh auth login`, `claude login`) open its login page on this device. */
export function handleOpenRequests(parser: OscParser, open: (url: string) => void): { dispose(): void } {
  const handle = parser.registerOscHandler(OPEN_OSC, (data) => {
    const url = openRequestUrl(data);
    if (url) open(url);
    return true;
  });
  return { dispose: () => handle.dispose() };
}

/**
 * Opens a web address in a new window; false when it is not a web address or the browser blocked the window. `noopener`
 * cannot be asked for here: the browser then returns null for a window it did open, which looks like a block.
 */
export function openWeb(url: string, win: { open(url: string, target: string, features?: string): Window | null } = window): boolean {
  if (!/^https?:\/\/[^/?#\s]+/.test(url)) return false;
  const opened = win.open(url, '_blank');
  if (!opened) return false;
  try {
    opened.opener = null;
  } catch {
    /* a window we cannot reach is one that has no link back to us */
  }
  return true;
}

/**
 * xterm's handler for OSC 8 hyperlinks (Claude Code prints long URLs as a shortened text with the address behind it).
 * Its own default asks `confirm()` first; this opens an `http(s)` target at once and ignores every other scheme.
 */
export function hyperlinkHandler(open: (url: string) => void): {
  activate(event: MouseEvent, text: string, range: unknown): void;
  allowNonHttpProtocols: boolean;
} {
  return {
    allowNonHttpProtocols: false,
    activate(event, text) {
      event.preventDefault();
      if (/^https?:\/\/[^/?#\s]+/.test(text)) open(text);
    },
  };
}
