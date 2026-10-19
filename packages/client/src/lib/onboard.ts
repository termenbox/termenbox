// Add Server (DECISION-0056): the user's only job is to get a claim secret onto the VPS, by pasting one
// command (flow A) or scanning a QR the installer shows (flow B). Everything after that is automatic:
// the server announces itself to the directory with a proof that it knows the secret.
import { pending, type DirRecord } from './directory';
import type { ClientMessage, ServerMessage } from './protocol';
import { openTransport, Unreachable, type Target, type Transport } from './transport';

/** Secrets are 43 url-safe characters (32 random bytes). */
const SECRET = /^[A-Za-z0-9_-]{43}$/;

/**
 * Where sessions run (DECISION-0058, 0075). The installer takes it as a last word: none for the isolated container,
 * `host-user` for this machine as a dedicated unprivileged user, `host-me` as the user who runs the
 * installer (sessions open on their behalf, in their home), `host` for this machine as root.
 */
export type RunChoice = 'container' | 'host-user' | 'host-me' | 'host';

const modeWord = (mode: RunChoice) => (mode === 'container' ? '' : ` ${mode}`);

export function installCommand(origin: string, secret: string, mode: RunChoice = 'container'): string {
  return `curl -fsSL ${origin}/install.sh | sh -s -- ${secret} ${origin}${modeWord(mode)}`;
}

/** The command for the QR flow: the installer shows a QR instead of taking a secret (`-`). */
export function scanCommand(origin: string, mode: RunChoice = 'container'): string {
  return mode === 'container' ? `curl -fsSL ${origin}/install.sh | sh` : `curl -fsSL ${origin}/install.sh | sh -s -- - ${origin}${modeWord(mode)}`;
}

/** The text the installer's QR encodes: opening it in any camera app starts Add Server in the client. */
export function claimLink(origin: string, secret: string): string {
  return `${origin.replace(/\/$/, '')}/#claim=${secret}`;
}

/** The link that adds an already-registered server on another device (see `parseServerLink`). */
export function serverLink(origin: string, serverId: string): string {
  return `${origin.replace(/\/$/, '')}/#server=${serverId}`;
}

/**
 * The same thing as a line of text for the case where a link cannot be followed: an iPhone opens a tapped link in Safari,
 * never in the installed app, so the owner pastes this into Add Server inside the app instead.
 */
export function serverCode(serverId: string): string {
  return `termenbox-server:${serverId}`;
}

/** Accepts a claim link, a URL fragment (`#claim=...`) or the bare secret. */
export function parseClaim(text: string): string | null {
  const t = text.trim();
  const m = /[#&?]claim=([A-Za-z0-9_-]{43})(?![A-Za-z0-9_-])/.exec(t);
  if (m) return m[1];
  return SECRET.test(t) ? t : null;
}

/** The server id from a re-add link (`#server=...`), as printed by the installer for an already-registered server, or from its `serverCode`. */
export function parseServerLink(text: string): string | null {
  const m = /(?:[#&?]server=|(?:^|\s)termenbox-server:)([A-Za-z0-9_-]{43})(?![A-Za-z0-9_-])/.exec(text.trim());
  return m ? m[1] : null;
}

/** 2 s, 4 s, 8 s, then every 10 s: quick when the server is fast, cheap on the directory when it is not. */
export function pollDelay(attempt: number): number {
  return Math.min(2000 * 2 ** attempt, 10_000);
}

export const CLAIM_WINDOW_MS = 15 * 60_000;

export interface WaitDeps {
  pending: (origin: string, secret: string) => Promise<DirRecord | null>;
  sleep: (ms: number) => Promise<void>;
  now: () => number;
}

const realDeps: WaitDeps = {
  pending,
  sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
  now: () => Date.now(),
};

export class ClaimTimeout extends Error {
  constructor() {
    super('Nothing arrived within 15 minutes');
  }
}

/**
 * Polls the directory until the server announces itself with this secret. Resolves with the record,
 * already checked for a valid signature and claim proof. Rejects on timeout, abort, or a record that
 * fails verification (which is never retried: someone is answering for our secret and cannot prove it).
 */
export async function waitForServer(
  origin: string,
  secret: string,
  signal: AbortSignal,
  deps: WaitDeps = realDeps,
): Promise<DirRecord> {
  const started = deps.now();
  for (let attempt = 0; ; attempt++) {
    if (signal.aborted) throw new DOMException('aborted', 'AbortError');
    let record: DirRecord | null = null;
    try {
      record = await deps.pending(origin, secret);
    } catch (e) {
      if (e instanceof Error && e.message.includes('claim secret')) throw e;
      // Network or directory hiccup: keep waiting.
    }
    if (record) return record;
    if (deps.now() - started >= CLAIM_WINDOW_MS) throw new ClaimTimeout();
    await deps.sleep(pollDelay(attempt));
  }
}

/** What a claim link would add (decision 0117); `mode` is `container` or `host`, `run_user` is empty in a container. */
export interface ClaimPreview {
  name: string;
  mode: string;
  run_user: string;
  version: string;
}

export const PREVIEW_TIMEOUT_MS = 15_000;

export interface PreviewDeps {
  open: (target: Target) => Promise<Transport>;
}

const realPreviewDeps: PreviewDeps = {
  open: async (target) => {
    const [{ openRtc }, { openWs }] = await Promise.all([import('./rtc'), import('./ws')]);
    return openTransport(target, { rtc: openRtc, ws: openWs });
  },
};

/**
 * Asks the server what this claim would add, before anything is bound (decision 0117). Sends `hello`, then
 * `claim_preview` in place of `auth`: no device certificate is presented, so the owner is never set and the
 * token stays valid however often this runs. The connection is closed once there is an answer, and also when
 * `signal` aborts (the dialog was closed), in which case nothing more is sent.
 */
export function previewClaim(
  target: Target,
  secret: string,
  signal: AbortSignal,
  deps: PreviewDeps = realPreviewDeps,
  timeoutMs = PREVIEW_TIMEOUT_MS,
): Promise<ClaimPreview> {
  return new Promise<ClaimPreview>((resolve, reject) => {
    let transport: Transport | null = null;
    let done = false;
    const finish = (fn: () => void) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      signal.removeEventListener('abort', onAbort);
      transport?.close();
      fn();
    };
    const onAbort = () => finish(() => reject(new DOMException('aborted', 'AbortError')));
    const timer = setTimeout(() => finish(() => reject(new Error('The server did not answer'))), timeoutMs);
    if (signal.aborted) return onAbort();
    signal.addEventListener('abort', onAbort);
    const send = (m: ClientMessage) => transport?.send(JSON.stringify(m));
    deps.open(target).then(
      (t) => {
        transport = t;
        if (done) return t.close();
        t.onmessage = (text) => {
          let msg: ServerMessage;
          try { msg = JSON.parse(text); } catch { return; }
          if (msg.type === 'challenge') {
            if (target.record && msg.server_id !== target.record.server_id) {
              finish(() => reject(new Error('This server is not the one that announced itself')));
              return;
            }
            send({ type: 'claim_preview', server_token: secret });
          } else if (msg.type === 'claim_preview') {
            const answer = { name: msg.name, mode: msg.mode, run_user: msg.run_user ?? '', version: msg.version };
            finish(() => resolve(answer));
          } else if (msg.type === 'error') {
            finish(() => reject(new Error(msg.message)));
          }
        };
        t.onclose = () => finish(() => reject(new Error('The connection closed before the server answered')));
        send({ type: 'hello' });
      },
      (e) => finish(() => reject(e instanceof Error ? e : new Error('Server unreachable'))),
    );
  });
}

/** The reserved range a server binds from (decision 0118), and the one command an admin runs to open it. */
export const UDP_RANGE = '4443:4542';
export const firewallCommand = `ufw allow ${UDP_RANGE}/udp`;

/**
 * What the reachability check found: `reachable` (WebRTC opened), `degraded` (WebRTC failed but the WebSocket
 * fallback opened: it works, slower; `port` is the server's bound UDP port), `unreachable` (WebRTC and any
 * fallback failed) or `unknown` (the check could not run or failed for another reason, so nothing is claimed
 * either way).
 */
export type Reachability =
  | { state: 'reachable' }
  | { state: 'degraded'; port: number | null }
  | { state: 'unreachable'; port: number | null; message: string }
  | { state: 'unknown' };

/**
 * Runs the real connect once, after the server is added (decision 0118). It opens a transport and closes it
 * again without sending `auth`, so it never touches the claim. Advice only: the caller keeps the server
 * whatever this returns (GUARDRAIL connections-warn-never-drop).
 */
export async function checkReachable(target: Target, deps: PreviewDeps = realPreviewDeps): Promise<Reachability> {
  if (!target.record && !target.wsUrl) return { state: 'unknown' };
  try {
    const t = await deps.open(target);
    t.close();
    // A record means WebRTC was tried first, so a WebSocket here is the fallback. A bare URL has no other path.
    if (t.kind === 'ws' && target.record) return { state: 'degraded', port: target.record.udp_port ?? null };
    return { state: 'reachable' };
  } catch (e) {
    if (e instanceof Unreachable) return { state: 'unreachable', port: target.record?.udp_port ?? null, message: e.message };
    return { state: 'unknown' };
  }
}
