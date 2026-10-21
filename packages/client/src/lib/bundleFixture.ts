// Test helper: a candidate bundle signed with a fresh Ed25519 key, the way a server signs one.
import { b64urlEncode } from '../../functions/api/directory/_lib';
import { signedBytes } from './bundle';
import type { Bundle } from './protocol';

export const FINGERPRINT = `sha-256 ${Array(32).fill('AB').join(':')}`;

export async function newServerKey(): Promise<{ serverId: string; sign: (b: Omit<Bundle, 'server_id'>) => Promise<string> }> {
  const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
  const serverId = b64urlEncode(new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey)));
  return {
    serverId,
    async sign(b) {
      const body = signedBytes({ ...b, server_id: serverId });
      const sig = new Uint8Array(await crypto.subtle.sign({ name: 'Ed25519' }, pair.privateKey, body as BufferSource));
      const out = new Uint8Array(body.length + sig.length);
      out.set(body);
      out.set(sig, body.length);
      return b64urlEncode(out);
    },
  };
}

/** `seq`, `expires` (unix seconds) and the candidates are what tests vary. */
export const bundleOf = (seq: number, candidates: Bundle['candidates'], over: Partial<Bundle> = {}): Omit<Bundle, 'server_id'> => ({
  seq, expires: Math.floor(Date.now() / 1000) + 3600, published: false, fingerprint: FINGERPRINT, name: 'box', candidates, ...over,
});
