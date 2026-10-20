import { describe, it, expect, beforeEach } from 'vitest';
import { readFileSync } from 'node:fs';
import { testDb } from '../_testdb';
import {
  b64urlEncode, canonicalMessage, handleAnnounce, handleLookup, handlePending, isPublicIp, parseAnnounce, verifySignature,
  MIN_ANNOUNCE_INTERVAL_MS, NEW_IDS_PER_SRC_PER_HOUR, CLAIM_TTL_MS, IDLE_EXPIRY_MS, type D1Like, type DirRecord,
} from './_lib';

const FP = 'sha-256 ' + Array(32).fill('AB').join(':');

async function newServer() {
  const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
  const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
  return { priv: pair.privateKey, id: b64urlEncode(raw) };
}

async function signed(s: { priv: CryptoKey; id: string }, over: Partial<DirRecord> = {}): Promise<DirRecord> {
  const r: DirRecord = {
    server_id: s.id, seq: 1000, ip: '185.199.108.153', udp_port: 4443, fingerprint: FP,
    ws_url: '', claim_hash: '', claim_proof: '', sig: '', ...over,
  };
  const sig = new Uint8Array(await crypto.subtle.sign({ name: 'Ed25519' }, s.priv, new TextEncoder().encode(canonicalMessage(r))));
  return { ...r, sig: b64urlEncode(sig) };
}

describe('Rust-signed fixture', () => {
  const fixture = JSON.parse(readFileSync(new URL('../../../../server/testdata/announce.json', import.meta.url), 'utf8')) as DirRecord;

  it('verifies with the directory code (same canonical message, key and signature encoding)', async () => {
    expect(parseAnnounce(fixture)).toEqual(fixture);
    expect(await verifySignature(fixture)).toBe(true);
  });

  it('fails when any signed field is altered', async () => {
    for (const change of [{ ip: '185.199.108.154' }, { udp_port: 4444 }, { seq: fixture.seq + 1 }, { ws_url: 'https://evil.example' }, { claim_hash: fixture.claim_hash.slice(1) + 'A' }] as Partial<DirRecord>[]) {
      expect(await verifySignature({ ...fixture, ...change })).toBe(false);
    }
  });
});

describe('announce', () => {
  let db: D1Like;
  beforeEach(() => { db = testDb(); });
  const T0 = 1_800_000_000_000;

  it('accepts a signed record and serves it back unchanged', async () => {
    const s = await newServer();
    const r = await signed(s);
    expect((await handleAnnounce(db, r, 'src1', T0)).status).toBe(200);
    const got = await handleLookup(db, s.id, T0 + 1000);
    expect(got.status).toBe(200);
    expect(got.body.record).toEqual(r);
    expect(await verifySignature(got.body.record as DirRecord)).toBe(true);
  });

  it('rejects a bad signature and a record signed by another key', async () => {
    const a = await newServer(), b = await newServer();
    const forged = { ...(await signed(a)), server_id: b.id };
    expect((await handleAnnounce(db, forged, 's', T0)).status).toBe(401);
    const altered = { ...(await signed(a)), ip: '185.199.108.154' };
    expect((await handleAnnounce(db, altered, 's', T0)).status).toBe(401);
  });

  it('rejects replays and stale sequence numbers', async () => {
    const s = await newServer();
    expect((await handleAnnounce(db, await signed(s, { seq: 5 }), 's', T0)).status).toBe(200);
    expect((await handleAnnounce(db, await signed(s, { seq: 5 }), 's', T0 + 60_000)).status).toBe(409);
    expect((await handleAnnounce(db, await signed(s, { seq: 4 }), 's', T0 + 60_000)).status).toBe(409);
    expect((await handleAnnounce(db, await signed(s, { seq: 6 }), 's', T0 + 60_000)).status).toBe(200);
  });

  it('rate-limits a server that announces too often', async () => {
    const s = await newServer();
    await handleAnnounce(db, await signed(s, { seq: 1 }), 's', T0);
    expect((await handleAnnounce(db, await signed(s, { seq: 2 }), 's', T0 + MIN_ANNOUNCE_INTERVAL_MS - 1)).status).toBe(429);
    expect((await handleAnnounce(db, await signed(s, { seq: 2 }), 's', T0 + MIN_ANNOUNCE_INTERVAL_MS)).status).toBe(200);
  });

  it('limits how many new servers one address can create per hour', async () => {
    for (let i = 0; i < NEW_IDS_PER_SRC_PER_HOUR; i++) {
      expect((await handleAnnounce(db, await signed(await newServer()), 'noisy', T0)).status).toBe(200);
    }
    expect((await handleAnnounce(db, await signed(await newServer()), 'noisy', T0)).status).toBe(429);
    expect((await handleAnnounce(db, await signed(await newServer()), 'other', T0)).status).toBe(200);
    expect((await handleAnnounce(db, await signed(await newServer()), 'noisy', T0 + 3600_001)).status).toBe(200);
  });

  it('refuses non-public addresses, bad ports, bad fingerprints and bad urls', async () => {
    const s = await newServer();
    const bad: Partial<DirRecord>[] = [
      { ip: '10.0.0.1' }, { ip: '127.0.0.1' }, { ip: '169.254.169.254' }, { ip: '100.64.1.1' }, { ip: '::1' }, { ip: 'example.com' },
      { udp_port: 0 }, { udp_port: 70000 }, { fingerprint: 'sha-256 AB' }, { fingerprint: 'sha-1 ' + Array(20).fill('AB').join(':') },
      { ws_url: 'http://x.example' }, { ws_url: 'https://user:pw@x.example' }, { ws_url: 'https://x.example/\nInjected: 1' },
    ];
    for (const over of bad) {
      expect((await handleAnnounce(db, await signed(s, over), 's', T0)).status, JSON.stringify(over)).toBe(400);
    }
  });

  it('accepts public IPv6 and rejects private IPv6', () => {
    expect(isPublicIp('2606:4700:4700::1111')).toBe(true);
    for (const ip of ['fe80::1', 'fd12:3456::1', 'ff02::1', '::', '2001:db8::1', '::ffff:10.0.0.1']) expect(isPublicIp(ip), ip).toBe(false);
    expect(isPublicIp('::ffff:8.8.8.8')).toBe(true);
  });

  it('refuses oversized or malformed bodies', () => {
    expect(parseAnnounce(null)).toBe('bad body');
    expect(parseAnnounce({})).toBe('bad server_id');
  });

  it('forgets servers idle for 30 days', async () => {
    const old = await newServer(), fresh = await newServer();
    await handleAnnounce(db, await signed(old), 's1', T0);
    expect((await handleLookup(db, old.id, T0 + IDLE_EXPIRY_MS + 1)).status).toBe(404);
    await handleAnnounce(db, await signed(fresh), 's2', T0 + IDLE_EXPIRY_MS + 10);
    expect((await handleLookup(db, old.id, T0 + IDLE_EXPIRY_MS + 10)).status).toBe(404);
    expect((await handleLookup(db, fresh.id, T0 + IDLE_EXPIRY_MS + 20)).status).toBe(200);
  });
});

describe('announce clean-up of owner links', () => {
  const T0 = 1_800_000_000_000;
  const link = (db: D1Like, id: string, owner: string) =>
    db.prepare('INSERT INTO owner_servers (server_id, owner, created_at) VALUES (?, ?, ?)').bind(id, owner, T0).run();
  const linked = async (db: D1Like) => (await db.prepare('SELECT server_id FROM owner_servers').bind().all<{ server_id: string }>()).results.map((r) => r.server_id).sort();

  it('drops the links of the records it expires and reads nothing else', async () => {
    const real = testDb();
    const seen: string[] = [];
    const db: D1Like = { prepare: (sql) => (seen.push(sql), real.prepare(sql)), batch: (st) => real.batch(st) };
    const idle = await newServer(), live = await newServer(), trigger = await newServer();
    await handleAnnounce(db, await signed(idle), 's1', T0);
    await handleAnnounce(db, await signed(live), 's2', T0 + IDLE_EXPIRY_MS - 1000);
    await link(db, idle.id, 'github:1');
    await link(db, live.id, 'github:2');
    // A link whose record is not in the table is left alone: the clean-up follows expiring records, it does not scan owner_servers.
    await link(db, 'unrelated', 'github:3');
    seen.length = 0;
    await handleAnnounce(db, await signed(trigger), 's3', T0 + IDLE_EXPIRY_MS + 10);
    const cleanUp = seen.filter((q) => /owner_servers/.test(q));
    expect(await linked(db)).toEqual([live.id, 'unrelated'].sort());
    expect((await handleLookup(db, live.id, T0 + IDLE_EXPIRY_MS + 20)).status).toBe(200);
    expect(cleanUp).toEqual(['DELETE FROM owner_servers WHERE server_id IN (SELECT server_id FROM records WHERE updated_at < ?)']);
  });
});

describe('pending (onboarding)', () => {
  const T0 = 1_800_000_000_000;
  const hash = 'A'.repeat(43);
  const proof = 'B'.repeat(43);

  it('returns the first announcement made with a claim, until it expires', async () => {
    const db = testDb();
    const s = await newServer();
    expect((await handlePending(db, hash, T0)).status).toBe(404);
    await handleAnnounce(db, await signed(s, { claim_hash: hash, claim_proof: proof }), 's', T0);
    const got = await handlePending(db, hash, T0 + 1000);
    expect(got.status).toBe(200);
    expect((got.body.record as DirRecord).server_id).toBe(s.id);
    expect((await handlePending(db, hash, T0 + CLAIM_TTL_MS + 1)).status).toBe(404);
    expect((await handleLookup(db, s.id, T0 + CLAIM_TTL_MS + 1)).status).toBe(200);
  });

  it('does not answer for unknown or malformed hashes', async () => {
    const db = testDb();
    expect((await handlePending(db, 'C'.repeat(43), T0)).status).toBe(404);
    expect((await handlePending(db, 'short', T0)).status).toBe(400);
    expect((await handlePending(db, null, T0)).status).toBe(400);
  });

  it('requires claim hash and proof together', async () => {
    const db = testDb();
    expect((await handleAnnounce(db, await signed(await newServer(), { claim_hash: hash }), 's', T0)).status).toBe(400);
  });
});
