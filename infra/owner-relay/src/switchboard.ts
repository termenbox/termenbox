// The whole behaviour of the owner's relay, free of the Workers runtime so it is tested in plain Node. The Durable
// Object (`relay.ts`) only maps sockets and hibernation onto this. It stores nothing: no message, no key, no secret.
//
// One `server` socket (the Termenbox server, dialing out) and any number of short-lived `client` sockets meet here.
// A client message reaches the server as [client id][payload]; the server answers with [client id][payload] and only
// that client gets the payload. Everything is untrusted bytes: the signatures are in the payload and checked by the
// server (GUARDRAIL relay-credentials-are-owner-scoped-minted-after-auth-and-capped), not here.

export const MAX_MESSAGE = 8 * 1024;
export const RATE_PER_MIN = 20;
export const IDLE_MS = 60_000;
export const MAX_CLIENTS = 64;
const WINDOW_MS = 60_000;

export interface Sock {
  send(data: Uint8Array): void;
  close(code: number, reason: string): void;
}

export type Verdict = 'ok' | 'oversize' | 'rate' | 'no-server' | 'unknown-client';

interface Client { sock: Sock; source: string; last: number }

export class Switchboard {
  private server: Sock | null = null;
  private clients = new Map<number, Client>();
  /** Message counts per source address in the current minute; memory only, lost on hibernation. */
  private rates = new Map<string, { start: number; count: number }>();

  get clientCount(): number { return this.clients.size; }

  /** Puts back a socket that survived hibernation. */
  restore(role: 'server' | 'client', sock: Sock, id: number, source: string, last: number): void {
    if (role === 'server') this.server = sock;
    else this.clients.set(id, { sock, source, last });
  }

  /** The newest server socket wins: a server that reconnects replaces its stale one. */
  addServer(sock: Sock): void {
    this.server?.close(1000, 'replaced');
    this.server = sock;
  }

  /** Returns the id the server addresses this client by, or null when the source is over its rate or the room is full. */
  addClient(sock: Sock, source: string, now: number): number | null {
    if (this.clients.size >= MAX_CLIENTS || !this.allow(source, now)) return null;
    for (let id = 1; id <= 255; id++) {
      if (!this.clients.has(id)) {
        this.clients.set(id, { sock, source, last: now });
        return id;
      }
    }
    return null;
  }

  private allow(source: string, now: number): boolean {
    for (const [k, r] of this.rates) if (now - r.start >= WINDOW_MS) this.rates.delete(k);
    const r = this.rates.get(source);
    if (!r) { this.rates.set(source, { start: now, count: 1 }); return true; }
    return ++r.count <= RATE_PER_MIN;
  }

  onClient(id: number, data: Uint8Array, now: number): Verdict {
    const c = this.clients.get(id);
    if (!c) return 'unknown-client';
    c.last = now;
    if (data.length > MAX_MESSAGE) return 'oversize';
    if (!this.allow(c.source, now)) return 'rate';
    if (!this.server) return 'no-server';
    const out = new Uint8Array(data.length + 1);
    out[0] = id;
    out.set(data, 1);
    this.server.send(out);
    return 'ok';
  }

  onServer(data: Uint8Array): Verdict {
    if (data.length < 1) return 'unknown-client';
    if (data.length - 1 > MAX_MESSAGE) return 'oversize';
    const c = this.clients.get(data[0]);
    if (!c) return 'unknown-client';
    c.sock.send(data.subarray(1));
    return 'ok';
  }

  /** A socket closed (by either side). */
  remove(sock: Sock): void {
    if (this.server === sock) this.server = null;
    for (const [id, c] of this.clients) if (c.sock === sock) this.clients.delete(id);
  }

  /** Closes clients that have been silent for `IDLE_MS`. */
  sweep(now: number): void {
    for (const [id, c] of this.clients) {
      if (now - c.last >= IDLE_MS) {
        this.clients.delete(id);
        c.sock.close(1000, 'idle');
      }
    }
  }
}
