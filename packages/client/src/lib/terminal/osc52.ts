import type { OscParser } from './inputGuards';

/** Most a program may put on the clipboard at once (base64 characters); a runaway or hostile stream is dropped. */
export const MAX_OSC52_BASE64 = 1_400_000;

/**
 * The text of an OSC 52 "set clipboard" payload (`Pc;Pd`), or null when it is a query (`?`), empty (a clear), not
 * base64, or too large. Only writes are honoured: a program in the terminal must never read the user's clipboard.
 */
export function osc52Text(data: string): string | null {
  const at = data.indexOf(';');
  if (at < 0) return null;
  const payload = data.slice(at + 1);
  if (!payload || payload === '?' || payload.length > MAX_OSC52_BASE64) return null;
  try {
    const bin = atob(payload);
    return new TextDecoder('utf-8', { fatal: false }).decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)));
  } catch {
    return null;
  }
}

/**
 * Lets tmux and the programs in it (opencode, Claude Code, vim) put text on this machine's clipboard with OSC 52, which
 * is how a copy made on the server crosses to the browser. `copy` receives the text; the sequence is always consumed.
 */
export function handleClipboardWrites(parser: OscParser, copy: (text: string) => void): { dispose(): void } {
  const handle = parser.registerOscHandler(52, (data) => {
    const text = osc52Text(data);
    if (text) copy(text);
    return true;
  });
  return { dispose: () => handle.dispose() };
}
