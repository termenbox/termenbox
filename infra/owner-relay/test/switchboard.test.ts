import { describe, expect, it } from 'vitest';
import { IDLE_MS, MAX_CLIENTS, MAX_MESSAGE, RATE_PER_MIN, Switchboard, type Sock } from '../src/switchboard';

class Fake implements Sock {
  got: Uint8Array[] = [];
  closed: [number, string] | null = null;
  send(d: Uint8Array) { this.got.push(Uint8Array.from(d)); }
  close(code: number, reason: string) { this.closed = [code, reason]; }
}
const u = (...b: number[]) => Uint8Array.from(b);

function room() {
  const board = new Switchboard();
  const server = new Fake();
  board.addServer(server);
  return { board, server };
}

describe('the owner relay switchboard', () => {
  it('a client message reaches the server with the client id, and the reply returns to that client only', () => {
    const { board, server } = room();
    const a = new Fake(), b = new Fake();
    const ia = board.addClient(a, '1.1.1.1', 0)!, ib = board.addClient(b, '2.2.2.2', 0)!;
    expect(ia).not.toBe(ib);
    expect(board.onClient(ia, u(9, 8, 7), 1)).toBe('ok');
    expect(server.got).toEqual([u(ia, 9, 8, 7)]);
    expect(board.onServer(u(ia, 5, 5))).toBe('ok');
    expect(a.got).toEqual([u(5, 5)]);
    expect(b.got).toEqual([]);
    expect(board.onServer(u(200, 1))).toBe('unknown-client');
    expect(board.onServer(u())).toBe('unknown-client');
  });

  it('drops an oversize message in either direction and keeps the limit exact', () => {
    const { board, server } = room();
    const c = new Fake();
    const id = board.addClient(c, 'x', 0)!;
    expect(board.onClient(id, new Uint8Array(MAX_MESSAGE + 1), 1)).toBe('oversize');
    expect(server.got).toEqual([]);
    expect(board.onClient(id, new Uint8Array(MAX_MESSAGE), 1)).toBe('ok');
    const big = new Uint8Array(MAX_MESSAGE + 2); big[0] = id;
    expect(board.onServer(big)).toBe('oversize');
    expect(c.got).toEqual([]);
  });

  it('allows at most 20 messages a minute per source, across its sockets, and counts again after the minute', () => {
    const { board, server } = room();
    const first = board.addClient(new Fake(), 'ip', 0)!; // the connect counts as one
    for (let i = 1; i < RATE_PER_MIN; i++) expect(board.onClient(first, u(1), 1000), String(i)).toBe('ok');
    expect(board.onClient(first, u(1), 2000)).toBe('rate');
    expect(board.addClient(new Fake(), 'ip', 3000)).toBeNull(); // reconnecting does not reset it
    expect(board.addClient(new Fake(), 'other-ip', 3000)).not.toBeNull();
    expect(server.got).toHaveLength(RATE_PER_MIN - 1);
    expect(board.onClient(first, u(1), 61_000)).toBe('ok');
  });

  it('closes a client that has been idle for 60 s and keeps an active one', () => {
    const { board } = room();
    const idle = new Fake(), busy = new Fake();
    board.addClient(idle, 'a', 0);
    const b = board.addClient(busy, 'b', 0)!;
    board.onClient(b, u(1), 50_000);
    board.sweep(IDLE_MS);
    expect(idle.closed).toEqual([1000, 'idle']);
    expect(busy.closed).toBeNull();
    expect(board.clientCount).toBe(1);
  });

  it('a new server socket replaces the old; with no server a client message is not forwarded', () => {
    const { board, server } = room();
    const id = board.addClient(new Fake(), 'a', 0)!;
    const fresh = new Fake();
    board.addServer(fresh);
    expect(server.closed).toEqual([1000, 'replaced']);
    board.onClient(id, u(1), 1);
    expect(fresh.got).toEqual([u(id, 1)]);
    board.remove(fresh);
    expect(board.onClient(id, u(1), 2)).toBe('no-server');
  });

  it('caps the clients in the room and frees an id when a client leaves', () => {
    const { board } = room();
    const socks = Array.from({ length: MAX_CLIENTS }, (_, i) => new Fake());
    socks.forEach((s, i) => expect(board.addClient(s, `ip${i}`, 0)).not.toBeNull());
    expect(board.addClient(new Fake(), 'late', 0)).toBeNull();
    board.remove(socks[0]);
    expect(board.addClient(new Fake(), 'late', 0)).not.toBeNull();
  });
});
