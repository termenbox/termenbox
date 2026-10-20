// Client side of the signed directory (DECISION-0056). Records are self-certifying: `server_id` is
// the server's Ed25519 public key, so whatever the directory returns is checked against it.
import { canonicalMessage, claimHash, verifySignature, b64urlEncode, type DirRecord } from '../../functions/api/directory/_lib';

export type { DirRecord };
export { b64urlEncode, claimHash };

export function randomClaimSecret(): string {
  return b64urlEncode(crypto.getRandomValues(new Uint8Array(32)));
}

/** True when the record's proof shows its server knows `secret`, i.e. it is the machine we just set up. */
export async function verifyClaimProof(secret: string, r: DirRecord): Promise<boolean> {
  const key = await crypto.subtle.importKey('raw', new TextEncoder().encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  const msg = `termenbox-claim-v1\n${r.server_id}\n${r.fingerprint}\n${r.ip}:${r.udp_port}`;
  const mac = new Uint8Array(await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(msg)));
  const expected = b64urlEncode(mac);
  return expected.length === r.claim_proof.length && expected === r.claim_proof;
}

async function getRecord(url: string): Promise<DirRecord | null> {
  const res = await fetch(url, { cache: 'no-store' });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`directory answered ${res.status}`);
  return ((await res.json()) as { record: DirRecord }).record;
}

/** Latest record of a pinned server. Throws if the directory returns anything not signed by that key. */
export async function lookup(origin: string, serverId: string): Promise<DirRecord | null> {
  const r = await getRecord(`${origin.replace(/\/$/, '')}/api/directory/lookup?id=${encodeURIComponent(serverId)}`);
  if (!r) return null;
  if (r.server_id !== serverId || !(await verifySignature(r))) throw new Error('directory returned a record that is not signed by the pinned key');
  return r;
}

/** The first record announced with our claim secret, verified both ways: signature and claim proof. */
export async function pending(origin: string, secret: string): Promise<DirRecord | null> {
  const r = await getRecord(`${origin.replace(/\/$/, '')}/api/directory/pending?h=${encodeURIComponent(await claimHash(secret))}`);
  if (!r) return null;
  if (!(await verifySignature(r)) || !(await verifyClaimProof(secret, r))) throw new Error('announcement does not prove knowledge of the claim secret');
  return r;
}

export { canonicalMessage };
