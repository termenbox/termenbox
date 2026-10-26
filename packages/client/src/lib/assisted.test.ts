import { describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import PathReport from '../PathReport.svelte';
import { AssistedError, buildIceAnswer, candidateAddr, openAssisted, type AssistedAuth, type AssistedDeps, type AssistedTarget } from './assisted';
import { openTransport, Unreachable, type Openers, type Transport } from './transport';
import { allFailedAdvice, buildPaths } from './paths';
import { FINGERPRINT, bundleOf, newServerKey } from './bundleFixture';
import { b64url } from './device';

const URL = 'wss://termenbox-relay-abc.owner.workers.dev/s/6kpsYabcdefghijkl';
const TARGET: AssistedTarget = { url: URL, fingerprint: FINGERPRINT, relay: true };
const TURN = { urls: ['turn:turn.cloudflare.com:3478?transport=udp'], username: 'u', credential: 'c' };
const SECRET_TOKEN = 'turn-secret-not-for-the-page';

const auth: AssistedAuth = { cert: 'CERT', serverId: 'SERVERID', sign: async (_id, msg) => `signed(${msg})` };

interface Sent { t: string; n: string; exp: number; cert: string; sig: string; [k: string]: unknown }

/** A signaling Worker with a server behind it: records what the client sent and answers like the real one. */
function fakeWorker(over: { answer?: () => unknown; relay?: () => unknown; open?: boolean } = {}) {
  const sent: Sent[] = [];
  const ws = {
    onopen: null as null | (() => void), onmessage: null as null | ((e: { data: string }) => void),
    onclose: null as null | (() => void), onerror: null as null | (() => void),
    send(text: string) {
      const m = JSON.parse(text) as Sent;
      sent.push(m);
      const reply = m.t === 'offer' ? (over.answer?.() ?? { t: 'answer', ufrag: 'srvufrag1234567890', pwd: 'srvpwd-0123456789012345678', cands: ['198.51.100.7:4443', '[2001:db8::7]:4443'] })
        : m.t === 'relay' ? (over.relay?.() ?? { t: 'ice', servers: [TURN] }) : { t: 'ok' };
      queueMicrotask(() => ws.onmessage?.({ data: JSON.stringify(reply) }));
    },
    close: vi.fn(),
  };
  const socket = () => {
    if (over.open !== false) queueMicrotask(() => ws.onopen?.());
    else queueMicrotask(() => ws.onerror?.());
    return ws as unknown as WebSocket;
  };
  return { ws, sent, socket };
}

type Behaviour = 'connect' | 'hang' | 'fail';

/** A browser peer connection: gathers one server-reflexive candidate and behaves as `how(config)` says. */
function fakePeers(how: (config: RTCConfiguration, n: number) => Behaviour) {
  const made: { config: RTCConfiguration; remote?: string; closed: boolean }[] = [];
  const peer = (config: RTCConfiguration) => {
    const rec = { config, remote: undefined as string | undefined, closed: false };
    made.push(rec);
    const behaviour = how(config, made.length);
    const channels: { onopen: null | (() => void); readyState: string }[] = [];
    const pc = {
      iceConnectionState: 'new',
      onicecandidate: null as null | ((e: { candidate: unknown }) => void),
      oniceconnectionstatechange: null as null | (() => void),
      createDataChannel: () => { const c = { onopen: null, readyState: 'connecting', onmessage: null, onclose: null, send() {} }; channels.push(c); return c; },
      createOffer: async () => ({ type: 'offer', sdp: 'v=0\r\na=ice-ufrag:abcd\r\na=ice-pwd:efghefghefghefghefghefgh\r\na=mid:0\r\n' }),
      setLocalDescription: async () => {
        queueMicrotask(() => {
          pc.onicecandidate?.({ candidate: { candidate: 'candidate:1 1 udp 1686052607 203.0.113.5 50000 typ srflx', address: '203.0.113.5', port: 50000, protocol: 'udp' } });
          pc.onicecandidate?.({ candidate: { candidate: 'candidate:2 1 udp 2113937151 7f3a.local 50001 typ host', address: '7f3a.local', port: 50001, protocol: 'udp' } });
          pc.onicecandidate?.({ candidate: null });
        });
      },
      setRemoteDescription: async (d: { sdp: string }) => {
        rec.remote = d.sdp;
        setTimeout(() => {
          if (behaviour === 'connect') channels[0].onopen?.();
          if (behaviour === 'fail') { pc.iceConnectionState = 'failed'; pc.oniceconnectionstatechange?.(); }
        }, 1);
      },
      getStats: async () => new Map<string, unknown>([
        ['t', { type: 'transport', selectedCandidatePairId: 'p' }],
        ['p', { type: 'candidate-pair', id: 'p', localCandidateId: 'l', remoteCandidateId: 'r' }],
        ['l', { candidateType: config.iceServers?.some((s) => String(s.urls).includes('turn:')) ? 'relay' : 'srflx' }],
        ['r', { candidateType: 'host' }],
      ]),
      close: () => { rec.closed = true; },
    };
    return pc as unknown as RTCPeerConnection;
  };
  return { made, peer };
}

const fast = { gatherMs: 20, punchMs: 40, relayMs: 60 };
const run = (w: ReturnType<typeof fakeWorker>, peers: ReturnType<typeof fakePeers>, target = TARGET, extra: Partial<AssistedDeps> = {}) =>
  openAssisted(target, auth, undefined, { socket: w.socket, peer: peers.peer, ...fast, ...extra });

describe('the punch wins', () => {
  it('sends one signed offer with the gathered candidates and connects without asking for a relay', async () => {
    const w = fakeWorker();
    const peers = fakePeers(() => 'connect');
    const t = await run(w, peers);
    expect(t.via).toBe('punched');
    expect(w.sent.map((m) => m.t)).toEqual(['offer']);
    const offer = w.sent[0];
    expect(offer.cands).toEqual(['203.0.113.5:50000']); // the mDNS host candidate is left out
    expect(offer.relay).toBe(false);
    expect(offer.cert).toBe('CERT');
    // The signature covers the kind, the nonce, the expiry and the hash of the content, in the server's order.
    const digest = b64url(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(`${offer.ufrag}\n${offer.pwd}\n${offer.relay}\n${(offer.cands as string[]).join(',')}`)));
    expect(offer.sig).toBe(`signed(offer.${offer.n}.${offer.exp}.${digest})`);
    expect(offer.n).toMatch(/^[A-Za-z0-9_-]{16,64}$/);
    expect(offer.exp - Date.now() / 1000).toBeLessThanOrEqual(60);
    expect(offer.ufrag).toMatch(/^[a-z0-9]{16,64}$/);
    expect(String(offer.pwd).length).toBeGreaterThanOrEqual(22);
    // Free STUN only: no TURN credential was in play.
    expect(JSON.stringify(peers.made[0].config)).not.toContain('turn:');
    // The answer is a full ICE answer, pinned to the bundle's fingerprint.
    expect(peers.made[0].remote).toContain(`a=fingerprint:${FINGERPRINT}`);
    expect(peers.made[0].remote).not.toContain('a=ice-lite');
    expect(w.ws.close).toHaveBeenCalled(); // the Worker is not needed once the session is up
  });
});

describe('the punch times out and the relay wins', () => {
  it('asks for credentials, retries with them and reports Relay', async () => {
    const w = fakeWorker();
    const peers = fakePeers((config) => (JSON.stringify(config).includes('turn:') ? 'connect' : 'hang'));
    const t = await run(w, peers);
    expect(t.via).toBe('relay');
    expect(w.sent.map((m) => m.t)).toEqual(['offer', 'relay', 'offer']);
    expect(w.sent[2].relay).toBe(true); // the server allocates its own relay for this session
    expect(JSON.stringify(peers.made[1].config.iceServers)).toContain('turn.cloudflare.com');
    expect(peers.made[0].closed).toBe(true);
  });

  it('a relay stage that settles on a direct pair is still reported as Punched', async () => {
    const w = fakeWorker();
    const peers = fakePeers((_c, n) => (n === 1 ? 'fail' : 'connect'));
    const direct = peers.peer;
    // Same peers, but the stats say the pair is not relayed.
    const t = await run(w, { made: peers.made, peer: (c) => { const pc = direct(c); (pc as unknown as { getStats: () => Promise<Map<string, unknown>> }).getStats = async () => new Map([['t', { type: 'transport', selectedCandidatePairId: 'p' }], ['p', { type: 'candidate-pair', id: 'p', localCandidateId: 'l', remoteCandidateId: 'r' }], ['l', { candidateType: 'srflx' }], ['r', { candidateType: 'host' }]]); return pc; } });
    expect(t.via).toBe('punched');
  });
});

describe('what goes wrong', () => {
  it('signal unreachable: names the signaling stage and its fix', async () => {
    const err = await run(fakeWorker({ open: false }), fakePeers(() => 'connect')).catch((e) => e);
    expect(err).toBeInstanceOf(AssistedError);
    expect(err.stage).toBe('signal');
    expect(err.message).toContain('unreachable');
    expect(err.fix).toContain('--setup-cloudflare');
  });

  it('credential refused: the relay stage never starts and the error says why', async () => {
    const w = fakeWorker({ relay: () => ({ t: 'error', reason: 'relay unavailable' }) });
    const peers = fakePeers(() => 'hang');
    const err = await run(w, peers).catch((e) => e);
    expect(err).toBeInstanceOf(AssistedError);
    expect(err.stage).toBe('credential');
    expect(err.message).toContain('relay unavailable');
    expect(err.fix).toContain('cap');
    expect(peers.made).toHaveLength(1);
    expect(w.sent.map((m) => m.t)).toEqual(['offer', 'relay']);
  });

  it('the server refuses the offer: a signal error, not a hang', async () => {
    const err = await run(fakeWorker({ answer: () => ({ t: 'error', reason: 'device not allowed' }) }), fakePeers(() => 'connect')).catch((e) => e);
    expect(err).toBeInstanceOf(AssistedError);
    expect(err.message).toContain('device not allowed');
  });

  it('a server without a TURN key gets one attempt over the whole window and no relay request', async () => {
    const w = fakeWorker();
    const err = await run(w, fakePeers(() => 'hang'), { ...TARGET, relay: false }, { punchMs: 20, relayMs: 30 }).catch((e) => e);
    expect(err.stage).toBe('punch');
    expect(err.fix).toContain('Tailscale');
    expect(w.sent.map((m) => m.t)).toEqual(['offer']);
  });

  it('no relay credential ever reaches a message the client sends', async () => {
    const w = fakeWorker({ relay: () => ({ t: 'ice', servers: [{ ...TURN, credential: SECRET_TOKEN }] }) });
    await run(w, fakePeers((c) => (JSON.stringify(c).includes('turn:') ? 'connect' : 'hang')));
    expect(JSON.stringify(w.sent)).not.toContain(SECRET_TOKEN);
  });
});

describe('the pieces', () => {
  it('candidateAddr keeps UDP IPs, brackets IPv6 and drops mDNS names and TCP', () => {
    const c = (candidate: string, address: string, port: number, protocol: string) => candidateAddr({ candidate, address, port, protocol } as RTCIceCandidate);
    expect(c('candidate:1 1 udp 1 203.0.113.5 50000 typ srflx', '203.0.113.5', 50000, 'udp')).toBe('203.0.113.5:50000');
    expect(c('candidate:1 1 udp 1 2001:db8::1 50000 typ host', '2001:db8::1', 50000, 'udp')).toBe('[2001:db8::1]:50000');
    expect(c('candidate:1 1 udp 1 abc.local 50000 typ host', 'abc.local', 50000, 'udp')).toBeNull();
    expect(c('candidate:1 1 tcp 1 203.0.113.5 9 typ host', '203.0.113.5', 9, 'tcp')).toBeNull();
  });

  it('buildIceAnswer is a full-ICE passive answer with the server\'s candidates', () => {
    const lines = buildIceAnswer(FINGERPRINT, '0', 'uf', 'pw', ['198.51.100.7:4443', '[2001:db8::7]:4443']).split('\r\n');
    expect(lines).toContain('a=setup:passive');
    expect(lines).toContain('a=ice-ufrag:uf');
    expect(lines).toContain('c=IN IP4 198.51.100.7');
    expect(lines.filter((l) => l.startsWith('a=candidate:'))).toHaveLength(2);
    expect(lines.join('\n')).toContain('2001:db8::7 4443 typ host');
    expect(lines).not.toContain('a=ice-lite');
  });
});

describe('in the race and in the report', () => {
  const server = newServerKey();
  const bundleWith = async (over = {}) => {
    const { serverId, sign } = await server;
    const { verifyBundle } = await import('./bundle');
    const wire = await sign(bundleOf(1, [{ kind: 'lan', addr: '192.168.1.20', port: 4443 }], { signal: URL, relay: true, ...over }));
    return verifyBundle(wire, { nowSecs: Math.floor(Date.now() / 1000), pin: serverId });
  };
  const tcp = (): Transport => ({ kind: 'rtc', onmessage: null, onclose: null, send() {}, close() {} });
  const lanFails = () => Promise.reject(new Error('ICE failed'));

  it('the winner is named Punched or Relay in the session header', async () => {
    const bundle = await bundleWith();
    for (const via of ['punched', 'relay'] as const) {
      const openers: Openers = { rtc: lanFails, ws: lanFails, assisted: async () => Object.assign(tcp(), { via }) };
      const t = await openTransport({ bundle }, openers, auth);
      expect(t.path?.label).toBe(via === 'punched' ? 'Punched' : 'Relay');
      expect(t.path?.kind).toBe('signal');
    }
  });

  it('the signaling path is not tried without a device identity', async () => {
    const bundle = await bundleWith();
    const assisted = vi.fn();
    const err = await openTransport({ bundle }, { rtc: lanFails, ws: lanFails, assisted }).catch((e) => e);
    expect(err).toBeInstanceOf(Unreachable);
    expect(assisted).not.toHaveBeenCalled();
  });

  it('each failed outcome reaches the report with its own fix', async () => {
    const bundle = await bundleWith();
    const outcomes: [AssistedError, RegExp][] = [
      [new AssistedError('signal', 'the signaling server is unreachable'), /signaling Worker still exists/],
      [new AssistedError('credential', 'relay unavailable'), /monthly relay cap/],
      [new AssistedError('punch', 'no data channel after 5000 ms'), /Both networks block a direct path/],
      [new AssistedError('relay', 'ICE failed'), /turn\.cloudflare\.com/],
    ];
    for (const [e, fix] of outcomes) {
      const err: Unreachable = await openTransport({ bundle }, { rtc: lanFails, ws: lanFails, assisted: () => Promise.reject(e) }, auth).catch((x) => x);
      const f = err.failures.find((x) => x.path.kind === 'signal')!;
      expect(f.reason).toBe(e.message);
      expect(f.fix).toMatch(fix);
      const html = (render as unknown as (c: unknown, o: { props: unknown }) => { body: string })(PathReport, { props: { failures: err.failures } }).body;
      expect(html).toContain(e.message);
      expect(html).toMatch(fix);
    }
  });

  it('a LAN-only server names the WSL cause and --setup-cloudflare; one with a Worker does not', async () => {
    const lanOnly = buildPaths({ bundle: await bundleWith({ signal: undefined, relay: false }) });
    expect(allFailedAdvice(lanOnly)).toMatch(/WSL2.*mirrored.*--setup-cloudflare/);
    expect(allFailedAdvice(buildPaths({ bundle: await bundleWith() }))).toBe('');
  });

  it('the Worker is the same wss: origin family as a tunnel, so the hosted CSP needs nothing new', async () => {
    const { readFileSync } = await import('node:fs');
    const csp = readFileSync('public/_headers', 'utf8');
    expect(csp).toContain("connect-src 'self' wss:");
    expect(new globalThis.URL(URL).protocol).toBe('wss:');
  });
});
