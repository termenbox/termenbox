import { describe, it, expect, vi } from 'vitest';
import { openTransport, describeRtcFailure, Unreachable, type Openers, type Transport } from './transport';
import type { DirRecord } from './directory';

const record: DirRecord = {
  server_id: 'A'.repeat(43), seq: 1, ip: '185.199.108.153', udp_port: 4443,
  fingerprint: 'sha-256 ' + Array(32).fill('AB').join(':'), ws_url: '', claim_hash: '', claim_proof: '', sig: 'B'.repeat(86),
};

const fake = (kind: 'rtc' | 'ws'): Transport => ({ kind, onmessage: null, onclose: null, send() {}, close() {} });

function openers(over: Partial<Openers> = {}): Openers & { rtc: ReturnType<typeof vi.fn>; ws: ReturnType<typeof vi.fn> } {
  return { rtc: vi.fn(async () => fake('rtc')), ws: vi.fn(async () => fake('ws')), ...over } as never;
}

describe('openTransport', () => {
  it('prefers WebRTC-direct from the directory record', async () => {
    const o = openers();
    expect((await openTransport({ record, wsUrl: 'https://x.example' }, o)).kind).toBe('rtc');
    expect(o.ws).not.toHaveBeenCalled();
  });

  it('falls back to the record\'s tunnel URL when WebRTC cannot open', async () => {
    const o = openers({ rtc: vi.fn(async () => { throw new Error('no data channel'); }) });
    const t = await openTransport({ record: { ...record, ws_url: 'https://t.trycloudflare.com' } }, o);
    expect(t.kind).toBe('ws');
    expect(o.ws).toHaveBeenCalledWith('https://t.trycloudflare.com', expect.any(Number));
  });

  it('uses a manually entered WebSocket URL when there is no record', async () => {
    const o = openers();
    expect((await openTransport({ wsUrl: 'https://s.example' }, o)).kind).toBe('ws');
    expect(o.rtc).not.toHaveBeenCalled();
  });

  it('names the UDP port when WebRTC fails and there is no fallback', async () => {
    const o = openers({ rtc: vi.fn(async () => { throw new Error('ICE failed'); }) });
    const err = await openTransport({ record }, o).catch((e) => e);
    expect(err).toBeInstanceOf(Unreachable);
    expect(err.message).toBe(describeRtcFailure(record, 'ICE failed'));
    expect(err.message).toContain('UDP port 4443 (ICE failed)');
    expect(err.attempts).toEqual(['rtc: ICE failed']);
  });

  it('keeps the UDP diagnostic when the fallback fails too', async () => {
    const o = openers({
      rtc: vi.fn(async () => { throw new Error('timeout'); }),
      ws: vi.fn(async () => { throw new Error('WebSocket failed'); }),
    });
    const err = await openTransport({ record: { ...record, ws_url: 'https://t.example' } }, o).catch((e) => e);
    expect(err.message).toContain('UDP port 4443 (timeout)');
    expect(err.attempts).toEqual(['rtc: timeout', 'ws: WebSocket failed']);
  });

  it('fails plainly with nothing to try', async () => {
    const err = await openTransport({}, openers()).catch((e) => e);
    expect(err).toBeInstanceOf(Unreachable);
  });
});

describe('openTransport over a candidate bundle (decision 0123)', () => {
  const FP = 'sha-256 ' + Array(32).fill('AB').join(':');
  const bundle = {
    server_id: 'A'.repeat(43), seq: 1, expires: 9e9, published: true, fingerprint: FP, name: 'box',
    candidates: [{ kind: 'lan' as const, addr: '192.168.1.20', port: 4443 }, { kind: 'wan4' as const, addr: '8.8.8.8', port: 50000 }],
  };

  it('races the candidates and returns the first to connect, labelled with its path', async () => {
    const o = openers({ rtc: vi.fn(async (t: { ip: string }) => { if (t.ip === '192.168.1.20') throw new Error('ICE failed'); return fake('rtc'); }) });
    const t = await openTransport({ bundle }, o);
    expect(t.path).toMatchObject({ kind: 'wan4', label: 'Direct, internet' });
  });

  it('wrong_fingerprint_never_connects: every attempt is pinned to the signed fingerprint, whatever address named it', async () => {
    const o = openers({ rtc: vi.fn(async () => { throw new Error('DTLS fingerprint mismatch'); }) });
    const recordFp = 'sha-256 ' + Array(32).fill('CD').join(':');
    const err = await openTransport({ bundle, record: { ...record, ip: '9.9.9.9', fingerprint: recordFp } }, o).catch((e) => e);
    expect(err).toBeInstanceOf(Unreachable);
    const pins = o.rtc.mock.calls.map((c) => `${(c[0] as { ip: string }).ip}=${(c[0] as { fingerprint: string }).fingerprint}`).sort();
    // The bundle's addresses carry the bundle's fingerprint; the record's own address carries the record's.
    expect(pins).toEqual([`192.168.1.20=${FP}`, `8.8.8.8=${FP}`, `9.9.9.9=${recordFp}`].sort());
    expect(err.failures).toHaveLength(3);
  });

  it('without a bundle the old order holds: the record over WebRTC, then the tunnel', async () => {
    const o = openers();
    expect((await openTransport({ record }, o)).path).toBeUndefined();
  });
});
