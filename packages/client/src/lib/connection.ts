import { CONTROL_TYPES, type ClientMessage, type ServerMessage } from './protocol';
import { openRtc } from './rtc';
import { openTransport, Unreachable, type Openers, type Target, type Transport } from './transport';
import { openWs } from './ws';
import type { Credentials } from './device';
import type { PathFailure } from './paths';
import { PROTOCOL_VERSION } from './extensions';
import { fromLegacy, toLegacy } from './legacy';

/** `checking`: still connected on paper, but the server has not answered for a few seconds (input may be lost). */
export type ConnState = 'connecting' | 'connected' | 'checking' | 'reconnecting' | 'unreachable' | 'closed';

const BACKOFF_MS = [1000, 2000, 4000, 8000, 16000, 30000];
const PING_EVERY_MS = 5000;
/** A hidden tab keeps pinging, slowly: a link that carries nothing for a minute or two is cut by NATs, tunnels and proxies. */
const HIDDEN_PING_EVERY_MS = 20000;
/** Coming back to the tab, a link that died while hidden is found out in seconds, not after the full window. */
const WAKE_PONG_WITHIN_MS = 3000;
/** This long without a reply to a ping, the UI is told the link is being checked. */
const STALE_AFTER_MS = 2500;
/** One late reply on a mobile link is normal; this long without any message means the connection is half-open. */
const PONG_WITHIN_MS = 12000;
/** After the network changed (Wi-Fi to cellular and back), a link that does not answer within this is re-raced (decision 0123). */
const NETWORK_PONG_WITHIN_MS = 2000;

/** The result of a control message (`Connection.request`). */
export interface Ack { ok: boolean; acknowledged: boolean; error?: string }

/** 1s, 2s, 4s, 8s, 16s, then 30s; +0..25% jitter so many tabs do not reconnect in lockstep. */
export function backoffDelay(attempt: number, random = Math.random): number {
  const base = BACKOFF_MS[Math.min(attempt, BACKOFF_MS.length - 1)];
  return Math.round(base * (1 + random() * 0.25));
}

/** The path of a transport; one opened without a race is named by its kind. */
function pathOf(t: Transport): { key: string; label: string } {
  return t.path ?? { key: t.kind, label: t.kind === 'rtc' ? 'Direct' : 'Tunnel' };
}

type NetInfo = { addEventListener(type: 'change', fn: () => void): void; removeEventListener(type: 'change', fn: () => void): void };
/** `navigator.connection` (Chrome, Android); absent in Safari, where `online`/`offline` are all there is. */
const netInfo = (): NetInfo | undefined => (navigator as unknown as { connection?: NetInfo }).connection;

export function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Keystrokes may be any Unicode; send them as UTF-8 bytes. */
export function textToB64(text: string): string {
  let bin = '';
  for (const b of new TextEncoder().encode(text)) bin += String.fromCharCode(b);
  return btoa(bin);
}

export interface ConnectionOptions {
  /** Resolved on every attempt, so a reconnect can pick up a new address from the directory. */
  target: () => Promise<Target>;
  openers?: Openers;
  /** The device certificate and its signer; asked for on every attempt, so a fresh login is picked up. */
  credentials: () => Promise<Credentials>;
  claim?: string;
  /** The server id this connection was made for (pinned when the server was added). The challenge must name it,
   *  so a server relaying another server's challenge gets no usable signature. */
  serverId?: string;
  /** `detail` explains `unreachable`. */
  onState: (state: ConnState, detail?: string) => void;
  /** Called after every successful auth, including after a reconnect. */
  onReady: (msg: Extract<ServerMessage, { type: 'auth_ok' }>, isReconnect: boolean) => void;
  /** The connection was lost: why, and how long it had been up (for the diagnostics and the status bar). */
  onDrop?: (reason: string, upMs: number) => void;
  /** The path now carrying the workspace (`Direct, LAN`, `Tunnel`), or null when there is none. */
  onPath?: (path: { key: string; label: string } | null) => void;
  /** Every path that was tried and failed in the last attempt, with why and one fix; empty after a success. */
  onFailures?: (failures: PathFailure[]) => void;
  /** The path that just authenticated: remembered per server and tried first next time. */
  onWinner?: (key: string) => void;
  /** Auth errors are final: retrying with the same credentials would be rejected again. */
  onFatal: (message: string) => void;
}

const DEFAULT_OPENERS: Openers = { rtc: openRtc, ws: openWs };

/** One authenticated connection to a server that heals itself: transport choice, backoff, heartbeat, wake-up on foreground. */
export class Connection {
  private transport: Transport | null = null;
  private attempt = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private pongTimer: ReturnType<typeof setTimeout> | null = null;
  private staleTimer: ReturnType<typeof setTimeout> | null = null;
  private checking = false;
  private retryAttempt = 0;
  private retryAt = 0;
  private listeners = new Set<(msg: ServerMessage) => void>();
  private authed = false;
  /** The server predates protocol versions: messages are translated to and from the old vocabulary (legacy.ts). */
  private legacy = false;
  /** The server acknowledges control messages and, over WebRTC, has the `ctl` channel for them (step 3.3). */
  private control = false;
  private nextReq = 1;
  private waiting = new Map<number, { resolve: (r: Ack) => void; timer: ReturnType<typeof setTimeout> }>();
  /** The server put terminal bytes on their own lane for this connection (`auth_ok.channels`). */
  private lanes = false;
  private everReady = false;
  private stopped = false;
  private opening = false;
  /** The next loss comes from a network change: re-race at once instead of waiting out the backoff. */
  private fastReconnect = false;
  /** Pings sent whose pong has not arrived yet: a pong is only the proof of life of the latest one once this is back to 0. */
  private pingsOut = 0;
  /** After a network change: only the answer to the ping asked for it counts, not other traffic or an older pong. */
  private proving = false;
  private pongDueAt = 0;
  private pingSentAt = 0;
  private readyAt = 0;
  /** How many times a live connection was lost since this one was created. */
  drops = 0;
  /** Last measured round trip in ms (heartbeat ping to pong), for the status bar. */
  rtt: number | null = null;

  constructor(private opts: ConnectionOptions) {
    document.addEventListener('visibilitychange', this.wake);
    window.addEventListener('online', this.wake);
    window.addEventListener('pageshow', this.wake);
    window.addEventListener('online', this.networkChanged);
    window.addEventListener('offline', this.networkChanged);
    netInfo()?.addEventListener('change', this.networkChanged);
  }

  /** The path carrying the workspace, for the status line: `Direct, LAN` or `Tunnel`. */
  get pathLabel(): string | null {
    return this.transport ? pathOf(this.transport).label : null;
  }

  /** Which transport is carrying the workspace, for a connection-quality indicator. */
  get kind(): 'rtc' | 'ws' | null {
    return this.transport?.kind ?? null;
  }

  /** While reconnecting: which attempt is next or running, and how long until it starts (null: running now). */
  get retry(): { attempt: number; inMs: number | null } | null {
    if (!this.retryAttempt) return null;
    return { attempt: this.retryAttempt, inMs: this.retryAt ? Math.max(0, this.retryAt - Date.now()) : null };
  }

  /** The user's "retry now": skips the backoff wait. Nothing happens while connected or already connecting. */
  retryNow() {
    if (this.stopped || this.opening || (this.transport && this.authed)) return;
    this.reconnectNow();
  }

  subscribe(fn: (msg: ServerMessage) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  start() {
    void this.open();
  }

  send(msg: ClientMessage) {
    if (!this.transport || !this.authed) return;
    // Keystrokes go as raw bytes on the lane when there is one, and as JSON otherwise.
    if (msg.type === 'input' && this.lanes && this.transport.sendTerm?.(b64ToBytes(msg.data))) return;
    const text = JSON.stringify(this.legacy ? toLegacy(msg) : msg);
    // Control messages use the control channel when there is one: they never queue behind terminal bytes or events.
    if (this.control && CONTROL_TYPES.has(msg.type) && this.transport.sendControl?.(text)) return;
    this.transport.send(text);
  }

  /** Sends a control message and resolves with the server's result. A server that does not acknowledge (an older one)
   *  resolves at once as `{ ok: true, acknowledged: false }`; a lost connection or no answer within `timeoutMs`
   *  resolves `{ ok: false }` with the reason. */
  request(msg: ClientMessage, timeoutMs = 8000): Promise<Ack> {
    if (!this.transport || !this.authed) return Promise.resolve({ ok: false, acknowledged: false, error: 'Not connected' });
    if (!this.control) {
      this.send(msg);
      return Promise.resolve({ ok: true, acknowledged: false });
    }
    const req = this.nextReq++;
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.waiting.delete(req);
        resolve({ ok: false, acknowledged: false, error: 'No answer from the server' });
      }, timeoutMs);
      this.waiting.set(req, { resolve, timer });
      this.send({ ...msg, req } as unknown as ClientMessage);
    });
  }

  /** Ends every request that waits for an answer: the connection it was sent on is gone. */
  private failWaiting(error: string) {
    for (const { resolve, timer } of this.waiting.values()) {
      clearTimeout(timer);
      resolve({ ok: false, acknowledged: false, error });
    }
    this.waiting.clear();
  }

  close() {
    this.stopped = true;
    this.failWaiting('The connection was closed');
    this.clearTimers();
    document.removeEventListener('visibilitychange', this.wake);
    window.removeEventListener('online', this.wake);
    window.removeEventListener('pageshow', this.wake);
    window.removeEventListener('online', this.networkChanged);
    window.removeEventListener('offline', this.networkChanged);
    netInfo()?.removeEventListener('change', this.networkChanged);
    this.transport?.close();
    this.transport = null;
    this.opts.onState('closed');
  }

  /**
   * The device changed network. A live link is asked for proof within 2 s and, if it does not answer, dropped and
   * raced again at once over every known path; a connection that is not up is retried now.
   */
  private networkChanged = () => {
    if (this.stopped) return;
    if (this.transport && this.authed) {
      this.fastReconnect = true;
      this.proving = true;
      this.ping(NETWORK_PONG_WITHIN_MS);
    } else if (!this.opening) {
      this.reconnectNow();
    }
  };

  private wake = () => {
    if (this.stopped) return;
    const hidden = document.visibilityState === 'hidden';
    if (this.transport && this.authed) {
      this.restartHeartbeat(); // the cadence depends on whether the tab is in view
      if (!hidden) this.ping(WAKE_PONG_WITHIN_MS); // a link that died while hidden shows itself within seconds
    } else if (!hidden && !this.opening) {
      this.reconnectNow();
    }
  };

  private reconnectNow() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.attempt = 0;
    this.retryAttempt = 0;
    this.retryAt = 0;
    void this.open();
  }

  private scheduleRetry() {
    if (this.stopped) return;
    const delay = backoffDelay(this.attempt++);
    this.retryAttempt = this.attempt;
    this.retryAt = Date.now() + delay;
    this.timer = setTimeout(() => void this.open(), delay);
  }

  private async open() {
    if (this.stopped || this.opening) return;
    this.opening = true;
    this.clearTimers();
    this.retryAt = 0; // an attempt is running now
    this.authed = false;
    this.legacy = false;
    this.control = false;
    this.lanes = false;
    this.opts.onState(this.everReady ? 'reconnecting' : 'connecting');

    let transport: Transport;
    try {
      transport = await openTransport(await this.opts.target(), this.opts.openers ?? DEFAULT_OPENERS);
    } catch (e) {
      this.opening = false;
      if (this.stopped) return;
      // The banner keeps one line; what each transport said goes to the console for whoever is debugging.
      if (e instanceof Unreachable) console.warn('[termenbox] connect failed:', e.attempts.join('; ') || e.message);
      this.opts.onPath?.(null);
      this.opts.onFailures?.(e instanceof Unreachable ? e.failures : []);
      this.opts.onState('unreachable', e instanceof Unreachable ? e.message : 'Server unreachable');
      this.scheduleRetry();
      return;
    }
    this.opening = false;
    if (this.stopped) {
      transport.close();
      return;
    }

    this.transport = transport;
    this.opts.onFailures?.([]);
    this.opts.onPath?.(pathOf(transport));
    transport.onmessage = (text) => this.onText(transport, text);
    if (transport.lanes) {
      transport.onterm = (bytes) => {
        if (this.transport !== transport) return;
        this.heard();
        this.listeners.forEach((fn) => fn({ type: 'term', bytes }));
      };
    }
    transport.onclose = () => {
      if (this.transport !== transport || this.stopped) return;
      this.dropped('the transport closed');
      this.clearTimers();
      this.authed = false;
      this.transport = null;
      this.opts.onState('reconnecting');
      if (this.fastReconnect) {
        this.fastReconnect = false;
        this.reconnectNow();
      } else this.scheduleRetry();
    };
    transport.send(JSON.stringify({ type: 'hello', protocol: PROTOCOL_VERSION }));
  }

  private onText(transport: Transport, text: string) {
    if (this.transport !== transport) return;
    const raw: ServerMessage = JSON.parse(text);
    // A server whose `auth_ok` names no protocol version speaks the old wire vocabulary (legacy.ts, step 3.2).
    const msg: ServerMessage = this.legacy ? fromLegacy(raw) : raw;
    if (msg.type === 'pong') {
      this.pingsOut = Math.max(0, this.pingsOut - 1);
      if (this.pingsOut === 0 && this.proving) {
        this.proving = false;
        this.fastReconnect = false; // the link answered the ping asked after the change: it did not cost the link
      }
    }
    this.heard();
    if (msg.type === 'challenge') {
      void this.answer(transport, msg.server_id, msg.nonce);
      return;
    }
    if (msg.type === 'auth_ok') {
      const isReconnect = this.everReady;
      this.authed = true;
      this.legacy = !(msg.protocol && msg.protocol >= 1);
      this.control = !!msg.control_channel;
      this.pingsOut = 0;
      this.proving = false;
      this.fastReconnect = false;
      this.lanes = !!transport.lanes && !!msg.channels?.includes('term');
      this.everReady = true;
      this.attempt = 0;
      this.retryAttempt = 0;
      this.retryAt = 0;
      this.checking = false;
      this.readyAt = performance.now();
      this.startHeartbeat();
      this.opts.onState('connected');
      if (transport.path) this.opts.onWinner?.(transport.path.key);
      this.opts.onReady(msg, isReconnect);
    } else if (msg.type === 'error' && !this.authed) {
      this.stopped = true;
      transport.close();
      this.opts.onState('closed');
      this.opts.onFatal(msg.message);
      return;
    }
    if (msg.type === 'pong') {
      if (this.pingSentAt) this.rtt = Math.round(performance.now() - this.pingSentAt);
    } else if (msg.type === 'control_ack') {
      const w = this.waiting.get(msg.req);
      if (w) {
        clearTimeout(w.timer);
        this.waiting.delete(msg.req);
        w.resolve({ ok: msg.ok, acknowledged: true, error: msg.error });
      }
    } else {
      this.listeners.forEach((fn) => fn(msg));
    }
  }

  /** Signs the server's challenge with the device key; the server id in it was pinned when the server was added. */
  private async answer(transport: Transport, serverId: string, nonce: string) {
    try {
      if (this.opts.serverId && this.opts.serverId !== serverId) {
        throw new Error('This server is not the one you added');
      }
      const { cert, sign } = await this.opts.credentials();
      const signature = await sign(serverId, nonce);
      if (this.transport === transport) {
        const auth: ClientMessage = { type: 'auth', cert, signature, server_token: this.opts.claim ?? '' };
        if (transport.lanes) auth.channels = true;
        transport.send(JSON.stringify(auth));
      }
    } catch (e) {
      if (this.transport !== transport) return;
      this.stopped = true;
      transport.close();
      this.opts.onState('closed');
      this.opts.onFatal(e instanceof Error ? e.message : 'Sign in again');
    }
  }

  private startHeartbeat() {
    if (this.pingTimer) return;
    const every = document.visibilityState === 'hidden' ? HIDDEN_PING_EVERY_MS : PING_EVERY_MS;
    this.pingTimer = setInterval(() => this.ping(), every);
  }

  private restartHeartbeat() {
    if (this.pingTimer) clearInterval(this.pingTimer);
    this.pingTimer = null;
    this.startHeartbeat();
  }

  private ping(within = PONG_WITHIN_MS) {
    if (this.stopped || !this.authed) return;
    this.pingSentAt = performance.now();
    this.pingsOut++;
    this.send({ type: 'ping' });
    this.armPongTimeout(within);
  }

  private armPongTimeout(within: number) {
    const due = performance.now() + within;
    if (this.pongTimer) {
      if (due >= this.pongDueAt) return; // one deadline at a time: the sooner one stays...
      clearTimeout(this.pongTimer); // ...so a foregrounding cannot stretch a network change's 2 s
    }
    this.pongDueAt = due;
    // Any message counts as proof of life (see heard()); silence means a half-open connection.
    this.pongTimer = setTimeout(() => {
      this.pongTimer = null;
      this.dropped(`no reply for ${within / 1000} s`);
      this.transport?.close();
      // close() on our own transport does not notify; treat it as a drop.
      this.transport?.onclose?.();
    }, within);
    if (this.staleTimer) clearTimeout(this.staleTimer);
    this.staleTimer = setTimeout(() => {
      this.staleTimer = null;
      if (!this.authed || this.checking) return;
      this.checking = true;
      this.opts.onState('checking', 'The server has not answered for a few seconds');
    }, Math.min(STALE_AFTER_MS, within - 500));
  }

  private dropped(reason: string) {
    if (!this.authed) return; // a failed attempt is reported as `unreachable`, not as a drop
    this.authed = false; // one loss is reported once, whichever path notices it first
    this.failWaiting('The connection dropped');
    this.drops++;
    this.opts.onDrop?.(reason, Math.round(performance.now() - this.readyAt));
  }

  private heard() {
    // After a network change, traffic may be what the old network still had in flight: only the answer to the
    // proof-of-life ping (see onText) ends the deadline.
    if (this.proving) return;
    if (this.pongTimer) clearTimeout(this.pongTimer);
    if (this.staleTimer) clearTimeout(this.staleTimer);
    this.pongTimer = this.staleTimer = null;
    if (this.checking) {
      this.checking = false;
      this.opts.onState('connected'); // it was only slow
    }
  }

  private clearTimers() {
    if (this.timer) clearTimeout(this.timer);
    if (this.pingTimer) clearInterval(this.pingTimer);
    if (this.pongTimer) clearTimeout(this.pongTimer);
    if (this.staleTimer) clearTimeout(this.staleTimer);
    this.timer = this.pingTimer = this.pongTimer = this.staleTimer = null;
    this.checking = false;
    this.proving = false;
    this.pingsOut = 0;
  }
}
