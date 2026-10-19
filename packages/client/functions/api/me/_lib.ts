// Account actions on the directory (DECISION-0066): register a server as yours, and delete everything stored
// about you. Both are authenticated with the device certificate the hosted function issued at login plus a
// signature from the device key it names, so a leaked certificate alone cannot delete anything (DECISION-0062).
import { b64urlDecode, claimHash, srcHash, type D1Like, type Reply } from '../directory/_lib';

export const MAX_CLOCK_SKEW_MS = 5 * 60_000;

/** The exact bytes the device key signs: one action, one moment. Keep in sync with packages/client/src/lib/account.ts. */
export function accountMessage(action: string, ts: number): string {
  return `termenbox-account-v1\n${action}\n${ts}`;
}

export interface Owner {
  /** `<provider>:<account id>`, the key of the owner's rows. */
  key: string;
}

interface CertPayload {
  v?: number;
  provider?: string;
  id?: string;
  sub?: number;
  dpk?: string;
  exp?: number;
}

const ED25519 = { name: 'Ed25519' } as const;
const ECDSA_P256 = { name: 'ECDSA', namedCurve: 'P-256' } as const;

// Importing keys costs CPU and the Workers limit is 10 ms: keep the verifying key for the life of the isolate.
let cachedVerifier: { secret: string; key: Promise<CryptoKey> } | null = null;

/** The public half of the certificate signing key, derived from the secret this function already holds. */
function certVerifier(signingKey: string): Promise<CryptoKey> {
  if (cachedVerifier?.secret !== signingKey) {
    const key = (async () => {
      const priv = await crypto.subtle.importKey('pkcs8', Uint8Array.from(atob(signingKey), (c) => c.charCodeAt(0)), ED25519, true, ['sign']);
      const { x } = await crypto.subtle.exportKey('jwk', priv);
      return crypto.subtle.importKey('raw', b64urlDecode(x ?? ''), ED25519, false, ['verify']);
    })();
    cachedVerifier = { secret: signingKey, key };
    key.catch(() => (cachedVerifier = null));
  }
  return cachedVerifier.key;
}

export type Auth = { owner: Owner } | { denied: Reply };

const deny = (reason: string): Auth => ({ denied: { status: 401, body: { error: reason } } });

/**
 * Checks the certificate (our signature, not expired), then the device signature over `action` and a fresh
 * timestamp. Nothing here is trusted before both pass. `denied.body.error` is a short reason, never a secret.
 */
export async function authenticate(
  signingKey: string,
  cert: string | null,
  deviceSig: string | null,
  ts: string | null,
  action: string,
  now: number,
): Promise<Auth> {
  if (!cert || !deviceSig || !ts) return deny('missing credentials');
  const when = Number(ts);
  if (!Number.isSafeInteger(when) || Math.abs(now - when) > MAX_CLOCK_SKEW_MS) return deny('stale request');
  try {
    const [body, sig] = cert.split('.');
    const payloadBytes = b64urlDecode(body);
    if (!(await crypto.subtle.verify(ED25519, await certVerifier(signingKey), b64urlDecode(sig), payloadBytes))) return deny('bad certificate');
    const p = JSON.parse(new TextDecoder().decode(payloadBytes)) as CertPayload;
    if (p.v !== 1 || typeof p.exp !== 'number' || p.exp * 1000 <= now) return deny('certificate expired');
    const account = p.id || (p.sub !== undefined ? String(p.sub) : '');
    if (!account || typeof p.dpk !== 'string') return deny('bad certificate');
    const dpk = await crypto.subtle.importKey('raw', b64urlDecode(p.dpk), ECDSA_P256, false, ['verify']);
    const signed = await crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, dpk, b64urlDecode(deviceSig), new TextEncoder().encode(accountMessage(action, when)));
    if (!signed) return deny('bad device signature');
    return { owner: { key: `${(p.provider || 'github').toLowerCase()}:${account}` } };
  } catch {
    return deny('bad credentials');
  }
}

/** Links a server that has just been set up to its owner. The claim secret proves the caller is whoever installed it. */
export async function handleRegisterServer(db: D1Like, owner: Owner, body: unknown, now: number): Promise<Reply> {
  const secret = (body as { claim_secret?: unknown } | null)?.claim_secret;
  if (typeof secret !== 'string' || !/^[A-Za-z0-9_-]{43}$/.test(secret)) return { status: 400, body: { error: 'bad claim secret' } };
  const row = await db
    .prepare('SELECT server_id FROM records WHERE claim_hash = ? AND claim_expires_at > ?')
    .bind(await claimHash(secret), now)
    .first<{ server_id: string }>();
  if (!row) return { status: 404, body: { error: 'no such claim' } };
  await db
    .prepare('INSERT INTO owner_servers (server_id, owner, created_at) VALUES (?, ?, ?) ON CONFLICT(server_id) DO UPDATE SET owner = excluded.owner')
    .bind(row.server_id, owner.key, now)
    .run();
  return { status: 200, body: { ok: true, server_id: row.server_id } };
}

/** Removes the records of every server this owner set up, then the links themselves. Safe to repeat. */
export async function handleDeleteOwner(db: D1Like, owner: Owner): Promise<Reply> {
  const { results } = await db.prepare('SELECT server_id FROM owner_servers WHERE owner = ?').bind(owner.key).all<{ server_id: string }>();
  await db.prepare('DELETE FROM records WHERE server_id IN (SELECT server_id FROM owner_servers WHERE owner = ?)').bind(owner.key).run();
  await db.prepare('DELETE FROM owner_servers WHERE owner = ?').bind(owner.key).run();
  return { status: 200, body: { ok: true, servers: results.length } };
}

export interface AccountEnv {
  DB: D1Like;
  CERT_SIGNING_KEY: string;
  DIRECTORY_SALT?: string;
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' } });

/**
 * Runs one authenticated account action and writes its audit line: when, which action, a salted reference to the
 * owner, the outcome. Never the certificate, a signature, a claim secret or the account id itself.
 */
export async function serveAccountAction(
  request: Request,
  env: AccountEnv,
  action: string,
  event: string,
  run: (owner: Owner, body: unknown) => Promise<Reply>,
): Promise<Response> {
  // Same-origin only: the app and this function share a host, so a foreign Origin is never legitimate.
  const origin = request.headers.get('Origin');
  if (origin && origin !== new URL(request.url).origin) return json({ error: 'forbidden' }, 403);
  if (!env.CERT_SIGNING_KEY) return json({ error: 'not configured' }, 500);
  const text = await request.text();
  if (text.length > 500) return json({ error: 'too large' }, 413);
  let body: unknown = null;
  if (text) {
    try {
      body = JSON.parse(text);
    } catch {
      return json({ error: 'bad json' }, 400);
    }
  }
  const now = Date.now();
  const bearer = /^Bearer (\S+)$/.exec(request.headers.get('Authorization') ?? '');
  const auth = await authenticate(env.CERT_SIGNING_KEY, bearer?.[1] ?? null, request.headers.get('X-Device-Signature'), request.headers.get('X-Device-Time'), action, now);
  const salt = env.DIRECTORY_SALT ?? '';
  const audit = async (event: string, owner: Owner | null, result: Record<string, unknown>) =>
    console.log(JSON.stringify({ ts: new Date(now).toISOString(), level: event === 'owner.denied' ? 'warn' : 'info', event, action, owner_ref: owner ? await srcHash(owner.key, salt) : undefined, ...result }));
  if ('denied' in auth) {
    await audit('owner.denied', null, { status: auth.denied.status, reason: auth.denied.body.error });
    return json(auth.denied.body, auth.denied.status);
  }
  const reply = await run(auth.owner, body);
  await audit(event, auth.owner, { status: reply.status, ...reply.body });
  return json(reply.body, reply.status);
}
