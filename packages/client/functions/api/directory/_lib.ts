// Signed server directory. The directory stores only what a server signs about itself (public key,
// address, DTLS fingerprint, optional tunnel URL) and never sees terminal traffic. Records are
// self-certifying: `server_id` is the Ed25519 public key, so a client that pinned the key rejects
// any record the directory forges or alters.

export interface D1Like {
  prepare(sql: string): {
    bind(...args: unknown[]): {
      first<T = Record<string, unknown>>(): Promise<T | null>;
      run(): Promise<unknown>;
      all<T = Record<string, unknown>>(): Promise<{ results: T[] }>;
    };
  };
  /** Runs statements prepared with `prepare(..).bind(..)` in order, all or nothing. */
  batch(statements: unknown[]): Promise<unknown[]>;
}

export interface DirRecord {
  server_id: string;
  seq: number;
  ip: string;
  udp_port: number;
  fingerprint: string;
  ws_url: string;
  claim_hash: string;
  claim_proof: string;
  sig: string;
}

export interface Reply {
  status: number;
  body: Record<string, unknown>;
}

export const MIN_ANNOUNCE_INTERVAL_MS = 5_000;
export const NEW_IDS_PER_SRC_PER_HOUR = 30;
export const CLAIM_TTL_MS = 15 * 60_000;
export const IDLE_EXPIRY_MS = 30 * 24 * 3600_000;

const B64URL = /^[A-Za-z0-9_-]+$/;
const FINGERPRINT = /^sha-256 ([0-9A-F]{2}:){31}[0-9A-F]{2}$/;

export function b64urlDecode(s: string): Uint8Array<ArrayBuffer> {
  const b64 = s.replace(/-/g, '+').replace(/_/g, '/');
  const bin = atob(b64 + '='.repeat((4 - (b64.length % 4)) % 4));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

export function b64urlEncode(bytes: Uint8Array): string {
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** The exact bytes a server signs. Keep in sync with packages/server/src/directory.rs. */
export function canonicalMessage(r: DirRecord): string {
  return [
    'termenbox-announce-v1',
    r.server_id,
    String(r.seq),
    r.ip,
    String(r.udp_port),
    r.fingerprint,
    r.ws_url,
    r.claim_hash,
    r.claim_proof,
  ].join('\n');
}

export async function verifySignature(r: DirRecord): Promise<boolean> {
  try {
    const key = await crypto.subtle.importKey('raw', b64urlDecode(r.server_id), { name: 'Ed25519' }, false, ['verify']);
    return await crypto.subtle.verify({ name: 'Ed25519' }, key, b64urlDecode(r.sig), new TextEncoder().encode(canonicalMessage(r)));
  } catch {
    return false;
  }
}

// --- address validation (mirror of netutil::is_public_unicast in the server) ----------------------

function parseIPv4(s: string): number[] | null {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(s);
  if (!m) return null;
  const o = m.slice(1).map(Number);
  return o.every((n) => n <= 255) && o.every((n, i) => String(n) === m[i + 1]) ? o : null;
}

export function isPublicIPv4(o: number[]): boolean {
  const [a, b, c] = o;
  if (a === 0 || a === 10 || a === 127 || a >= 224) return false; // this-net, private, loopback, multicast/reserved/broadcast
  if (a === 100 && b >= 64 && b < 128) return false; // carrier-grade NAT
  if (a === 169 && b === 254) return false; // link-local
  if (a === 172 && b >= 16 && b < 32) return false; // private
  if (a === 192 && b === 168) return false; // private
  if (a === 192 && b === 0 && (c === 0 || c === 2)) return false; // IETF protocol, TEST-NET-1
  if (a === 198 && (b === 18 || b === 19)) return false; // benchmarking
  if (a === 198 && b === 51 && c === 100) return false; // TEST-NET-2
  if (a === 203 && b === 0 && c === 113) return false; // TEST-NET-3
  return true;
}

function expandIPv6(s: string): number[] | null {
  if (!/^[0-9a-fA-F:.]+$/.test(s) || s.split('::').length > 2) return null;
  let tail: number[] = [];
  let str = s;
  const v4 = /(\d+\.\d+\.\d+\.\d+)$/.exec(s);
  if (v4) {
    const o = parseIPv4(v4[1]);
    if (!o) return null;
    tail = [(o[0] << 8) | o[1], (o[2] << 8) | o[3]];
    str = s.slice(0, s.length - v4[1].length) + '0:0';
  }
  const [head, rest] = str.split('::');
  const h = head ? head.split(':') : [];
  const t = rest !== undefined && rest ? rest.split(':') : [];
  const missing = 8 - h.length - t.length;
  if (rest === undefined ? h.length !== 8 : missing < 1) return null;
  const groups = rest === undefined ? h : [...h, ...Array(missing).fill('0'), ...t];
  const nums = groups.map((g) => (/^[0-9a-fA-F]{1,4}$/.test(g) ? parseInt(g, 16) : NaN));
  if (nums.some(Number.isNaN)) return null;
  if (tail.length) {
    nums[6] = tail[0];
    nums[7] = tail[1];
  }
  return nums;
}

export function isPublicIPv6(g: number[]): boolean {
  if (g.every((x) => x === 0)) return false; // ::
  if (g.slice(0, 7).every((x) => x === 0) && g[7] === 1) return false; // ::1
  if ((g[0] & 0xff00) === 0xff00) return false; // multicast
  if ((g[0] & 0xffc0) === 0xfe80) return false; // link-local
  if ((g[0] & 0xfe00) === 0xfc00) return false; // unique local
  if (g.slice(0, 5).every((x) => x === 0) && g[5] === 0xffff) {
    return isPublicIPv4([g[6] >> 8, g[6] & 255, g[7] >> 8, g[7] & 255]); // IPv4-mapped
  }
  if (g[0] === 0x2001 && g[1] === 0x0db8) return false; // documentation
  return true;
}

export function isPublicIp(ip: string): boolean {
  const v4 = parseIPv4(ip);
  if (v4) return isPublicIPv4(v4);
  const v6 = expandIPv6(ip);
  return v6 ? isPublicIPv6(v6) : false;
}

// --- validation -----------------------------------------------------------------------------------

function str(v: unknown, max: number): string | null {
  return typeof v === 'string' && v.length <= max && !/[\r\n]/.test(v) ? v : null;
}

/** Returns a record, or a short reason it is invalid. Nothing here is trusted until the signature is checked. */
export function parseAnnounce(body: unknown): DirRecord | string {
  if (typeof body !== 'object' || body === null) return 'bad body';
  const b = body as Record<string, unknown>;
  const server_id = str(b.server_id, 43);
  if (!server_id || server_id.length !== 43 || !B64URL.test(server_id)) return 'bad server_id';
  const sig = str(b.sig, 86);
  if (!sig || sig.length !== 86 || !B64URL.test(sig)) return 'bad sig';
  const seq = b.seq;
  if (typeof seq !== 'number' || !Number.isSafeInteger(seq) || seq < 1) return 'bad seq';
  const ip = str(b.ip, 45);
  if (!ip || !isPublicIp(ip)) return 'ip must be a public address';
  const udp_port = b.udp_port;
  if (typeof udp_port !== 'number' || !Number.isInteger(udp_port) || udp_port < 1 || udp_port > 65535) return 'bad udp_port';
  const fingerprint = str(b.fingerprint, 110);
  if (!fingerprint || !FINGERPRINT.test(fingerprint)) return 'bad fingerprint';
  const ws_url = b.ws_url === undefined || b.ws_url === null ? '' : str(b.ws_url, 200);
  if (ws_url === null) return 'bad ws_url';
  if (ws_url) {
    try {
      const u = new URL(ws_url);
      if (u.protocol !== 'https:' || u.username || u.password) return 'ws_url must be a plain https url';
    } catch {
      return 'bad ws_url';
    }
  }
  const claim_hash = b.claim_hash === undefined || b.claim_hash === null ? '' : str(b.claim_hash, 43);
  const claim_proof = b.claim_proof === undefined || b.claim_proof === null ? '' : str(b.claim_proof, 43);
  if (claim_hash === null || claim_proof === null || (claim_hash && !B64URL.test(claim_hash)) || (claim_proof && !B64URL.test(claim_proof))) return 'bad claim';
  if (Boolean(claim_hash) !== Boolean(claim_proof)) return 'claim_hash and claim_proof go together';
  return { server_id, seq, ip, udp_port, fingerprint, ws_url, claim_hash, claim_proof, sig };
}

// --- handlers -------------------------------------------------------------------------------------

export async function srcHash(address: string, salt: string): Promise<string> {
  const d = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(`${salt}|${address}`));
  return b64urlEncode(new Uint8Array(d)).slice(0, 22);
}

/** What the directory stores of a claim secret: its SHA-256, so the secret itself never reaches it. */
export async function claimHash(secret: string): Promise<string> {
  return b64urlEncode(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(secret))));
}

export async function handleAnnounce(db: D1Like, body: unknown, src: string, now: number): Promise<Reply> {
  const r = parseAnnounce(body);
  if (typeof r === 'string') return { status: 400, body: { error: r } };
  if (!(await verifySignature(r))) return { status: 401, body: { error: 'bad signature' } };

  const existing = await db
    .prepare('SELECT seq, updated_at, created_at FROM records WHERE server_id = ?')
    .bind(r.server_id)
    .first<{ seq: number; updated_at: number; created_at: number }>();

  if (existing) {
    if (r.seq <= existing.seq) return { status: 409, body: { error: 'stale seq', seq: existing.seq } };
    if (now - existing.updated_at < MIN_ANNOUNCE_INTERVAL_MS) return { status: 429, body: { error: 'too frequent' } };
  } else {
    const row = await db
      .prepare('SELECT COUNT(*) AS n FROM records WHERE src = ? AND created_at > ?')
      .bind(src, now - 3600_000)
      .first<{ n: number }>();
    if ((row?.n ?? 0) >= NEW_IDS_PER_SRC_PER_HOUR) return { status: 429, body: { error: 'too many new servers from this address' } };
  }

  await db
    .prepare(
      `INSERT INTO records (server_id, seq, ip, udp_port, fingerprint, ws_url, claim_hash, claim_proof, sig, src, created_at, updated_at, claim_expires_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT(server_id) DO UPDATE SET seq = excluded.seq, ip = excluded.ip, udp_port = excluded.udp_port,
         fingerprint = excluded.fingerprint, ws_url = excluded.ws_url, claim_hash = excluded.claim_hash,
         claim_proof = excluded.claim_proof, sig = excluded.sig, updated_at = excluded.updated_at,
         claim_expires_at = excluded.claim_expires_at`
    )
    .bind(r.server_id, r.seq, r.ip, r.udp_port, r.fingerprint, r.ws_url, r.claim_hash, r.claim_proof, r.sig, src, existing?.created_at ?? now, now, r.claim_hash ? now + CLAIM_TTL_MS : 0)
    .run();

  // Housekeeping on writes only, so reads stay cheap: forget servers idle for 30 days, and who set them up.
  // Both statements touch only the expiring rows (records_updated index, owner_servers primary key): never scan owner_servers.
  const cutoff = now - IDLE_EXPIRY_MS;
  await db.batch([
    db.prepare('DELETE FROM owner_servers WHERE server_id IN (SELECT server_id FROM records WHERE updated_at < ?)').bind(cutoff),
    db.prepare('DELETE FROM records WHERE updated_at < ?').bind(cutoff),
  ]);
  return { status: 200, body: { ok: true, seq: r.seq } };
}

const COLUMNS = 'server_id, seq, ip, udp_port, fingerprint, ws_url, claim_hash, claim_proof, sig';

export async function handleLookup(db: D1Like, id: string | null, now: number): Promise<Reply> {
  if (!id || id.length !== 43 || !B64URL.test(id)) return { status: 400, body: { error: 'bad id' } };
  const row = await db
    .prepare(`SELECT ${COLUMNS} FROM records WHERE server_id = ? AND updated_at > ?`)
    .bind(id, now - IDLE_EXPIRY_MS)
    .first<DirRecord>();
  return row ? { status: 200, body: { record: row } } : { status: 404, body: { error: 'unknown server' } };
}

/** First announcement made with the claim secret whose hash the client holds. */
export async function handlePending(db: D1Like, hash: string | null, now: number): Promise<Reply> {
  if (!hash || hash.length !== 43 || !B64URL.test(hash)) return { status: 400, body: { error: 'bad hash' } };
  const row = await db
    .prepare(`SELECT ${COLUMNS} FROM records WHERE claim_hash = ? AND claim_expires_at > ?`)
    .bind(hash, now)
    .first<DirRecord>();
  return row ? { status: 200, body: { record: row } } : { status: 404, body: { error: 'not yet' } };
}
