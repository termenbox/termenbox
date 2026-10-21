import { describe, it, expect, vi, afterEach } from 'vitest';
import { allFailedAdvice, buildPaths, fixFor, orderPaths, racePaths, storedBundle, Unreachable, type Path } from './paths';
import { bundleOf, newServerKey, FINGERPRINT } from './bundleFixture';
import type { Bundle } from './protocol';

const bundle: Bundle = {
  server_id: 'S'.repeat(43), seq: 1, expires: 9e9, published: false, fingerprint: FINGERPRINT, name: 'box',
  candidates: [
    { kind: 'wan4', addr: '8.8.8.8', port: 50000 },
    { kind: 'lan', addr: '192.168.1.20', port: 4443 },
    { kind: 'overlay', addr: '100.64.1.2', port: 4443 },
    { kind: 'wan6', addr: '2001:4860::9', port: 4443 },
    { kind: 'tunnel', addr: 'wss://t.example/ws', port: 443 },
  ],
};

const closable = (name: string) => ({ name, closed: false, close() { this.closed = true; } });

afterEach(() => vi.useRealTimers());

describe('paths', () => {
  it('builds one path per candidate, adds the directory record and a tunnel URL, and never twice', () => {
    const record = { ip: '8.8.8.8', udp_port: 50000, fingerprint: 'sha-256 ' + Array(32).fill('CD').join(':'), ws_url: 'https://ws.example' };
    const paths = buildPaths({ bundle, record, wsUrl: 'https://ignored.example' });
    expect(paths.map((p) => p.key)).toEqual([
      'wan4 8.8.8.8:50000', 'lan 192.168.1.20:4443', 'overlay 100.64.1.2:4443', 'wan6 [2001:4860::9]:4443', 'tunnel wss://t.example/ws', 'ws https://ws.example',
    ]);
    // Each direct path is pinned to the fingerprint of the source that named it (the bundle, signed by the server key).
    expect(paths.filter((p) => p.kind !== 'ws' && p.kind !== 'tunnel').every((p) => p.fingerprint === FINGERPRINT)).toBe(true);
  });

  it('starts the remembered winner first, then LAN, IPv6, IPv4, overlay, and tunnels last', () => {
    const paths = buildPaths({ bundle });
    expect(orderPaths(paths).map((p) => p.kind)).toEqual(['lan', 'wan6', 'wan4', 'overlay', 'tunnel']);
    expect(orderPaths(paths, 'wan4 8.8.8.8:50000').map((p) => p.kind)).toEqual(['wan4', 'lan', 'wan6', 'overlay', 'tunnel']);
  });

  it('names a fix for each kind of path', () => {
    for (const p of buildPaths({ bundle })) expect(fixFor(p).length).toBeGreaterThan(10);
    expect(fixFor(buildPaths({ bundle })[0])).toContain('50000');
  });
});

describe('the path race', () => {
  const direct = buildPaths({ bundle: { ...bundle, candidates: bundle.candidates.slice(0, 2) } }); // wan4, lan
  const fast = () => 0;

  it('race_first_wins: the first path to connect wins and the others are cancelled', async () => {
    vi.useFakeTimers();
    const opened: string[] = [];
    const signals: AbortSignal[] = [];
    const open = vi.fn((p: Path, signal: AbortSignal) => {
      opened.push(p.key);
      signals.push(signal);
      // The first path (started first) connects at 100 ms; the second never does.
      return new Promise<ReturnType<typeof closable>>((resolve, reject) => {
        if (opened.length === 1) setTimeout(() => resolve(closable(p.key)), 100);
        else signal.addEventListener('abort', () => reject(new Error('cancelled')));
      });
    });
    const race = racePaths(direct, open, { stagger: () => 50 });
    await vi.advanceTimersByTimeAsync(200);
    const { value, path } = await race;
    expect(path.key).toBe('wan4 8.8.8.8:50000');
    expect(value.name).toBe('wan4 8.8.8.8:50000');
    expect(opened).toHaveLength(2); // the second was started after the stagger...
    expect(signals[1].aborted).toBe(true); // ...and cancelled when the first won
  });

  it('race_first_fails_second_wins: a failed path starts the next at once and its failure is reported', async () => {
    const open = vi.fn(async (p: Path) => {
      if (p.kind === 'wan4') throw new Error('ICE failed');
      return closable(p.key);
    });
    const { path } = await racePaths(direct, open, { stagger: () => 10_000 }); // would wait 10 s if it did not start at once
    expect(path.kind).toBe('lan');
    expect(open).toHaveBeenCalledTimes(2);
  });

  it('race_all_fail: every path is listed with why it failed and one fix', async () => {
    const open = vi.fn(async (p: Path): Promise<ReturnType<typeof closable>> => { throw new Error(`no answer from ${p.addr}`); });
    const err = await racePaths(direct, open, { stagger: fast }).catch((e) => e);
    expect(err).toBeInstanceOf(Unreachable);
    expect(err.failures.map((f: { path: Path }) => f.path.key).sort()).toEqual(['lan 192.168.1.20:4443', 'wan4 8.8.8.8:50000']);
    for (const f of err.failures) {
      expect(f.reason).toContain('no answer from');
      expect(f.fix.length).toBeGreaterThan(10);
    }
    expect(err.attempts).toHaveLength(2);
    expect(err.message).toContain('2 paths');
  });

  it('a path that connects after the race was decided is closed, not leaked', async () => {
    vi.useFakeTimers();
    const late = closable('late');
    const open = (p: Path) => new Promise<ReturnType<typeof closable>>((resolve) => setTimeout(() => resolve(p.kind === 'wan4' ? closable('first') : late), p.kind === 'wan4' ? 10 : 100));
    const race = racePaths(direct, open, { stagger: () => 1 });
    await vi.advanceTimersByTimeAsync(300);
    expect((await race).value.name).toBe('first');
    expect(late.closed).toBe(true);
  });

  it('tunnels are tried only after every direct path failed', async () => {
    const paths = buildPaths({ bundle });
    const order: string[] = [];
    const open = async (p: Path) => {
      order.push(p.kind);
      if (p.kind !== 'tunnel') throw new Error('x');
      return closable(p.key);
    };
    const { path } = await racePaths(orderPaths(paths), open, { stagger: fast });
    expect(path.kind).toBe('tunnel');
    expect(order.at(-1)).toBe('tunnel');
    expect(order.filter((k) => k !== 'tunnel')).toHaveLength(4);
  });

  it('a tunnel is not started while a direct path is still connecting', async () => {
    vi.useFakeTimers();
    const started: string[] = [];
    const open = (p: Path) => new Promise<ReturnType<typeof closable>>((resolve) => { started.push(p.kind); if (p.kind === 'lan') setTimeout(() => resolve(closable('lan')), 5000); });
    const race = racePaths(orderPaths(buildPaths({ bundle })), open, { stagger: () => 100 });
    await vi.advanceTimersByTimeAsync(10_000);
    expect((await race).path.kind).toBe('lan');
    expect(started).not.toContain('tunnel');
  });
});

describe('when everything failed', () => {
  it('a server that advertises only a LAN address says it is probably behind CGNAT and points to an overlay', () => {
    const lanOnly = buildPaths({ bundle: { ...bundle, candidates: [{ kind: 'lan', addr: '192.168.1.20', port: 4443 }] } });
    const advice = allFailedAdvice(lanOnly);
    expect(advice).toContain('CGNAT');
    expect(advice).toContain('Tailscale');
    expect(allFailedAdvice(buildPaths({ bundle }))).toBe('');
  });
});

describe('the stored bundle is a hint that is checked again', () => {
  it('is used when its signature and pin hold, even after it expired, and dropped otherwise', async () => {
    const key = await newServerKey();
    const lan = [{ kind: 'lan' as const, addr: '10.0.0.2', port: 4443 }];
    const old = await key.sign(bundleOf(3, lan, { expires: 1000 }));
    expect((await storedBundle({ bundle: old, server_id: key.serverId }))?.candidates).toEqual(lan);
    expect(await storedBundle({ bundle: old, server_id: 'A'.repeat(43) })).toBeNull();
    expect(await storedBundle({ bundle: old.slice(0, -2) + 'AA', server_id: key.serverId })).toBeNull();
    expect(await storedBundle({ server_id: key.serverId })).toBeNull();
  });
});
