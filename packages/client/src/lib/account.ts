// The account's footprint in the hosted directory (DECISION-0066): linking a new server to its owner, and
// deleting everything stored about the owner. Each request carries the device certificate and a fresh
// signature from the device key.
import { signAccountAction } from './device';

export interface AccountAuth {
  cert: string;
  pair: CryptoKeyPair;
}

async function call(origin: string, auth: AccountAuth, path: string, body?: unknown): Promise<Response> {
  const action = `POST ${path}`;
  const ts = Date.now();
  const text = body === undefined ? '' : JSON.stringify(body);
  return fetch(`${origin.replace(/\/$/, '')}${path}`, {
    method: 'POST',
    cache: 'no-store',
    headers: {
      Authorization: `Bearer ${auth.cert}`,
      'X-Device-Signature': await signAccountAction(auth.pair, action, ts, text),
      'X-Device-Time': String(ts),
      ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
    },
    body: body === undefined ? undefined : text,
  });
}

/** Tells the directory this account set up the server that announced itself with `claimSecret`. */
export async function registerServer(origin: string, auth: AccountAuth, claimSecret: string): Promise<void> {
  const res = await call(origin, auth, '/api/me/servers', { claim_secret: claimSecret });
  if (!res.ok) throw new Error(`directory answered ${res.status}`);
}

/** What went wrong, in words for the person who pressed Delete. */
export class DeleteFailed extends Error {
  /** The sign-in is no longer valid: retrying cannot work, signing in again can. */
  constructor(message: string, readonly expired = false) {
    super(message);
  }
}

export const SIGN_IN_EXPIRED = 'Your sign-in has expired. Sign out, sign in again and retry. Nothing was deleted.';

/** Deletes the account's records. Resolves with how many servers' records were removed. */
export async function deleteMyData(origin: string, auth: AccountAuth): Promise<number> {
  let res: Response;
  try {
    res = await call(origin, auth, '/api/me/delete');
  } catch {
    throw new DeleteFailed('Could not reach the service. Check your connection and try again. Nothing was deleted.');
  }
  if (res.status === 401) throw new DeleteFailed(SIGN_IN_EXPIRED, true);
  if (!res.ok) throw new DeleteFailed('Something went wrong on our side. Wait a moment and try again; deleting twice is harmless.');
  return ((await res.json()) as { servers: number }).servers;
}
