// Device key (DECISION-0062): a non-extractable ECDSA P-256 key pair kept in IndexedDB. The GitHub token is
// never stored; a certificate from the hosted function says this key belongs to a GitHub user, and the
// key signs each server's challenge, so no server can replay it elsewhere.
import { accountMessage } from '../../functions/api/me/_lib';

const ALG = { name: 'ECDSA', namedCurve: 'P-256' } as const;

export interface KeyStore {
  get(): Promise<CryptoKeyPair | undefined>;
  put(pair: CryptoKeyPair): Promise<void>;
}

export function b64url(bytes: ArrayBuffer | Uint8Array): string {
  const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let bin = '';
  for (const b of arr) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

const DB = 'termenbox';
const STORE = 'device';

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export const idbStore: KeyStore = {
  async get() {
    const db = await open();
    return new Promise((resolve, reject) => {
      const req = db.transaction(STORE).objectStore(STORE).get('key');
      req.onsuccess = () => resolve(req.result as CryptoKeyPair | undefined);
      req.onerror = () => reject(req.error);
    });
  },
  async put(pair) {
    const db = await open();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readwrite');
      tx.objectStore(STORE).put(pair, 'key');
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  },
};

let cached: Promise<CryptoKeyPair> | null = null;

/** The device's key pair, created on first use. The private key cannot be exported, even by this page. */
export function deviceKey(store: KeyStore = idbStore): Promise<CryptoKeyPair> {
  if (store === idbStore && cached) return cached;
  const p = (async () => {
    const existing = await store.get();
    if (existing) return existing;
    const pair = await crypto.subtle.generateKey(ALG, false, ['sign', 'verify']);
    await store.put(pair);
    return pair;
  })();
  if (store === idbStore) {
    cached = p;
    p.catch(() => (cached = null));
  }
  return p;
}

/** Uncompressed SEC1 public key, base64url: what the function puts in the certificate. */
export async function publicKeyB64(pair: CryptoKeyPair): Promise<string> {
  return b64url(await crypto.subtle.exportKey('raw', pair.publicKey));
}

/** The exact message the server verifies; binds the signature to one server and one connection. */
export function challengeMessage(serverId: string, nonce: string): Uint8Array<ArrayBuffer> {
  return new TextEncoder().encode(`termenbox-auth-v1\n${serverId}\n${nonce}`);
}

/** Raw r||s signature, base64url. */
export async function signChallenge(pair: CryptoKeyPair, serverId: string, nonce: string): Promise<string> {
  const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, challengeMessage(serverId, nonce));
  return b64url(sig);
}

/** Raw r||s signature over one account action and the moment it is made, base64url (see functions/api/me/_lib.ts). */
export async function signAccountAction(pair: CryptoKeyPair, action: string, ts: number): Promise<string> {
  const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, new TextEncoder().encode(accountMessage(action, ts)));
  return b64url(sig);
}

export interface Credentials {
  cert: string;
  sign: (serverId: string, nonce: string) => Promise<string>;
}

/** The GitHub account id in the certificate (`sub`), used for the avatar URL; 0 when unreadable. */
export function certUserId(cert: string): number {
  try {
    const body = cert.split('.')[0].replace(/-/g, '+').replace(/_/g, '/');
    return Number(JSON.parse(atob(body)).sub) || 0;
  } catch {
    return 0;
  }
}

/** The account's avatar by id: one host, no redirect, and it works without a token. */
export function avatarUrl(id: number, size = 64): string {
  return id > 0 ? `https://avatars.githubusercontent.com/u/${id}?s=${size}&v=4` : '';
}

/** Certificate expiry (unix seconds), read from its payload; the signature is the server's business. */
export function certExpiry(cert: string): number {
  try {
    const body = cert.split('.')[0].replace(/-/g, '+').replace(/_/g, '/');
    return Number(JSON.parse(atob(body)).exp) || 0;
  } catch {
    return 0;
  }
}
