// The Add Server wizard's logic (DECISION-0127): four steps, the live Connect checklist, and the mapping from a failure
// to one cause and one action (GUARDRAIL errors-fit-one-screen). No DOM and no Svelte here: the components render
// `AddServerState` and feed it events, and `connectPaths` reports each address as it is tried.
import { racePaths, Unreachable, type Path, type PathFailure, type Raced } from './paths';

export type Step = 'install' | 'connect' | 'confirm' | 'done';
export const STEPS: Step[] = ['install', 'connect', 'confirm', 'done'];

/** Where Connect is: code read, looking for the server, found it, reading what it is. */
export type Phase = 'code' | 'looking' | 'found' | 'reading';

export type AddressState = 'trying' | 'failed' | 'won';

export type FailReason = 'timeout' | 'refused' | 'denied' | 'unreachable' | 'other';

export interface AddressRow {
  key: string;
  label: string;
  /** `192.168.1.5:4443`: what was tried, so the user can tell which machine it is. */
  addr?: string;
  state: AddressState;
  reason?: FailReason;
  /** What the attempt itself said; shown only inside the fold. */
  message?: string;
}

/** Why Connect failed, as one thing the user can act on. */
export type Cause =
  | 'different_network'
  | 'guest_isolation'
  | 'local_network_denied'
  | 'server_stopped'
  | 'link_expired'
  | 'bad_link'
  | 'udp_closed'
  | 'wrong_code'
  | 'code_burned';

export type ActionId = 'retry' | 'rescan' | 'edit_code' | 'rerun_installer' | 'open_settings' | 'copy_firewall';

export interface Diagnosis {
  cause: Cause;
  /** One sentence: what is wrong. */
  title: string;
  /** The one primary action. */
  action: { id: ActionId; label: string };
  /** The detail behind the fold (closed by default). */
  detail: string;
}

export interface Connect {
  phase: Phase;
  addresses: AddressRow[];
  diagnosis?: Diagnosis;
  /** The server is reachable, but only over the WebSocket fallback: shown as a notice, not an error. */
  notice?: 'ws_fallback';
}

export interface AddServerState {
  step: Step;
  connect: Connect;
}

export const initialState = (): AddServerState => ({ step: 'install', connect: { phase: 'code', addresses: [] } });

export type Event =
  | { type: 'next' }
  | { type: 'back' }
  /** A link, scan or code was read (or the announcement is being awaited): Connect starts looking. */
  | { type: 'looking'; addresses?: AddressRow[] }
  | { type: 'address'; row: AddressRow }
  | { type: 'found' }
  | { type: 'reading' }
  | { type: 'previewed' }
  | { type: 'fallback' }
  | { type: 'failed'; diagnosis: Diagnosis }
  | { type: 'confirmed' }
  | { type: 'reset' };

const withConnect = (s: AddServerState, patch: Partial<Connect>, step: Step = s.step): AddServerState => ({ step, connect: { ...s.connect, ...patch } });

/** Pure transition. An event that does not apply in the current state leaves it unchanged. */
export function reduce(s: AddServerState, e: Event): AddServerState {
  switch (e.type) {
    case 'reset':
      return initialState();
    case 'next':
      if (s.step === 'install') return { ...s, step: 'connect' };
      return s;
    case 'back':
      if (s.step === 'connect') return { step: 'install', connect: { phase: 'code', addresses: [] } };
      if (s.step === 'confirm') return { step: 'connect', connect: { phase: 'code', addresses: [] } };
      return s;
    case 'looking':
      if (s.step !== 'connect') return s;
      return withConnect(s, { phase: 'looking', addresses: e.addresses ?? [], diagnosis: undefined, notice: undefined });
    case 'address': {
      if (s.step !== 'connect') return s;
      const at = s.connect.addresses.findIndex((a) => a.key === e.row.key);
      const addresses = at < 0 ? [...s.connect.addresses, e.row] : s.connect.addresses.map((a, i) => (i === at ? e.row : a));
      return withConnect(s, { addresses });
    }
    case 'found':
      return s.step === 'connect' ? withConnect(s, { phase: 'found', diagnosis: undefined }) : s;
    case 'reading':
      return s.step === 'connect' ? withConnect(s, { phase: 'reading' }) : s;
    case 'previewed':
      return s.step === 'connect' ? withConnect(s, {}, 'confirm') : s;
    case 'fallback':
      return s.step === 'connect' || s.step === 'done' ? withConnect(s, { notice: 'ws_fallback' }) : s;
    case 'failed':
      // A failure stays on the step it happened in, as a diagnosis: Connect goes back to its first phase for the retry.
      return s.step === 'connect' ? withConnect(s, { phase: 'code', diagnosis: e.diagnosis }) : s;
    case 'confirmed':
      return s.step === 'confirm' ? { ...s, step: 'done' } : s;
  }
}

// ---- failures --------------------------------------------------------------------------------------------------

/** What one attempt reported, as a reason the UI can group on. */
export function reasonOf(e: unknown): FailReason {
  const text = e instanceof Error ? `${e.name} ${e.message}` : String(e);
  if (/NotAllowed|denied|permission/i.test(text)) return 'denied';
  if (/timed? ?out|timeout/i.test(text)) return 'timeout';
  if (/refused|reset|closed/i.test(text)) return 'refused';
  if (/unreachable|network/i.test(text)) return 'unreachable';
  return 'other';
}

/** The signals the wizard can turn into a diagnosis. */
export type Signal =
  | { type: 'link'; result: 'expired' | 'invalid' }
  | { type: 'code'; result: 'wrong' | 'burned' }
  /** Nothing announced itself within the 15 minutes, or the server answered nothing at all. */
  | { type: 'silence' }
  | {
      type: 'paths';
      failures: { kind: Path['kind']; reason: FailReason }[];
      /** Whether this device is on the server's own network, when that is known (null when it is not). */
      sameNetwork?: boolean | null;
      /** The WebSocket fallback opened although every direct path failed. */
      wsWorked?: boolean;
    };

const D = (cause: Cause, title: string, label: string, id: ActionId, detail: string): Diagnosis => ({ cause, title, action: { id, label }, detail });

export const DIAGNOSES: Record<Cause, Diagnosis> = {
  different_network: D('different_network', 'This device is not on the server\'s network.', 'Try again', 'retry', 'The link only lists a local address. Join the same Wi-Fi as the server, then try again. A VPN on this device can also hide the local network.'),
  guest_isolation: D('guest_isolation', 'This Wi-Fi may not let devices talk to each other.', 'Try again', 'retry', 'Guest networks and some routers isolate devices (client isolation). Move this device and the server to the main network, then try again.'),
  local_network_denied: D('local_network_denied', 'This browser is not allowed to reach your local network.', 'Open settings', 'open_settings', 'On iPhone: Settings > Privacy & Security > Local Network, switch this browser on. On Android: allow nearby devices for the browser. Then try again.'),
  server_stopped: D('server_stopped', 'The server did not answer.', 'Try again', 'retry', 'Check that it is running: `systemctl --user status termenbox` or `docker ps`. The installer prints a new link whenever you run it again.'),
  link_expired: D('link_expired', 'This link has expired.', 'Run the installer again', 'rerun_installer', 'A pairing link works for 15 minutes. Running the install command again prints a new one.'),
  bad_link: D('bad_link', 'That is not a valid Termenbox link.', 'Scan again', 'rescan', 'The link was cut off or altered, or it was not made by a Termenbox server. Copy the whole line from the terminal, or scan the QR.'),
  udp_closed: D('udp_closed', 'The server\'s UDP port is closed.', 'Copy firewall commands', 'copy_firewall', 'Direct connections need the server\'s UDP port open in its firewall and, behind a router, forwarded. The firewall commands for ufw and firewalld are in the port help.'),
  wrong_code: D('wrong_code', 'That code is not right.', 'Check the code', 'edit_code', 'The code is the 8 characters printed under the QR, for example K7QM-4TXD. Letters can be lower case.'),
  code_burned: D('code_burned', 'This code can no longer be used.', 'Run the installer again', 'rerun_installer', 'A code stops working after too many wrong tries, after 15 minutes, or once it has been used. Running the install command again prints a new one.'),
};

/** Maps a signal to the one cause to show. A working fallback is not a failure: `null`, the caller shows the notice. */
export function diagnose(s: Signal): Diagnosis | null {
  switch (s.type) {
    case 'link': return DIAGNOSES[s.result === 'expired' ? 'link_expired' : 'bad_link'];
    case 'code': return DIAGNOSES[s.result === 'burned' ? 'code_burned' : 'wrong_code'];
    case 'silence': return DIAGNOSES.server_stopped;
    case 'paths': {
      if (s.wsWorked) return null;
      const fs = s.failures;
      if (fs.some((f) => f.reason === 'denied')) return DIAGNOSES.local_network_denied;
      const direct = fs.filter((f) => f.kind !== 'tunnel' && f.kind !== 'ws');
      const lanOnly = fs.length > 0 && fs.every((f) => f.kind === 'lan');
      if (lanOnly) return DIAGNOSES[s.sameNetwork === true ? 'guest_isolation' : 'different_network'];
      // A public or overlay address that timed out or was refused: the port, not the server.
      if (direct.some((f) => f.kind === 'wan4' || f.kind === 'wan6') && direct.every((f) => f.reason === 'timeout' || f.reason === 'refused' || f.reason === 'unreachable')) return DIAGNOSES.udp_closed;
      return DIAGNOSES.server_stopped;
    }
  }
}

// ---- trying the addresses --------------------------------------------------------------------------------------

export interface ConnectResult<T> {
  /** Set when an address connected. */
  won?: Raced<T>;
  /** Set when none did; `null` means a fallback worked and the caller shows the notice. */
  diagnosis?: Diagnosis | null;
  failures: PathFailure[];
}

/**
 * Races the addresses (decision 0123) and reports each one to `emit` as it goes: trying, then failed with a reason, or
 * won. `open` is the real opener in the app and a fake in tests. A tunnel that connects after every direct path failed
 * is the fallback: it wins, and `fallback` is true so the UI shows a notice instead of an error.
 */
export async function connectPaths<T extends { close(): void }>(
  paths: Path[],
  open: (p: Path, signal: AbortSignal) => Promise<T>,
  emit: (e: Event) => void,
  opts: { signal?: AbortSignal; stagger?: () => number; sameNetwork?: boolean | null } = {},
): Promise<ConnectResult<T> & { fallback: boolean }> {
  const addrOf = (p: Path) => (p.url ?? (p.addr.includes(':') ? `[${p.addr}]:${p.port}` : `${p.addr}:${p.port}`));
  const rows = paths.map((p): AddressRow => ({ key: p.key, label: p.label, addr: addrOf(p), state: 'trying' }));
  emit({ type: 'looking', addresses: rows });
  const reasons = new Map<string, { reason: FailReason; message: string }>();
  const tracked = async (p: Path, signal: AbortSignal) => {
    try {
      return await open(p, signal);
    } catch (e) {
      const row = { key: p.key, label: p.label, addr: addrOf(p), state: 'failed' as const, reason: reasonOf(e), message: e instanceof Error ? e.message : String(e) };
      reasons.set(p.key, { reason: row.reason, message: row.message });
      emit({ type: 'address', row });
      throw e;
    }
  };
  try {
    const won = await racePaths(paths, tracked, { signal: opts.signal, stagger: opts.stagger });
    emit({ type: 'address', row: { key: won.path.key, label: won.path.label, addr: addrOf(won.path), state: 'won' } });
    // Direct paths all failed and a tunnel or WebSocket won: it works, so it is a notice.
    const directFailed = paths.some((p) => p.kind !== 'tunnel' && p.kind !== 'ws') && paths.filter((p) => p.kind !== 'tunnel' && p.kind !== 'ws').every((p) => reasons.has(p.key));
    const fallback = (won.path.kind === 'tunnel' || won.path.kind === 'ws') && directFailed;
    if (fallback) emit({ type: 'fallback' });
    return { won, failures: [], fallback };
  } catch (e) {
    if (!(e instanceof Unreachable)) throw e;
    const diagnosis = diagnose({
      type: 'paths',
      failures: e.failures.map((f) => ({ kind: f.path.kind, reason: reasons.get(f.path.key)?.reason ?? reasonOf(f.reason) })),
      sameNetwork: opts.sameNetwork,
    });
    if (diagnosis) emit({ type: 'failed', diagnosis });
    return { diagnosis, failures: e.failures, fallback: false };
  }
}
