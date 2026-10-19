import { describe, it, expect, vi } from 'vitest';
import { installCommand, scanCommand, claimLink, parseClaim, parseServerLink, serverCode, serverLink, pollDelay, waitForServer, previewClaim, checkReachable, firewallCommand, ClaimTimeout, CLAIM_WINDOW_MS, type WaitDeps } from './onboard';
import type { DirRecord } from './directory';
import { Unreachable, type Transport } from './transport';

const S = 'A'.repeat(43);
const rec = { server_id: 'K'.repeat(43) } as DirRecord;

describe('claim text', () => {
  it('builds one command that carries the secret and the origin', () => {
    expect(installCommand('https://app.example.dev', S)).toBe(`curl -fsSL https://app.example.dev/install.sh | sh -s -- ${S} https://app.example.dev`);
  });

  it('round-trips through a link the phone camera can open', () => {
    const link = claimLink('https://app.example.dev/', S);
    expect(link).toBe(`https://app.example.dev/#claim=${S}`);
    expect(parseClaim(link)).toBe(S);
    expect(parseClaim(`#claim=${S}`)).toBe(S);
    expect(parseClaim(`  ${S}\n`)).toBe(S);
  });

  it('rejects anything that is not a 43-character secret', () => {
    for (const bad of ['', 'short', `#claim=${'A'.repeat(42)}`, `#claim=${'A'.repeat(44)}`, 'https://evil.example/#add=xyz', `x ${'!'.repeat(43)}`]) {
      expect(parseClaim(bad), bad).toBeNull();
    }
  });
});

describe('waitForServer', () => {
  const deps = (seq: Array<DirRecord | null | Error>): WaitDeps & { slept: number[] } => {
    let t = 0;
    const slept: number[] = [];
    return {
      slept,
      pending: vi.fn(async () => {
        const next = seq.shift();
        if (next instanceof Error) throw next;
        return next ?? null;
      }),
      sleep: async (ms) => { slept.push(ms); t += ms; },
      now: () => t,
    };
  };

  it('backs off 2s, 4s, 8s, then 10s and returns the record once it appears', async () => {
    const d = deps([null, null, null, null, null, rec]);
    expect(await waitForServer('o', S, new AbortController().signal, d)).toBe(rec);
    expect(d.slept).toEqual([2000, 4000, 8000, 10000, 10000]);
    expect(pollDelay(0)).toBe(2000);
    expect(pollDelay(99)).toBe(10000);
  });

  it('keeps waiting through directory hiccups', async () => {
    const d = deps([new Error('directory answered 500'), new TypeError('fetch failed'), rec]);
    expect(await waitForServer('o', S, new AbortController().signal, d)).toBe(rec);
  });

  it('stops at once when a record fails verification', async () => {
    const d = deps([new Error('announcement does not prove knowledge of the claim secret'), rec]);
    await expect(waitForServer('o', S, new AbortController().signal, d)).rejects.toThrow('claim secret');
    expect(d.slept).toEqual([]);
  });

  it('gives up after the 15-minute claim window', async () => {
    const d = deps([]);
    await expect(waitForServer('o', S, new AbortController().signal, d)).rejects.toBeInstanceOf(ClaimTimeout);
    expect(d.slept.reduce((a, b) => a + b, 0)).toBeGreaterThanOrEqual(CLAIM_WINDOW_MS);
    expect(d.slept.length).toBeLessThan(100);
  });

  it('can be cancelled', async () => {
    const ac = new AbortController();
    ac.abort();
    await expect(waitForServer('o', S, ac.signal, deps([rec]))).rejects.toThrow('aborted');
  });
});

describe('parseServerLink', () => {
  const id = 'A'.repeat(43);
  it('reads the id from a link or a fragment', () => {
    expect(parseServerLink(`https://app.example.dev/#server=${id}`)).toBe(id);
    expect(parseServerLink(`#server=${id}`)).toBe(id);
  });
  it('reads the id from the code a share carries, alone or inside a pasted message', () => {
    expect(serverCode(id)).toBe(`termenbox-server:${id}`);
    expect(parseServerLink(serverCode(id))).toBe(id);
    expect(parseServerLink(`Link not opening the app? In the app choose Add Server and paste: ${serverCode(id)}`)).toBe(id);
    expect(parseServerLink(`https://app.example.dev/#server=${id}\n${serverCode(id)}`)).toBe(id);
    expect(parseClaim(serverCode(id))).toBeNull(); // a server id is not a claim secret
  });
  it('rejects claim links, short ids and junk', () => {
    expect(parseServerLink(`termenbox-server:${'A'.repeat(42)}`)).toBeNull();
    expect(parseServerLink(`xtermenbox-server:${id}`)).toBeNull();
    expect(parseServerLink(id)).toBeNull();
    expect(parseServerLink(`https://app.example.dev/#claim=${id}`)).toBeNull();
    expect(parseServerLink('#server=short')).toBeNull();
    expect(parseServerLink('hello')).toBeNull();
  });
});

describe('run choice in the install commands', () => {
  const O = 'https://app.example.dev';
  it('adds the mode word only for the host modes', () => {
    expect(installCommand(O, S)).toBe(`curl -fsSL ${O}/install.sh | sh -s -- ${S} ${O}`);
    expect(installCommand(O, S, 'container')).toBe(installCommand(O, S));
    expect(installCommand(O, S, 'host-me')).toBe(`curl -fsSL ${O}/install.sh | sh -s -- ${S} ${O} host-me`);
    expect(installCommand(O, S, 'host-user')).toBe(`curl -fsSL ${O}/install.sh | sh -s -- ${S} ${O} host-user`);
    expect(installCommand(O, S, 'host')).toBe(`curl -fsSL ${O}/install.sh | sh -s -- ${S} ${O} host`);
  });

  it('keeps the QR command short for the container and passes - for the host modes', () => {
    expect(scanCommand(O)).toBe(`curl -fsSL ${O}/install.sh | sh`);
    expect(scanCommand(O, 'host-user')).toBe(`curl -fsSL ${O}/install.sh | sh -s -- - ${O} host-user`);
    expect(scanCommand(O, 'host')).toBe(`curl -fsSL ${O}/install.sh | sh -s -- - ${O} host`);
  });
});

describe('server link', () => {
  it('round-trips the public id and never carries a secret', () => {
    const id = 'K'.repeat(43);
    expect(serverLink('https://app.example.dev/', id)).toBe(`https://app.example.dev/#server=${id}`);
    expect(parseServerLink(serverLink('https://app.example.dev', id))).toBe(id);
  });
});

describe('previewClaim (decision 0117)', () => {
  /** A server that answers `claim_preview` for `good` only, and counts what it was sent. */
  const server = (good: string) => {
    const sent: any[] = [];
    let opened = 0;
    let closed = 0;
    const open = async (): Promise<Transport> => {
      opened++;
      const t: Transport = {
        kind: 'ws', onmessage: null, onclose: null,
        send(text) {
          const m = JSON.parse(text);
          sent.push(m);
          const reply = (r: unknown) => queueMicrotask(() => t.onmessage?.(JSON.stringify(r)));
          if (m.type === 'hello') reply({ type: 'challenge', nonce: 'n', server_id: rec.server_id });
          else if (m.type === 'claim_preview') {
            reply(m.server_token === good
              ? { type: 'claim_preview', name: 'vps-1', mode: 'host', run_user: 'alice', version: '0.1.0+abc' }
              : { type: 'error', message: 'This claim link is not valid (any more)' });
          }
        },
        close() { closed++; },
      };
      return t;
    };
    return { sent, open, opened: () => opened, closed: () => closed };
  };
  const live = () => new AbortController().signal;

  it('previews twice without ever presenting a device certificate or a claim', async () => {
    const s = server(S);
    const a = await previewClaim({ record: rec }, S, live(), { open: s.open });
    const b = await previewClaim({ record: rec }, S, live(), { open: s.open });
    expect(a).toEqual({ name: 'vps-1', mode: 'host', run_user: 'alice', version: '0.1.0+abc' });
    expect(b).toEqual(a);
    expect(s.sent.map((m) => m.type)).toEqual(['hello', 'claim_preview', 'hello', 'claim_preview']);
    expect(s.sent.some((m) => m.type === 'auth')).toBe(false);
    expect(s.closed()).toBe(2);
  });

  it('gets no preview for a wrong or expired token', async () => {
    const s = server(S);
    await expect(previewClaim({ record: rec }, 'B'.repeat(43), live(), { open: s.open })).rejects.toThrow(/not valid/);
    expect(s.closed()).toBe(1);
  });

  it('refuses a server whose challenge names another server id', async () => {
    const s = server(S);
    const other = { server_id: 'Z'.repeat(43) } as DirRecord;
    await expect(previewClaim({ record: other }, S, live(), { open: s.open })).rejects.toThrow(/not the one/);
    expect(s.sent.map((m) => m.type)).toEqual(['hello']);
  });

  it('sends nothing and closes when the dialog is closed first', async () => {
    const s = server(S);
    const ctl = new AbortController();
    ctl.abort();
    await expect(previewClaim({ record: rec }, S, ctl.signal, { open: s.open })).rejects.toMatchObject({ name: 'AbortError' });
    expect(s.opened()).toBe(0);

    const late = new AbortController();
    const p = previewClaim({ record: rec }, S, late.signal, { open: s.open });
    late.abort();
    await expect(p).rejects.toMatchObject({ name: 'AbortError' });
    expect(s.sent.some((m) => m.type === 'auth')).toBe(false);
    expect(s.closed()).toBeGreaterThanOrEqual(1);
  });

  it('gives up when the server never answers', async () => {
    vi.useFakeTimers();
    try {
      const hang = async (): Promise<Transport> => ({ kind: 'ws', onmessage: null, onclose: null, send() {}, close() {} });
      const p = previewClaim({ record: rec }, S, live(), { open: hang }, 1000);
      const check = expect(p).rejects.toThrow(/did not answer/);
      await vi.advanceTimersByTimeAsync(1001);
      await check;
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('checkReachable (decision 0118)', () => {
  const withPort = { server_id: 'K'.repeat(43), udp_port: 4450 } as DirRecord;
  it('is reachable when a transport opens, and closes it without authenticating', async () => {
    const close = vi.fn();
    const send = vi.fn();
    const r = await checkReachable({ record: withPort }, { open: async () => ({ close, send } as unknown as Transport) });
    expect(r).toEqual({ state: 'reachable' });
    expect(close).toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
  });

  it('is unreachable with the bound port when the transport throws Unreachable', async () => {
    const r = await checkReachable({ record: withPort }, { open: async () => { throw new Unreachable('Server unreachable on UDP port 4450.', []); } });
    expect(r).toEqual({ state: 'unreachable', port: 4450, message: 'Server unreachable on UDP port 4450.' });
  });

  it('is unknown for any other failure, or when there is nothing to connect to', async () => {
    expect(await checkReachable({ record: withPort }, { open: async () => { throw new Error('boom'); } })).toEqual({ state: 'unknown' });
    expect(await checkReachable({}, { open: async () => { throw new Error('unused'); } })).toEqual({ state: 'unknown' });
  });

  it('names the range command the admin runs', () => {
    expect(firewallCommand).toBe('ufw allow 4443:4542/udp');
  });
});
