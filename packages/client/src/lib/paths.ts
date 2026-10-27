// Candidate paths (DECISION-0123): a server is one identity reached over several addresses the client races. This file
// is the pure part: which paths there are, in what order they start, the race itself, and what to tell the user when
// every one failed. `transport.ts` plugs the real openers in.
import type { Bundle, CandidateKind, ServerListItem } from './protocol';
import type { DirRecord } from './directory';
import { verifyBundle } from './bundle';

export type PathKind = CandidateKind | 'ws' | 'signal';

export interface Path {
  /** `lan 192.168.1.20:4443`: what is remembered as the winner. */
  key: string;
  kind: PathKind;
  /** What the status line shows: "Direct, LAN", "Tunnel". */
  label: string;
  addr: string;
  port: number;
  /** The DTLS fingerprint pinned for this server; the browser refuses a connection to any other. */
  fingerprint?: string;
  /** For a tunnel: the WebSocket URL. For the assisted path: the owner's signaling Worker. */
  url?: string;
  /** The assisted path only: the server has a TURN key, so a relay can be asked for. */
  relay?: boolean;
}

export interface PathFailure {
  path: Path;
  /** What the attempt itself reported. */
  reason: string;
  /** One thing to try. */
  fix: string;
}

export const LABELS: Record<PathKind, string> = {
  lan: 'Direct, LAN',
  wan6: 'Direct, IPv6',
  wan4: 'Direct, internet',
  overlay: 'Direct, overlay',
  tunnel: 'Tunnel',
  ws: 'Tunnel',
  // Becomes "Punched" or "Relay" once the session is up (assisted.ts).
  signal: 'Assisted',
};

/** Tunnels are the fallback tier: they start only when no direct path won (a relay is slower and shared). */
export const isTunnel = (p: Path) => p.kind === 'tunnel' || p.kind === 'ws';

const keyOf = (kind: PathKind, addr: string, port: number) => (kind === 'ws' || kind === 'tunnel' || kind === 'signal' ? `${kind} ${addr}` : `${kind} ${addr.includes(':') ? `[${addr}]` : addr}:${port}`);

/** Every path the client knows for a server: the bundle's candidates, the directory record and a tunnel URL. */
export function buildPaths(input: { bundle?: Bundle | null; record?: Pick<DirRecord, 'ip' | 'udp_port' | 'fingerprint' | 'ws_url'> | null; wsUrl?: string }): Path[] {
  const out: Path[] = [];
  const add = (p: Omit<Path, 'key' | 'label'>) => {
    const path: Path = { ...p, key: keyOf(p.kind, p.url ?? p.addr, p.port), label: LABELS[p.kind] };
    if (!out.some((o) => o.key === path.key)) out.push(path);
  };
  const { bundle, record } = input;
  for (const c of bundle?.candidates ?? []) {
    if (c.kind === 'tunnel') add({ kind: 'tunnel', addr: c.addr, port: c.port, url: c.addr });
    else add({ kind: c.kind, addr: c.addr, port: c.port, fingerprint: bundle!.fingerprint });
  }
  if (record?.ip) add({ kind: record.ip.includes(':') ? 'wan6' : 'wan4', addr: record.ip, port: record.udp_port, fingerprint: record.fingerprint });
  if (bundle?.signal) add({ kind: 'signal', addr: bundle.signal, port: 0, url: bundle.signal, fingerprint: bundle.fingerprint, relay: bundle.relay === true });
  const ws = record?.ws_url || input.wsUrl;
  if (ws) add({ kind: 'ws', addr: ws, port: 0, url: ws });
  return out;
}

const COST: Record<PathKind, number> = { lan: 0, wan6: 1, wan4: 2, overlay: 3, signal: 4, tunnel: 5, ws: 5 };

/** The remembered winner first, then cheapest first (LAN, IPv6, IPv4, overlay, tunnel). */
export function orderPaths(paths: Path[], winner?: string): Path[] {
  return paths
    .map((p, i) => ({ p, i }))
    .sort((a, b) => Number(b.p.key === winner) - Number(a.p.key === winner) || COST[a.p.kind] - COST[b.p.kind] || a.i - b.i)
    .map((x) => x.p);
}

/** One thing to try for a failed path, by kind. */
export function fixFor(path: Path): string {
  switch (path.kind) {
    case 'lan': return 'Join the same Wi-Fi as the server (and allow local network access for this browser if it asks).';
    case 'wan4': return `Open UDP port ${path.port} on the server's firewall or router, or run a server that opens it itself (UPnP).`;
    case 'wan6': return `Open UDP port ${path.port} for IPv6 on the server's firewall; this network may also have no IPv6.`;
    case 'signal': return 'Check that the server is running and that its signaling Worker exists (`termenbox-server --setup-cloudflare`).';
    case 'overlay': return 'Turn the overlay (Tailscale or WireGuard) on for this device and the server.';
    default: return 'Check that the tunnel is running on the server.';
  }
}

export class Unreachable extends Error {
  constructor(message: string, readonly attempts: string[], readonly failures: PathFailure[] = [], readonly advice = '') {
    super(message);
  }
}

/** True when the server advertises no public address, no overlay and no tunnel: only its own network can reach it. */
export function noWayFromOutside(paths: Path[]): boolean {
  return paths.length > 0 && paths.every((p) => p.kind === 'lan');
}

/** What to say when every path failed, beyond the per-path lines. */
export function allFailedAdvice(paths: Path[]): string {
  if (noWayFromOutside(paths)) {
    return 'This server advertises only a LAN address: no public address and no relay. That usually means it sits behind carrier-grade NAT (CGNAT) or a router that blocks incoming UDP. Away from its network, reach it over an overlay such as Tailscale or WireGuard, or from the same Wi-Fi. If it runs in WSL2 or another VM behind NAT, switch WSL2 to mirrored networking, or run `termenbox-server --setup-cloudflare` on it for a punched or relayed connection.';
  }
  return '';
}

export interface Raced<T> {
  value: T;
  path: Path;
}

export interface RaceOptions {
  /** Delay between starting two paths; 150-300 ms by default. */
  stagger?: () => number;
  signal?: AbortSignal;
}

const defaultStagger = () => 150 + Math.random() * 150;

/**
 * Starts the paths one after another (staggered, in the order given), keeps the first that connects and cancels the
 * rest. A path that fails starts the next at once. `null` when none connected; each failure is appended to `failures`.
 */
function raceTier<T extends { close(): void }>(
  tier: Path[],
  open: (p: Path, signal: AbortSignal) => Promise<T>,
  stagger: () => number,
  failures: PathFailure[],
  outer?: AbortSignal,
): Promise<Raced<T> | null> {
  return new Promise((resolve) => {
    if (!tier.length || outer?.aborted) return resolve(null);
    const ctrl = new AbortController();
    const timers: ReturnType<typeof setTimeout>[] = [];
    let next = 0;
    let settled = 0;
    let done = false;
    const finish = (r: Raced<T> | null) => {
      if (done) return;
      done = true;
      timers.forEach(clearTimeout);
      outer?.removeEventListener('abort', onAbort);
      ctrl.abort();
      resolve(r);
    };
    const onAbort = () => finish(null);
    outer?.addEventListener('abort', onAbort);
    const launch = () => {
      if (done || next >= tier.length) return;
      const path = tier[next++];
      open(path, ctrl.signal).then(
        (value) => {
          if (done) return value.close(); // a loser that connected after the winner
          finish({ value, path });
        },
        (e) => {
          if (done) return;
          const reason = e instanceof Error ? e.message : String(e);
          failures.push({ path, reason, fix: (e as { fix?: string } | null)?.fix ?? fixFor(path) });
          settled++;
          if (settled === tier.length) finish(null);
          else launch();
        },
      );
    };
    launch();
    let at = 0;
    for (let i = 1; i < tier.length; i++) {
      at += stagger();
      timers.push(setTimeout(launch, at));
    }
  });
}

/**
 * The race of DECISION-0123: direct paths staggered cheapest first, the first to connect wins and the others are
 * cancelled; tunnels are tried only when no direct path connected. Throws `Unreachable` with every failure.
 */
export async function racePaths<T extends { close(): void }>(
  paths: Path[],
  open: (p: Path, signal: AbortSignal) => Promise<T>,
  opts: RaceOptions = {},
): Promise<Raced<T>> {
  const failures: PathFailure[] = [];
  const stagger = opts.stagger ?? defaultStagger;
  const direct = paths.filter((p) => !isTunnel(p));
  const tunnels = paths.filter(isTunnel);
  const won = (await raceTier(direct, open, stagger, failures, opts.signal)) ?? (await raceTier(tunnels, open, stagger, failures, opts.signal));
  if (won) return won;
  const advice = allFailedAdvice(paths);
  throw new Unreachable(summary(failures, advice), failures.map((f) => `${f.path.key}: ${f.reason}`), failures, advice);
}

/** One line for the status banner; the screen lists the rest. */
export function summary(failures: PathFailure[], advice = ''): string {
  if (!failures.length) return 'Server unreachable';
  const n = failures.length;
  return `Could not connect over ${n === 1 ? 'the only known path' : `any of ${n} paths`}${advice ? '. ' + advice : '.'}`;
}

/** The newest stored bundle of a server, checked again (signature and pin) before it is used as a hint. */
export async function storedBundle(server: Pick<ServerListItem, 'bundle' | 'server_id'>): Promise<Bundle | null> {
  if (!server.bundle || !server.server_id) return null;
  try {
    return await verifyBundle(server.bundle, { nowSecs: Math.floor(Date.now() / 1000), pin: server.server_id, allowExpired: true });
  } catch {
    return null;
  }
}
