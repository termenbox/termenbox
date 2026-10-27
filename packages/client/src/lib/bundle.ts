// Candidate bundles (DECISION-0123): the addresses a server can be reached at, signed by its Ed25519 key. Nothing in a
// bundle is used, previewed or cached before `verifyBundle` passes (GUARDRAIL-connection-hints-are-signed-and-never-
// identity). The layout is shared with packages/core/src/bundle.rs, which documents it; the shared vector in
// packages/core/testdata/bundle-vectors.json keeps both sides honest.
import { b64urlDecode, b64urlEncode } from '../../functions/api/directory/_lib';
import type { Bundle, Candidate, CandidateKind } from './protocol';

const MAGIC = 'tbxb2';
// The same layout plus the signal URL and the relay flag; a server signs it only when it has either (DECISION-0141).
const MAGIC_ASSISTED = 'tbxb3';
const KINDS: CandidateKind[] = ['lan', 'wan4', 'wan6', 'overlay', 'tunnel'];
const SIG_LEN = 64;
const MAX_CANDIDATES = 16;
const MAX_NAME = 64;
const MAX_SIGNAL = 200;

export type BundleFailure = 'malformed' | 'bad_signature' | 'expired' | 'stale' | 'wrong_server';

export class BundleRejected extends Error {
  constructor(readonly reason: BundleFailure) {
    super(reason);
  }
}

const bad = (reason: BundleFailure = 'malformed') => new BundleRejected(reason);

const IPV4 = /^(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$/;

function isIpv6(addr: string): boolean {
  if (!/^[0-9a-fA-F:.]+$/.test(addr) || !addr.includes(':')) return false;
  try {
    return new URL(`http://[${addr}]/`).hostname !== '';
  } catch {
    return false;
  }
}

/**
 * What a candidate's address may be, by kind. A bundle from a pairing link is signed by whatever key its maker chose, so
 * the signature does not make its contents safe: addresses go into an SDP `c=` line and WebSocket URLs, and must be
 * nothing but an IP address (direct paths) or a plain `wss://` host (tunnels).
 */
export function validAddr(kind: CandidateKind, addr: string): boolean {
  switch (kind) {
    case 'wan4': return IPV4.test(addr);
    case 'wan6': return isIpv6(addr);
    case 'lan':
    case 'overlay': return IPV4.test(addr) || isIpv6(addr);
    case 'tunnel': {
      if (!/^wss:\/\/[A-Za-z0-9.:\-_[\]/?=&%~+#@!$'()*,;]*$/.test(addr)) return false;
      try {
        const u = new URL(addr);
        return u.protocol === 'wss:' && u.hostname !== '' && !u.username && !u.password;
      } catch {
        return false;
      }
    }
  }
}

/** A signal URL is a plain `wss://` host with no credentials, as a tunnel's is. */
export function validSignal(url: string): boolean {
  return new TextEncoder().encode(url).length <= MAX_SIGNAL && validAddr('tunnel', url);
}

function fingerprintBytes(fp: string): Uint8Array {
  const m = /^sha-256 ((?:[0-9A-F]{2}:){31}[0-9A-F]{2})$/.exec(fp);
  if (!m) throw bad();
  return Uint8Array.from(m[1].split(':'), (h) => parseInt(h, 16));
}

const fingerprintText = (bytes: Uint8Array) => `sha-256 ${Array.from(bytes, (b) => b.toString(16).padStart(2, '0').toUpperCase()).join(':')}`;

/** The exact bytes the server signs. */
export function signedBytes(b: Bundle): Uint8Array {
  const enc = new TextEncoder();
  const name = enc.encode(b.name);
  if (b.candidates.length > MAX_CANDIDATES || name.length > MAX_NAME) throw bad();
  const parts: number[] = [];
  const push = (bytes: ArrayLike<number>) => { for (let i = 0; i < bytes.length; i++) parts.push(bytes[i]); };
  const u64 = (n: number) => { const v = new DataView(new ArrayBuffer(8)); v.setBigUint64(0, BigInt(n)); push(new Uint8Array(v.buffer)); };
  const assisted = b.signal !== undefined || b.relay === true;
  if (b.signal !== undefined && !validSignal(b.signal)) throw bad();
  push(enc.encode(assisted ? MAGIC_ASSISTED : MAGIC));
  const id = b64urlDecode(b.server_id);
  if (id.length !== 32) throw bad();
  push(id);
  u64(b.seq);
  u64(b.expires);
  push([(b.published ? 1 : 0) | (b.relay ? 2 : 0)]);
  push(fingerprintBytes(b.fingerprint));
  push([name.length]);
  push(name);
  push([b.candidates.length]);
  for (const c of b.candidates) {
    const addr = enc.encode(c.addr);
    const kind = KINDS.indexOf(c.kind) + 1;
    if (addr.length > 255 || kind < 1 || c.port < 0 || c.port > 65535) throw bad();
    push([kind, c.port >> 8, c.port & 0xff, addr.length]);
    push(addr);
  }
  if (assisted) {
    const signal = enc.encode(b.signal ?? '');
    push([signal.length]);
    push(signal);
  }
  return Uint8Array.from(parts);
}

/** Parses the wire string without checking the signature. */
export function decodeBundle(wire: string): { bundle: Bundle; signed: Uint8Array; sig: Uint8Array } {
  let bytes: Uint8Array;
  try {
    if (!/^[A-Za-z0-9_-]+$/.test(wire.trim())) throw bad();
    bytes = b64urlDecode(wire.trim());
  } catch {
    throw bad();
  }
  if (bytes.length < MAGIC.length + SIG_LEN) throw bad();
  const body = bytes.slice(0, bytes.length - SIG_LEN);
  const sig = bytes.slice(bytes.length - SIG_LEN);
  let at = 0;
  const take = (n: number) => {
    if (at + n > body.length) throw bad();
    const out = body.slice(at, at + n);
    at += n;
    return out;
  };
  const dec = new TextDecoder('utf-8', { fatal: true });
  const text = (n: number) => { try { return dec.decode(take(n)); } catch (e) { throw e instanceof BundleRejected ? e : bad(); } };
  const u64 = () => { const b = take(8); return Number(new DataView(b.buffer, b.byteOffset, 8).getBigUint64(0)); };
  const magic = text(5);
  if (magic !== MAGIC && magic !== MAGIC_ASSISTED) throw bad();
  const assisted = magic === MAGIC_ASSISTED;
  const server_id = b64urlEncode(take(32));
  const seq = u64();
  const expires = u64();
  const flags = take(1)[0];
  if (flags & ~(assisted ? 3 : 1)) throw bad(); // a flag this client does not know is not ignored
  const fingerprint = fingerprintText(take(32));
  const name = text(take(1)[0]);
  const count = take(1)[0];
  if (count > MAX_CANDIDATES || new TextEncoder().encode(name).length > MAX_NAME) throw bad();
  const candidates: Candidate[] = [];
  for (let i = 0; i < count; i++) {
    const kind = KINDS[take(1)[0] - 1];
    if (!kind) throw bad();
    const p = take(2);
    const addr = text(take(1)[0]);
    if (!validAddr(kind, addr)) throw bad();
    candidates.push({ kind, port: (p[0] << 8) | p[1], addr });
  }
  let signal: string | undefined;
  if (assisted) {
    const len = take(1)[0];
    if (len > 0) {
      signal = text(len);
      if (!validSignal(signal)) throw bad();
    }
    if (signal === undefined && !(flags & 2)) throw bad(); // tbxb3 with nothing in it is a second spelling of tbxb2
  }
  if (at !== body.length) throw bad();
  const bundle: Bundle = { server_id, seq, expires, published: (flags & 1) === 1, fingerprint, name, candidates };
  if (signal !== undefined) bundle.signal = signal;
  if (flags & 2) bundle.relay = true;
  // The signature is over these exact bytes: anything that does not re-encode to them is not what was signed.
  const signed = signedBytes(bundle);
  if (signed.length !== body.length || signed.some((v, i) => v !== body[i])) throw bad();
  return { bundle, signed, sig };
}

export interface VerifyOptions {
  nowSecs: number;
  /** The server id stored for a known server. */
  pin?: string;
  /** The highest `seq` accepted so far for a known server: a bundle must be above it. */
  storedSeq?: number;
  /** A stored bundle is kept as a hint after it expires; its signature still has to hold. */
  allowExpired?: boolean;
}

/** Decodes and verifies a bundle; throws `BundleRejected` with the reason. */
export async function verifyBundle(wire: string, opts: VerifyOptions): Promise<Bundle> {
  const { bundle, signed, sig } = decodeBundle(wire);
  if (opts.pin !== undefined && bundle.server_id !== opts.pin) throw bad('wrong_server');
  let ok = false;
  try {
    const key = await crypto.subtle.importKey('raw', b64urlDecode(bundle.server_id), { name: 'Ed25519' }, false, ['verify']);
    ok = await crypto.subtle.verify({ name: 'Ed25519' }, key, sig as BufferSource, signed as BufferSource);
  } catch {
    ok = false;
  }
  if (!ok) throw bad('bad_signature');
  if (!opts.allowExpired && bundle.expires <= opts.nowSecs) throw bad('expired');
  if (opts.storedSeq !== undefined && bundle.seq <= opts.storedSeq) throw bad('stale');
  return bundle;
}
