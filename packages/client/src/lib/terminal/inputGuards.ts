// Small fixes for the terminal's hidden input on phones.

/** The keyboard's return key says "return" rather than "OK/Done". */
export function setEnterKeyHint(area: HTMLTextAreaElement): void {
  area.setAttribute('enterkeyhint', 'enter');
}

/**
 * Colour-query answers (OSC 4 palette entries, OSC 10-19 dynamic colours) that xterm generates by itself. tmux relays an application's query to
 * every attached client, so with the workspace open in two places the second answer reaches the shell as typed
 * text. tmux answers these queries for the pane on its own, so the browser's answer is dropped.
 */
// eslint-disable-next-line no-control-regex
const COLOR_REPLY = /\x1b\](?:4;\d+|1[0-9]);rgb:[^\x07\x1b]*(?:\x07|\x1b\\)/g;

export function dropColorReplies(data: string): string {
  return data.replace(COLOR_REPLY, '');
}

/** The slice of xterm's parser this needs, so a test can stand in for it. */
export interface OscParser {
  registerOscHandler(ident: number, callback: (data: string) => boolean): { dispose(): void };
}

/**
 * Stops xterm answering colour queries at all (OSC 4, 10, 11, 12 with a `?`): the answer was never typed by
 * anyone and, with the workspace open twice, one of them lands in the shell. Setting a colour (no `?`) is left to
 * xterm. A handler that returns true ends the sequence without the built-in reply.
 */
export function silenceColorQueries(parser: OscParser): { dispose(): void } {
  const handles = [4, 10, 11, 12].map((ident) => parser.registerOscHandler(ident, (data) => data.split(';').includes('?')));
  return { dispose: () => handles.forEach((h) => h.dispose()) };
}
