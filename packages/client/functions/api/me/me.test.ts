import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { b64urlEncode, canonicalMessage, claimHash, handleAnnounce, handleLookup, IDLE_EXPIRY_MS, type D1Like, type DirRecord } from '../directory/_lib';
import { issueCertificate } from '../oauth/exchange';
import { testDb } from '../testdb';
import { accountMessage, authenticate, handleDeleteOwner, handleRegisterServer, MAX_CLOCK_SKEW_MS } from './_lib';
import { onRequestPost as deleteMe } from './delete';
import { onRequestPost as registerMe } from './servers';

const FP = 'sha-256 ' + Array(32).fill('AB').join(':');
const T0 = 1_800_000_000_000;
const DELETE = 'POST /api/me/delete';
const REGISTER = 'POST /api/me/servers';

const signingPair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
const SIGNING_KEY = btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.exportKey('pkcs8', signingPair.privateKey))));

/** A signed-in browser: a certificate for `id` bound to a fresh device key. */
async function person(id: number, over: { nowSecs?: number } = {}) {
  const pair = (await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign', 'verify'])) as CryptoKeyPair;
  const dpk = b64urlEncode(new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey)));
  const { cert } = await issueCertificate({ CERT_SIGNING_KEY: SIGNING_KEY }, { id, login: `user${id}` }, dpk, over.nowSecs ?? Math.floor(T0 / 1000));
  const sign = async (action: string, ts = T0) =>
    b64urlEncode(new Uint8Array(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, new TextEncoder().encode(accountMessage(action, ts)))));
  return { cert, sign };
}

/** A server that announced itself with `secret`, as a user sees right after the install command. */
async function announced(db: D1Like, secret: string) {
  const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
  const id = b64urlEncode(new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey)));
  const r: DirRecord = { server_id: id, seq: 1000, ip: '185.199.108.153', udp_port: 4443, fingerprint: FP, ws_url: '', claim_hash: await claimHash(secret), claim_proof: 'A'.repeat(43), sig: '' };
  r.sig = b64urlEncode(new Uint8Array(await crypto.subtle.sign({ name: 'Ed25519' }, pair.privateKey, new TextEncoder().encode(canonicalMessage(r)))));
  expect((await handleAnnounce(db, r, 'src', T0)).status).toBe(200);
  return id;
}

const secretOf = (n: number) => String(n).padStart(43, 'S');
const ownerRows = async (db: D1Like, owner: string) =>
  (await db.prepare('SELECT server_id FROM owner_servers WHERE owner = ?').bind(owner).all()).results;

describe('authenticate', () => {
  it('accepts a fresh action signed by the device key named in our certificate', async () => {
    const p = await person(7);
    expect(await authenticate(SIGNING_KEY, p.cert, await p.sign(DELETE), String(T0), DELETE, T0)).toEqual({ owner: { key: 'github:7' } });
  });

  it('refuses missing credentials', async () => {
    const p = await person(7);
    const sig = await p.sign(DELETE);
    for (const args of [[null, sig, String(T0)], [p.cert, null, String(T0)], [p.cert, sig, null]] as const) {
      expect(await authenticate(SIGNING_KEY, args[0], args[1], args[2], DELETE, T0)).toMatchObject({ denied: { status: 401 } });
    }
  });

  it('refuses a certificate we did not sign', async () => {
    const other = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
    const otherKey = btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.exportKey('pkcs8', other.privateKey))));
    const p = await person(7);
    expect(await authenticate(otherKey, p.cert, await p.sign(DELETE), String(T0), DELETE, T0)).toMatchObject({ denied: { status: 401, body: { error: 'bad certificate' } } });
  });

  it('refuses an expired certificate', async () => {
    const p = await person(7, { nowSecs: Math.floor(T0 / 1000) - 8 * 24 * 3600 });
    expect(await authenticate(SIGNING_KEY, p.cert, await p.sign(DELETE), String(T0), DELETE, T0)).toMatchObject({ denied: { body: { error: 'certificate expired' } } });
  });

  it("refuses a signature from someone else's device key, even with a valid certificate", async () => {
    const mine = await person(7), theirs = await person(8);
    expect(await authenticate(SIGNING_KEY, mine.cert, await theirs.sign(DELETE), String(T0), DELETE, T0)).toMatchObject({ denied: { body: { error: 'bad device signature' } } });
  });

  it('refuses a signature made for a different action or moment, and a stale clock', async () => {
    const p = await person(7);
    expect(await authenticate(SIGNING_KEY, p.cert, await p.sign(REGISTER), String(T0), DELETE, T0)).toMatchObject({ denied: { status: 401 } });
    expect(await authenticate(SIGNING_KEY, p.cert, await p.sign(DELETE, T0 - 1), String(T0), DELETE, T0)).toMatchObject({ denied: { status: 401 } });
    const old = T0 - MAX_CLOCK_SKEW_MS - 1;
    expect(await authenticate(SIGNING_KEY, p.cert, await p.sign(DELETE, old), String(old), DELETE, T0)).toMatchObject({ denied: { body: { error: 'stale request' } } });
  });

  it('refuses garbage without throwing', async () => {
    expect(await authenticate(SIGNING_KEY, 'not.a-cert', 'x', String(T0), DELETE, T0)).toMatchObject({ denied: { status: 401 } });
  });
});

describe('delete my data', () => {
  let db: D1Like;
  beforeEach(() => { db = testDb(); });

  it('after deletion the owner’s servers are unknown to lookup and the owner’s list is empty; other owners keep theirs', async () => {
    const mine = [await announced(db, secretOf(1)), await announced(db, secretOf(2))];
    const theirs = await announced(db, secretOf(3));
    for (const [n, who] of [[1, 'github:7'], [2, 'github:7'], [3, 'github:8']] as const) {
      expect((await handleRegisterServer(db, { key: who }, { claim_secret: secretOf(n) }, T0 + 1000)).status).toBe(200);
    }
    expect(await ownerRows(db, 'github:7')).toHaveLength(2);

    const reply = await handleDeleteOwner(db, { key: 'github:7' });
    expect(reply).toEqual({ status: 200, body: { ok: true, servers: 2 } });

    for (const id of mine) expect((await handleLookup(db, id, T0 + 2000)).status).toBe(404);
    expect(await ownerRows(db, 'github:7')).toEqual([]);
    expect((await handleLookup(db, theirs, T0 + 2000)).status).toBe(200);
    expect(await ownerRows(db, 'github:8')).toHaveLength(1);
  });

  it('is safe to repeat and reports zero for an owner with nothing stored', async () => {
    expect(await handleDeleteOwner(db, { key: 'github:9' })).toEqual({ status: 200, body: { ok: true, servers: 0 } });
  });
});

describe('register a server', () => {
  let db: D1Like;
  beforeEach(() => { db = testDb(); });

  it('needs the claim secret of a server that announced with it', async () => {
    await announced(db, secretOf(1));
    expect((await handleRegisterServer(db, { key: 'github:7' }, { claim_secret: secretOf(9) }, T0 + 1000)).status).toBe(404);
    expect((await handleRegisterServer(db, { key: 'github:7' }, { claim_secret: 'short' }, T0 + 1000)).status).toBe(400);
    expect((await handleRegisterServer(db, { key: 'github:7' }, null, T0 + 1000)).status).toBe(400);
    expect(await ownerRows(db, 'github:7')).toEqual([]);
  });

  it('stops working once the claim window has passed', async () => {
    await announced(db, secretOf(1));
    expect((await handleRegisterServer(db, { key: 'github:7' }, { claim_secret: secretOf(1) }, T0 + 16 * 60_000)).status).toBe(404);
  });

  it('moves a server to its new owner when it is claimed again', async () => {
    const id = await announced(db, secretOf(1));
    await handleRegisterServer(db, { key: 'github:7' }, { claim_secret: secretOf(1) }, T0 + 1000);
    await handleRegisterServer(db, { key: 'github:8' }, { claim_secret: secretOf(1) }, T0 + 2000);
    expect(await ownerRows(db, 'github:7')).toEqual([]);
    expect(await ownerRows(db, 'github:8')).toEqual([{ server_id: id }]);
  });

  it('forgets who set a server up when its record expires', async () => {
    const id = await announced(db, secretOf(1));
    await handleRegisterServer(db, { key: 'github:7' }, { claim_secret: secretOf(1) }, T0 + 1000);
    const later = T0 + IDLE_EXPIRY_MS + 1000;
    // Any announce runs the clean-up.
    const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
    const other: DirRecord = { server_id: b64urlEncode(new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey))), seq: 1, ip: '185.199.108.153', udp_port: 4443, fingerprint: FP, ws_url: '', claim_hash: '', claim_proof: '', sig: '' };
    other.sig = b64urlEncode(new Uint8Array(await crypto.subtle.sign({ name: 'Ed25519' }, pair.privateKey, new TextEncoder().encode(canonicalMessage(other)))));
    expect((await handleAnnounce(db, other, 'src2', later)).status).toBe(200);
    expect((await handleLookup(db, id, later)).status).toBe(404);
    expect(await ownerRows(db, 'github:7')).toEqual([]);
  });
});

describe('the endpoints', () => {
  let db: D1Like;
  let logs: string[];
  beforeEach(() => {
    db = testDb();
    logs = [];
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(T0);
    vi.spyOn(console, 'log').mockImplementation((line: string) => void logs.push(line));
  });
  afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

  const env = () => ({ DB: db, CERT_SIGNING_KEY: SIGNING_KEY, DIRECTORY_SALT: 'salt' });
  const req = (path: string, headers: Record<string, string>, body?: unknown) =>
    new Request(`https://termenbox.example${path}`, { method: 'POST', headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const authed = async (p: Awaited<ReturnType<typeof person>>, action: string, extra: Record<string, string> = {}) => ({
    Authorization: `Bearer ${p.cert}`, 'X-Device-Signature': await p.sign(action), 'X-Device-Time': String(T0), ...extra,
  });

  it('link a server, delete the account, and leave an audit trail without secrets', async () => {
    const p = await person(7);
    const id = await announced(db, secretOf(1));
    const linked = await registerMe({ request: req('/api/me/servers', await authed(p, REGISTER), { claim_secret: secretOf(1) }), env: env() });
    expect(linked.status).toBe(200);
    const gone = await deleteMe({ request: req('/api/me/delete', await authed(p, DELETE)), env: env() });
    expect(gone.status).toBe(200);
    expect(await gone.json()).toEqual({ ok: true, servers: 1 });
    expect((await handleLookup(db, id, T0 + 1000)).status).toBe(404);

    const events = logs.map((l) => JSON.parse(l));
    expect(events.map((e) => e.event)).toEqual(['owner.server_registered', 'owner.delete']);
    expect(events[1]).toMatchObject({ level: 'info', action: DELETE, status: 200, servers: 1 });
    const text = logs.join('\n');
    for (const secret of [p.cert, secretOf(1), await p.sign(DELETE), SIGNING_KEY, 'github:7', '"7"']) expect(text).not.toContain(secret);
  });

  it('answer 401 and delete nothing without valid credentials, and log the refusal', async () => {
    const p = await person(7);
    await announced(db, secretOf(1));
    await handleRegisterServer(db, { key: 'github:7' }, { claim_secret: secretOf(1) }, T0);
    for (const headers of [{}, { ...(await authed(p, DELETE)), 'X-Device-Signature': 'AAAA' }, { ...(await authed(p, REGISTER)) }]) {
      const res = await deleteMe({ request: req('/api/me/delete', headers), env: env() });
      expect(res.status).toBe(401);
    }
    expect(await ownerRows(db, 'github:7')).toHaveLength(1);
    expect(logs.map((l) => JSON.parse(l)).every((e) => e.event === 'owner.denied' && e.level === 'warn' && typeof e.reason === 'string')).toBe(true);
  });

  it('refuse a request from another origin', async () => {
    const p = await person(7);
    const res = await deleteMe({ request: req('/api/me/delete', await authed(p, DELETE, { Origin: 'https://evil.example' })), env: env() });
    expect(res.status).toBe(403);
  });
});
