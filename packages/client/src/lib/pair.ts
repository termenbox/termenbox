// The pairing link (DECISION-0123): `https://<client>/#/pair/<payload>`, printed by `termenbox-server --print-pairing`
// and drawn as a QR. The payload is a signed candidate bundle; the fragment never reaches a host. It is verified and
// previewed (name and fingerprint, decision 0117) before anything connects (GUARDRAIL-connection-hints-are-signed-and-
// never-identity). Trust in a first-contact link comes from the channel it arrived on: its signature proves only that
// whoever made it holds the key it names.
import { BundleRejected, verifyBundle } from './bundle';
import type { Candidate, ServerListItem } from './protocol';

export interface PairLink {
  payload: string;
  /** The one-time claim token of an unclaimed server; outside the signed part, trusted like a claim link. */
  claim?: string;
}

const PAIR = /#\/pair\/([A-Za-z0-9_-]+)(?:\?claim=([A-Za-z0-9_-]{43}))?(?![A-Za-z0-9_-])/;

/** The pairing link in a URL fragment or a pasted link, or null when there is none. */
export function parsePairLink(text: string): PairLink | null {
  const m = PAIR.exec(text.trim());
  return m ? { payload: m[1], claim: m[2] } : null;
}

export interface PairPreview {
  name: string;
  fingerprint: string;
  serverId: string;
  seq: number;
  candidates: Candidate[];
  payload: string;
  claim?: string;
}

export type PairResult = { ok: true; preview: PairPreview } | { ok: false; reason: 'invalid' | 'expired' };

/** Verifies the link. `invalid` for anything that does not verify; nothing is previewed in that case. */
export async function previewPair(link: PairLink, nowSecs = Math.floor(Date.now() / 1000)): Promise<PairResult> {
  try {
    const b = await verifyBundle(link.payload, { nowSecs });
    return { ok: true, preview: { name: b.name, fingerprint: b.fingerprint, serverId: b.server_id, seq: b.seq, candidates: b.candidates, payload: link.payload, claim: link.claim } };
  } catch (e) {
    return { ok: false, reason: e instanceof BundleRejected && e.reason === 'expired' ? 'expired' : 'invalid' };
  }
}

/** The saved server for a confirmed preview: pinned by the key in the link, reached over the bundle's candidates. */
export function pairedServer(p: PairPreview, name = p.name): ServerListItem {
  return { server_url: '', server_id: p.serverId, server_name: name, registered_at: Date.now(), claim: p.claim, bundle: p.payload, bundle_seq: p.seq };
}

/**
 * The list entry for a pairing link when the server may already be saved: what this device learned about it (tunnel URL,
 * directory record, the path that won, a newer bundle) is kept, and the link adds its claim and a bundle only if it is
 * newer than the stored one.
 */
export function mergePaired(existing: ServerListItem | undefined, paired: ServerListItem): ServerListItem {
  if (!existing) return paired;
  const newer = (paired.bundle_seq ?? 0) > (existing.bundle_seq ?? 0);
  return {
    ...existing,
    server_name: paired.server_name || existing.server_name,
    claim: paired.claim ?? existing.claim,
    bundle: newer ? paired.bundle : existing.bundle,
    bundle_seq: newer ? paired.bundle_seq : existing.bundle_seq,
  };
}

/**
 * Whether to tell the directory this account set the server up: it has a claim and a directory record. A server
 * paired by link has no record, and a LAN-only one must cause no directory request at all
 * (GUARDRAIL-lan-addresses-never-reach-the-directory), so a pairing alone never links.
 */
export function linksToAccount(server: ServerListItem, signedIn: boolean): boolean {
  return signedIn && !!server.claim && !!server.record;
}

const KIND_WORD: Record<string, string> = { lan: 'LAN', wan4: 'internet (IPv4)', wan6: 'internet (IPv6)', overlay: 'overlay network', tunnel: 'tunnel' };

/** `LAN, internet (IPv4)`: which ways the link says the server can be reached, without showing addresses. */
export function describeCandidates(list: Candidate[]): string {
  const kinds = [...new Set(list.map((c) => KIND_WORD[c.kind] ?? c.kind))];
  return kinds.length ? kinds.join(', ') : 'none';
}
