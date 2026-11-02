import type { DirRecord } from './directory';
import type { RtcTarget } from './rtc';
import type { Bundle } from './protocol';
import type { AssistedAuth, AssistedTarget, Via } from './assisted';
import { buildPaths, isTunnel, orderPaths, racePaths, Unreachable, type Path } from './paths';

const VIA_LABELS: Record<Via, string> = { punched: 'Punched', relay: 'Relay' };

export { Unreachable };

/** What `Connection` needs from a carrier: ordered JSON text messages and a close notice. */
export interface Transport {
  readonly kind: 'rtc' | 'ws';
  onmessage: ((text: string) => void) | null;
  onclose: (() => void) | null;
  send(text: string): void;
  close(): void;
  /** Terminal bytes on their own lane (decision 0096). Absent when the carrier has no such lane. */
  readonly lanes?: boolean;
  onterm?: ((bytes: Uint8Array) => void) | null;
  /** False when the lane is not open (yet): the caller falls back to the JSON `input` message. */
  sendTerm?(bytes: Uint8Array): boolean;
  /** One upload frame (decision 0194). False when the carrier has no upload lane open: an older server cannot take uploads. */
  sendUpload?(frame: Uint8Array): boolean;
  /** Control messages on a channel of their own (step 3.3). False when there is none (open): the caller uses `send`. */
  sendControl?(text: string): boolean;
  /** The path that carried this transport, when it was found by the race over candidate paths. */
  path?: Path;
  /** The assisted path only: whether the session was punched through or went over the relay. */
  via?: Via;
}

/** Where a server can be reached. A directory record is preferred; a plain WebSocket URL is the fallback. */
export interface Target {
  record?: DirRecord;
  wsUrl?: string;
  /** A verified candidate bundle (decision 0123); with candidates in it the paths are raced instead of tried in turn. */
  bundle?: Bundle | null;
  /** The key of the path that connected last: it starts first. */
  winner?: string;
}

export interface Openers {
  rtc(target: RtcTarget, timeoutMs: number, signal?: AbortSignal): Promise<Transport>;
  ws(url: string, timeoutMs: number, signal?: AbortSignal): Promise<Transport>;
  /** The assisted path through the owner's signaling Worker; absent where there is no device identity to sign with. */
  assisted?(target: AssistedTarget, auth: AssistedAuth, signal?: AbortSignal): Promise<Transport>;
}

/** ICE, DTLS and the data channel take several round trips; on a slow mobile link 5 s was too short and looped. */
export const RTC_TIMEOUT_MS = 10000;
export const WS_TIMEOUT_MS = 8000;

/**
 * The user-facing reason when WebRTC could not open: the UDP port is the thing to check. `cause` is what the
 * attempt itself reported (a timeout, a failed ICE check), so a reader can tell a closed port from the rest.
 */
export function describeRtcFailure(record: DirRecord, cause?: string): string {
  const why = cause ? ` (${cause})` : '';
  return `Server unreachable on UDP port ${record.udp_port}${why}. Check that the port is open in the server's firewall and that this network allows UDP.`;
}

/**
 * Opens the best available transport: WebRTC-direct from the directory record first, then the
 * WebSocket (tunnel) fallback. Throws Unreachable with a message the UI can show.
 */
export async function openTransport(target: Target, openers: Openers, auth?: AssistedAuth): Promise<Transport> {
  if (target.bundle?.candidates.length || (auth && target.bundle?.signal)) return raceTransport(target, openers, auth);
  const attempts: string[] = [];
  let rtcFailure: string | null = null;

  if (target.record) {
    try {
      return await openers.rtc(target.record, RTC_TIMEOUT_MS);
    } catch (e) {
      const cause = e instanceof Error ? e.message : String(e);
      rtcFailure = describeRtcFailure(target.record, cause);
      attempts.push(`rtc: ${cause}`);
    }
  }

  const url = target.record?.ws_url || target.wsUrl;
  if (url) {
    try {
      return await openers.ws(url, WS_TIMEOUT_MS);
    } catch (e) {
      attempts.push(`ws: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  throw new Unreachable(rtcFailure ?? 'Server unreachable', attempts);
}

/**
 * Decision 0123: every path the client knows (the bundle's candidates, the directory record, a tunnel URL) is raced,
 * the remembered winner and the cheapest paths first, staggered 150-300 ms; the first to connect wins and the rest are
 * cancelled. Every attempt pins the server's DTLS fingerprint, whichever address it came from.
 */
async function raceTransport(target: Target, openers: Openers, auth?: AssistedAuth): Promise<Transport> {
  // The assisted path signs with the device key: without an identity (or an opener) it is not a candidate at all.
  const usable = (p: Path) => p.kind !== 'signal' || (!!auth && !!openers.assisted);
  const paths = orderPaths(buildPaths({ bundle: target.bundle, record: target.record, wsUrl: target.wsUrl }).filter(usable), target.winner);
  const { value, path } = await racePaths(paths, (p, signal) => {
    if (p.kind === 'signal') return openers.assisted!({ url: p.url!, fingerprint: p.fingerprint!, relay: p.relay === true }, auth!, signal);
    return isTunnel(p) ? openers.ws(p.url!, WS_TIMEOUT_MS, signal) : openers.rtc({ ip: p.addr, udp_port: p.port, fingerprint: p.fingerprint! }, RTC_TIMEOUT_MS, signal);
  });
  value.path = value.via ? { ...path, label: VIA_LABELS[value.via] } : path;
  return value;
}
