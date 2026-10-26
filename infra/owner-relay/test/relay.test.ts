import { describe, expect, it } from 'vitest';
import { Relay } from '../src/relay';
import worker from '../src/worker';
import { IDLE_MS } from '../src/switchboard';

/** A hibernating socket as the runtime hands it: it keeps its attachment, never a message. */
class Ws {
  att: any = null;
  sent: Uint8Array[] = [];
  closed: [number, string] | null = null;
  send(d: Uint8Array) { this.sent.push(Uint8Array.from(d)); }
  close(c: number, r: string) { this.closed = [c, r]; }
  serializeAttachment(v: unknown) { this.att = structuredClone(v); }
  deserializeAttachment() { return this.att; }
}

function ctxOf(sockets: Ws[]) {
  const storage = { puts: 0, alarms: [] as number[], async setAlarm(at: number) { this.alarms.push(at); }, put() { this.puts++; } };
  return { storage, ctx: { storage, getWebSockets: () => sockets, acceptWebSocket: () => {} } as any };
}

describe('the Durable Object adapter', () => {
  it('survives hibernation: a woken object routes by the sockets attachments and persists no message', () => {
    const server = new Ws(), client = new Ws();
    server.att = { role: 'server', id: 0, source: 's', last: 0 };
    client.att = { role: 'client', id: 3, source: 'c', last: 0 };
    const { storage, ctx } = ctxOf([server, client]);
    const woken = new Relay(ctx); // a fresh instance, as after hibernation

    woken.webSocketMessage(client, new Uint8Array([7, 7]).buffer);
    expect(server.sent).toEqual([Uint8Array.from([3, 7, 7])]);
    woken.webSocketMessage(server, new Uint8Array([3, 1]).buffer);
    expect(client.sent).toEqual([Uint8Array.from([1])]);
    // Only the activity time is remembered on the socket; no payload anywhere, nothing in storage.
    expect(Object.keys(client.att).sort()).toEqual(['id', 'last', 'role', 'source']);
    expect(JSON.stringify(client.att)).not.toContain('7,7');
    expect(storage.puts).toBe(0);
  });

  it('text frames are forwarded as their UTF-8 bytes', () => {
    const server = new Ws(), client = new Ws();
    server.att = { role: 'server', id: 0, source: 's', last: 0 };
    client.att = { role: 'client', id: 1, source: 'c', last: 0 };
    const r = new Relay(ctxOf([server, client]).ctx);
    r.webSocketMessage(client, 'hi');
    expect(server.sent).toEqual([Uint8Array.from([1, 104, 105])]);
  });

  it('the alarm closes idle clients and stops when none are left', async () => {
    const server = new Ws(), idle = new Ws();
    server.att = { role: 'server', id: 0, source: 's', last: 0 };
    idle.att = { role: 'client', id: 1, source: 'c', last: Date.now() - IDLE_MS - 1 };
    const { storage, ctx } = ctxOf([server, idle]);
    await new Relay(ctx).alarm();
    expect(idle.closed).toEqual([1000, 'idle']);
    expect(storage.alarms).toEqual([]);
  });

  it('a closed socket leaves the room', () => {
    const server = new Ws(), client = new Ws();
    server.att = { role: 'server', id: 0, source: 's', last: 0 };
    client.att = { role: 'client', id: 1, source: 'c', last: 0 };
    const r = new Relay(ctxOf([server, client]).ctx);
    r.webSocketClose(server);
    r.webSocketMessage(client, new Uint8Array([1]).buffer);
    expect(server.sent).toEqual([]);
  });
});

describe('the Worker route', () => {
  const seen: Request[] = [];
  const env = { RELAY: { idFromName: (n: string) => n, get: (id: unknown) => ({ fetch: async (r: Request) => { seen.push(r); return new Response(`object ${id}`); } }) } };
  const KEY = 'a'.repeat(43);
  const ws = (path: string, headers: Record<string, string> = {}, method = 'GET') =>
    worker.fetch(new Request(`https://relay.example.workers.dev${path}`, { method, headers }), env);

  it('only GET /s/<keyhash> with a WebSocket upgrade reaches the object, keyed by the hash', async () => {
    expect((await ws('/')).status).toBe(404);
    expect((await ws('/s/short')).status).toBe(404);
    expect((await ws(`/s/${KEY}/x`)).status).toBe(404);
    expect((await ws(`/s/${KEY}`, {}, 'POST')).status).toBe(404);
    expect((await ws(`/s/${KEY}`)).status).toBe(426);
    expect(seen).toHaveLength(0);
    const ok = await ws(`/s/${KEY}?role=server`, { upgrade: 'websocket' });
    expect(await ok.text()).toBe(`object ${KEY}`);
    expect(new URL(seen[0].url).searchParams.get('role')).toBe('server');
  });

  it('the source address is Cloudflares, and a caller cannot set it', async () => {
    seen.length = 0;
    await ws(`/s/${KEY}`, { upgrade: 'websocket', 'x-source': 'forged', 'cf-connecting-ip': '198.51.100.7' });
    expect(seen[0].headers.get('x-source')).toBe('198.51.100.7');
    seen.length = 0;
    await ws(`/s/${KEY}`, { upgrade: 'websocket', 'x-source': 'forged' });
    expect(seen[0].headers.get('x-source')).toBe('unknown');
  });
});
