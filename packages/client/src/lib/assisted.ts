// The assisted path (DECISION-0141): a server that no client can reach on a UDP port is reached by punching a hole
// through both NATs, and by a TURN relay when the punch does not connect. The browser does ICE and TURN itself;
// this file carries the offer and the candidates to the server over the owner's signaling Worker (`wss:` only, so the
// hosted CSP needs no new origin) and builds the session from the answer. The Worker is untrusted: every message is
// signed with the device key and names its kind, freshness and content (packages/server/src/signal.rs).
import { b64url, type Credentials } from './device';
import { CHANNEL_ID, CONTROL_CHANNEL_ID, randomUfrag, RtcTransport, setIceCredentials, TERM_CHANNEL_ID } from './rtc';
import type { Transport } from './transport';

/** Free public STUN, the same defaults as the server (DECISION-0141). */
export const STUN_SERVERS: RTCIceServer[] = [{ urls: 'stun:stun.cloudflare.com:3478' }, { urls: 'stun:stun.l.google.com:19302' }];
/**
 * The STUN servers to ask: the public defaults, or the comma list in `localStorage.termenbox_stun` (the counterpart of the
 * server's `TERMENBOX_STUN`, for a network that blocks public STUN, and for the end-to-end test). An empty value means none.
 */
export function stunServers(): RTCIceServer[] {
  try {
    const v = localStorage.getItem('termenbox_stun');
    if (v !== null) return v.split(',').map((h) => h.trim()).filter((h) => /^[A-Za-z0-9.:[\]-]{1,100}$/.test(h)).map((h) => ({ urls: `stun:${h}` }));
  } catch { /* no storage: the defaults */ }
  return STUN_SERVERS;
}
/** How long the punch gets before the relay is asked for, and how long each stage waits for the data channel. */
export const PUNCH_WINDOW_MS = 5000;
export const RELAY_WINDOW_MS = 10000;
/** Candidates are collected this long (or until gathering completes) before the offer goes out; later ones trickle. */
export const GATHER_MS = 1500;
const MESSAGE_LIFETIME_S = 30;
const REPLY_TIMEOUT_MS = 8000;

export type AssistedStage = 'signal' | 'credential' | 'punch' | 'relay';

/** A failure of the assisted path: the report shows `message` and the fix for the stage that failed. */
export class AssistedError extends Error {
  readonly fix: string;
  constructor(readonly stage: AssistedStage, message: string) {
    super(message);
    this.fix = FIXES[stage];
  }
}

const FIXES: Record<AssistedStage, string> = {
  signal: "Check that the server is running and that its signaling Worker still exists; run `termenbox-server --setup-cloudflare` again on the server if it was deleted.",
  credential: 'The server refused or could not mint relay credentials: the monthly relay cap may be reached, or the TURN key was revoked. Run `termenbox-server --setup-cloudflare` again to replace the key.',
  punch: 'Both networks block a direct path. Without a relay, join the same Wi-Fi as the server or use an overlay such as Tailscale; with `termenbox-server --setup-cloudflare` a relay is the fallback.',
  relay: 'The relay did not connect either. Check that this network allows UDP or TCP to Cloudflare (turn.cloudflare.com), and that the server is online.',
};

/** What the server's signaling Worker offers: the route, the pinned fingerprint and whether a relay can be asked for. */
export interface AssistedTarget {
  /** `wss://<worker>/s/<route>` from the signed bundle. */
  url: string;
  fingerprint: string;
  relay: boolean;
}

export interface AssistedAuth extends Credentials {
  serverId: string;
  claim?: string;
}

/** The parts a test replaces. */
export interface AssistedDeps {
  socket?: (url: string) => WebSocket;
  peer?: (config: RTCConfiguration) => RTCPeerConnection;
  gatherMs?: number;
  punchMs?: number;
  relayMs?: number;
  now?: () => number;
}

export type Via = 'punched' | 'relay';
export const VIA_LABELS: Record<Via, string> = { punched: 'Punched', relay: 'Relay' };

interface Reply {
  t: string;
  [k: string]: unknown;
}

/** The Worker socket: signed requests out, replies in. */
class Signaling {
  private waiters: { want: string[]; resolve: (r: Reply) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }[] = [];

  private constructor(private ws: WebSocket, private auth: AssistedAuth, private now: () => number) {
    // The Durable Object forwards the server's frames as they came, and the server sends binary ones: read both kinds.
    ws.binaryType = 'arraybuffer';
    ws.onmessage = (e) => {
      const text = typeof e.data === 'string' ? e.data : e.data instanceof ArrayBuffer ? new TextDecoder().decode(e.data) : '';
      let r: Reply;
      try { r = JSON.parse(text); } catch { return; }
      if (!r || typeof r.t !== 'string') return;
      const i = this.waiters.findIndex((w) => r.t === 'error' || w.want.includes(r.t));
      if (i < 0) return;
      const [w] = this.waiters.splice(i, 1);
      clearTimeout(w.timer);
      w.resolve(r);
    };
    ws.onclose = () => this.failAll(new AssistedError('signal', 'the signaling connection closed'));
    ws.onerror = () => this.failAll(new AssistedError('signal', 'the signaling connection failed'));
  }

  static open(url: string, auth: AssistedAuth, deps: AssistedDeps, timeoutMs: number, signal?: AbortSignal): Promise<Signaling> {
    return new Promise((resolve, reject) => {
      let ws: WebSocket;
      try { ws = (deps.socket ?? ((u) => new WebSocket(u)))(url); } catch (e) {
        return reject(new AssistedError('signal', `cannot open the signaling connection: ${e instanceof Error ? e.message : e}`));
      }
      const fail = (why: string) => { clearTimeout(timer); try { ws.close(); } catch { /* already closed */ } reject(new AssistedError('signal', why)); };
      const timer = setTimeout(() => fail(`the signaling server did not answer within ${timeoutMs} ms`), timeoutMs);
      signal?.addEventListener('abort', () => fail('cancelled'), { once: true });
      ws.onopen = () => { clearTimeout(timer); resolve(new Signaling(ws, auth, deps.now ?? Date.now)); };
      ws.onerror = () => fail('the signaling server is unreachable');
      ws.onclose = () => fail('the signaling server closed the connection');
    });
  }

  private failAll(e: Error) {
    for (const w of this.waiters.splice(0)) { clearTimeout(w.timer); w.reject(e); }
  }

  /** Signs and sends one message; `body` is the exact text the server hashes for this kind (see `Msg::body`). */
  async send(t: 'offer' | 'cand' | 'relay', fields: Record<string, unknown>, body: string) {
    const n = b64url(crypto.getRandomValues(new Uint8Array(18)));
    const exp = Math.floor(this.now() / 1000) + MESSAGE_LIFETIME_S;
    const digest = b64url(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(body)));
    const sig = await this.auth.sign(this.auth.serverId, `${t}.${n}.${exp}.${digest}`);
    this.ws.send(JSON.stringify({ t, n, exp, cert: this.auth.cert, sig, ...(this.auth.claim ? { claim: this.auth.claim } : {}), ...fields }));
  }

  /** The next reply of one of these kinds; an `error` reply rejects with its reason. */
  expect(want: string[], timeoutMs = REPLY_TIMEOUT_MS): Promise<Reply> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiters = this.waiters.filter((w) => w.timer !== timer);
        reject(new AssistedError('signal', 'the server did not answer through the signaling Worker'));
      }, timeoutMs);
      this.waiters.push({ want, resolve: (r) => (r.t === 'error' ? reject(new Error(String(r.reason ?? 'refused'))) : resolve(r)), reject, timer });
    });
  }

  close() {
    this.ws.onmessage = this.ws.onclose = this.ws.onerror = null;
    try { this.ws.close(); } catch { /* already closed */ }
    this.failAll(new AssistedError('signal', 'cancelled'));
  }
}

/** `ip:port` (IPv6 as `[ip]:port`) of a gathered candidate the server can use; null for mDNS names and TCP. */
export function candidateAddr(c: Pick<RTCIceCandidate, 'candidate' | 'address' | 'port' | 'protocol'>): string | null {
  const m = /candidate:\S+ \d+ (\S+) \d+ (\S+) (\d+) typ/.exec(c.candidate);
  const proto = (c.protocol ?? m?.[1] ?? '').toLowerCase();
  const address = (c.address ?? m?.[2])?.replace(/^\[|\]$/g, ''); // Chromium may hand an IPv6 address over bracketed
  const port = c.port ?? (m ? Number(m[3]) : undefined);
  if (proto !== 'udp' || !address || !port || address.endsWith('.local')) return null;
  return address.includes(':') ? `[${address}]:${port}` : `${address}:${port}`;
}

/** The server's side of the session as an SDP answer for a full ICE agent (the direct path's answer is ICE-lite). */
export function buildIceAnswer(fingerprint: string, mid: string, ufrag: string, pwd: string, cands: string[]): string {
  const parsed = cands.map((c) => {
    const i = c.lastIndexOf(':');
    return { ip: c.slice(0, i).replace(/^\[|\]$/g, ''), port: Number(c.slice(i + 1)) };
  });
  const first = parsed[0] ?? { ip: '0.0.0.0', port: 9 };
  const v6 = first.ip.includes(':');
  return [
    'v=0',
    `o=- 0 0 IN ${v6 ? 'IP6' : 'IP4'} ${first.ip}`,
    's=-',
    't=0 0',
    `a=group:BUNDLE ${mid}`,
    `m=application ${first.port} UDP/DTLS/SCTP webrtc-datachannel`,
    `c=IN ${v6 ? 'IP6' : 'IP4'} ${first.ip}`,
    `a=mid:${mid}`,
    `a=ice-ufrag:${ufrag}`,
    `a=ice-pwd:${pwd}`,
    `a=fingerprint:${fingerprint}`,
    'a=setup:passive',
    'a=sctp-port:5000',
    'a=max-message-size:65536',
    ...parsed.map((p, i) => `a=candidate:${i + 1} 1 UDP ${2113929471 - i * 256} ${p.ip} ${p.port} typ host`),
    'a=end-of-candidates',
    '',
  ].join('\r\n');
}

interface Stage {
  iceServers: RTCIceServer[];
  /** Ask the server to allocate its own relay for this session. */
  relay: boolean;
  windowMs: number;
  failure: AssistedStage;
}

/** One attempt: a fresh peer connection, the signed offer, the answer, then a wait for the data channel. */
async function attempt(sig: Signaling, target: AssistedTarget, stage: Stage, deps: AssistedDeps, signal?: AbortSignal): Promise<{ transport: Transport; pc: RTCPeerConnection }> {
  const pc = (deps.peer ?? ((c) => new RTCPeerConnection(c)))({ iceServers: stage.iceServers });
  const dc = pc.createDataChannel('t', { negotiated: true, id: CHANNEL_ID });
  const term = pc.createDataChannel('term', { negotiated: true, id: TERM_CHANNEL_ID });
  const ctl = pc.createDataChannel('ctl', { negotiated: true, id: CONTROL_CHANNEL_ID });
  try {
    const gathered: string[] = [];
    let session = '';
    const later: string[] = [];
    let flush: () => void = () => {};
    const complete = new Promise<void>((resolve) => {
      pc.onicecandidate = (e) => {
        if (!e.candidate) return resolve();
        const a = candidateAddr(e.candidate);
        if (!a) return;
        if (session) later.push(a); else gathered.push(a);
        flush();
      };
    });
    const offer = await pc.createOffer();
    const ufrag = randomUfrag();
    const sdp = setIceCredentials(offer.sdp!, ufrag);
    await pc.setLocalDescription({ type: 'offer', sdp });
    const mid = /a=mid:(\S+)/.exec(sdp)?.[1] ?? '0';
    await Promise.race([complete, new Promise((r) => setTimeout(r, deps.gatherMs ?? GATHER_MS))]);
    signal?.throwIfAborted();

    const cands = gathered.slice(0, 16);
    let answer: Reply;
    try {
      // Listen before sending: the reply must not be able to beat the listener.
      const reply = sig.expect(['answer']);
      reply.catch(() => {});
      await sig.send('offer', { ufrag, pwd: ufrag, relay: stage.relay, cands }, `${ufrag}\n${ufrag}\n${stage.relay}\n${cands.join(',')}`);
      answer = await reply;
    } catch (e) {
      if (e instanceof AssistedError) throw e;
      throw new AssistedError('signal', `the server refused the offer: ${e instanceof Error ? e.message : e}`);
    }
    session = String(answer.ufrag);
    const serverCands = Array.isArray(answer.cands) ? answer.cands.map(String) : [];
    await pc.setRemoteDescription({ type: 'answer', sdp: buildIceAnswer(target.fingerprint, mid, session, String(answer.pwd), serverCands) });

    // Candidates found after the offer went out, one signed message each.
    flush = () => {
      for (const a of later.splice(0)) void sig.send('cand', { session, addr: a }, `${session}\n${a}`).catch(() => {});
    };
    flush();

    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new AssistedError(stage.failure, `no data channel after ${stage.windowMs} ms`)), stage.windowMs);
      signal?.addEventListener('abort', () => { clearTimeout(timer); reject(new Error('cancelled')); }, { once: true });
      dc.onopen = () => { clearTimeout(timer); resolve(); };
      pc.oniceconnectionstatechange = () => {
        if (pc.iceConnectionState === 'failed') { clearTimeout(timer); reject(new AssistedError(stage.failure, 'ICE failed')); }
      };
    });
    return { transport: new RtcTransport(pc, dc, term, ctl), pc };
  } catch (e) {
    pc.close();
    throw e;
  }
}

/** True when the connection the browser settled on goes through a TURN relay. */
async function usesRelay(pc: RTCPeerConnection): Promise<boolean> {
  try {
    const stats = await pc.getStats();
    let pairId = '';
    stats.forEach((s) => { if (s.type === 'transport' && s.selectedCandidatePairId) pairId = s.selectedCandidatePairId; });
    let relayed = false;
    stats.forEach((s) => {
      if (s.type === 'candidate-pair' && (s.id === pairId || s.selected)) {
        const local = stats.get(s.localCandidateId);
        const remote = stats.get(s.remoteCandidateId);
        relayed = local?.candidateType === 'relay' || remote?.candidateType === 'relay';
      }
    });
    return relayed;
  } catch {
    return true; // asked for the relay and cannot tell otherwise: say so
  }
}

/**
 * Opens the session through the owner's Worker: punch first with free STUN; when it has not connected within its
 * window and the server has a TURN key, ask for credentials and try again with the relay.
 */
export async function openAssisted(target: AssistedTarget, auth: AssistedAuth, signal?: AbortSignal, deps: AssistedDeps = {}): Promise<Transport> {
  const punchMs = deps.punchMs ?? PUNCH_WINDOW_MS;
  const relayMs = deps.relayMs ?? RELAY_WINDOW_MS;
  const sig = await Signaling.open(target.url, auth, deps, 6000, signal);
  try {
    let first: { transport: Transport; pc: RTCPeerConnection } | null = null;
    let punchError: unknown;
    try {
      first = await attempt(sig, target, { iceServers: stunServers(), relay: false, windowMs: target.relay ? punchMs : punchMs + relayMs, failure: 'punch' }, deps, signal);
    } catch (e) {
      if (!target.relay || signal?.aborted || (e instanceof AssistedError && e.stage === 'signal')) throw e;
      punchError = e;
    }
    if (first) return withVia(first.transport, 'punched', sig);

    // The punch window passed: ask for relay credentials, then run the same exchange with them.
    let servers: RTCIceServer[];
    try {
      const reply = sig.expect(['ice']);
      reply.catch(() => {});
      await sig.send('relay', {}, '');
      const ice = await reply;
      servers = Array.isArray(ice.servers) ? (ice.servers as RTCIceServer[]) : [];
      if (!servers.length) throw new Error('no servers');
    } catch (e) {
      if (e instanceof AssistedError && e.stage === 'signal') throw e;
      throw new AssistedError('credential', `${e instanceof Error ? e.message : e}; the direct attempt said: ${punchError instanceof Error ? punchError.message : 'it did not connect'}`);
    }
    const second = await attempt(sig, target, { iceServers: [...stunServers(), ...servers], relay: true, windowMs: relayMs, failure: 'relay' }, deps, signal);
    return withVia(second.transport, (await usesRelay(second.pc)) ? 'relay' : 'punched', sig);
  } catch (e) {
    sig.close();
    throw e;
  }
}

function withVia(t: Transport, via: Via, sig: Signaling): Transport {
  t.via = via;
  sig.close(); // the session is up; the Worker is no longer needed
  return t;
}
