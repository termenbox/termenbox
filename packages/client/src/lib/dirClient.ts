import type { ClientMessage, DirEntry, ServerMessage } from './protocol';

export interface DirListing { path: string; parent?: string | null; entries: DirEntry[]; truncated?: boolean; home: string }
export type DirReply = ({ ok: true } & DirListing) | { ok: false; reason: string };
export type ListDir = (req: { path?: string; prefix?: string; hidden?: boolean }) => Promise<DirReply>;

/**
 * `list_dir` over the hub. Replies carry no request id, but a server answers in order, so the oldest request waits for the
 * next `dir_listing` / `dir_error`. Callers that send a newer request ignore the older answer themselves.
 */
export class DirClient {
  private waiting: ((r: DirReply) => void)[] = [];
  constructor(private send: (m: ClientMessage) => void) {}
  readonly list: ListDir = (req) => new Promise(resolve => {
    this.waiting.push(resolve);
    this.send({ type: 'list_dir', ...req });
  });
  /** Feed every server message; returns true when it was a directory reply. */
  handle(msg: ServerMessage): boolean {
    if (msg.type !== 'dir_listing' && msg.type !== 'dir_error') return false;
    const next = this.waiting.shift();
    if (msg.type === 'dir_listing') next?.({ ok: true, path: msg.path, parent: msg.parent, entries: msg.entries, truncated: msg.truncated, home: msg.home });
    else next?.({ ok: false, reason: msg.reason });
    return true;
  }
  /** The connection dropped: answer everything waiting with an error so no picker spins forever. */
  fail(reason: string) {
    for (const w of this.waiting.splice(0)) w({ ok: false, reason });
  }
}
