import { afterEach, describe, expect, it, vi } from 'vitest';
import { openWs } from './ws';
import { racePaths, type Path } from './paths';

class FakeWebSocket {
  static OPEN = 1;
  static last: FakeWebSocket;
  readyState = 0;
  binaryType = '';
  closed = 0;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((e: unknown) => void) | null = null;
  constructor(readonly url: string) { FakeWebSocket.last = this; }
  open() { this.readyState = 1; this.onopen?.(); }
  close() { this.closed++; this.readyState = 3; }
  send() {}
}

afterEach(() => vi.unstubAllGlobals());

describe('openWs under a race', () => {
  it('a tunnel that wins the race stays open: only the losers are cancelled', async () => {
    vi.stubGlobal('WebSocket', FakeWebSocket);
    const tunnel: Path = { key: 'tunnel wss://t.example/ws', kind: 'tunnel', label: 'Tunnel', addr: 'wss://t.example/ws', port: 443, url: 'wss://t.example/ws' };
    const won = racePaths([tunnel], (p, signal) => {
      const t = openWs(p.url!, 5000, signal);
      queueMicrotask(() => FakeWebSocket.last.open());
      return t;
    });
    const { value } = await won;
    await new Promise((r) => setTimeout(r, 0));
    expect(FakeWebSocket.last.closed).toBe(0);
    expect(FakeWebSocket.last.readyState).toBe(1);
    value.close();
  });

  it('a path that lost is still closed when the race is cancelled before it opens', async () => {
    vi.stubGlobal('WebSocket', FakeWebSocket);
    const ctrl = new AbortController();
    const p = openWs('wss://t.example/ws', 5000, ctrl.signal);
    const sock = FakeWebSocket.last;
    ctrl.abort();
    await expect(p).rejects.toThrow('cancelled');
    expect(sock.closed).toBe(1);
  });
});
