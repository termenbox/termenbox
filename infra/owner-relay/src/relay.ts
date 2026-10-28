// The Durable Object: one per Termenbox server, named by the hash of its public key. A thin adapter that gives
// `Switchboard` hibernating WebSockets. Per-socket facts (role, id, source, last activity) live in the socket
// attachment, which hibernation keeps; nothing is written to storage and no message is kept.
import { IDLE_MS, Switchboard, type Sock } from './switchboard';

interface Attachment { role: 'server' | 'client'; id: number; source: string; last: number }
type WS = Sock & { serializeAttachment(v: unknown): void; deserializeAttachment(): Attachment | null };

interface State {
  acceptWebSocket(ws: unknown, tags?: string[]): void;
  getWebSockets(tag?: string): WS[];
  storage: { setAlarm(at: number): Promise<void> };
}

const bytes = (m: string | ArrayBuffer): Uint8Array => (typeof m === 'string' ? new TextEncoder().encode(m) : new Uint8Array(m));

export class Relay {
  private board = new Switchboard();

  constructor(private ctx: State, _env?: unknown) {
    // Woken from hibernation: the sockets are still open, the memory is gone.
    for (const ws of ctx.getWebSockets()) {
      const a = ws.deserializeAttachment();
      if (a) this.board.restore(a.role, ws, a.id, a.source, a.last);
    }
  }

  async fetch(req: Request): Promise<Response> {
    if (req.headers.get('upgrade')?.toLowerCase() !== 'websocket') return new Response('expected a WebSocket', { status: 426 });
    const source = req.headers.get('x-source') ?? 'unknown';
    const role = new URL(req.url).searchParams.get('role') === 'server' ? 'server' : 'client';
    const { 0: client, 1: server } = new (globalThis as any).WebSocketPair() as { 0: WS; 1: WS };
    const now = Date.now();
    let id = 0;
    if (role === 'server') this.board.addServer(server);
    else {
      const got = this.board.addClient(server, source, now);
      if (got === null) return new Response('too many requests', { status: 429 });
      id = got;
    }
    this.ctx.acceptWebSocket(server, [role]);
    server.serializeAttachment({ role, id, source, last: now } satisfies Attachment);
    if (role === 'client') await this.ctx.storage.setAlarm(now + IDLE_MS);
    return new Response(null, { status: 101, webSocket: client } as ResponseInit);
  }

  webSocketMessage(ws: WS, message: string | ArrayBuffer): void {
    const a = ws.deserializeAttachment();
    if (!a) return;
    const now = Date.now();
    const data = bytes(message);
    if (a.role === 'server') this.board.onServer(data);
    else {
      this.board.onClient(a.id, data, now);
      ws.serializeAttachment({ ...a, last: now });
    }
  }

  webSocketClose(ws: WS): void { this.board.remove(ws); }
  webSocketError(ws: WS): void { this.board.remove(ws); }

  async alarm(): Promise<void> {
    this.board.sweep(Date.now());
    if (this.board.clientCount > 0) await this.ctx.storage.setAlarm(Date.now() + IDLE_MS / 2);
  }
}
