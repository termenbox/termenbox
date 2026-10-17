// Cloudflare Pages Function: POST /api/oauth/exchange { code, device_key } -> { login, cert, exp }
// The GitHub client secret lives only here (Pages secret), never in the browser or on user servers.
// The GitHub token is used once to learn who logged in and is then dropped: the browser gets a device
// certificate instead (DECISION-0062), so no server ever sees a replayable GitHub credential.
// Self-hosters deploy their own copy with their own OAuth app (DECISION-0052).

interface Env {
  GITHUB_CLIENT_ID: string;
  GITHUB_CLIENT_SECRET: string;
  /** base64 PKCS#8 Ed25519 private key; the matching public key is listed in packages/server/cert-keys.txt. */
  CERT_SIGNING_KEY: string;
  /** Names the signing key so servers can trust several during a rotation. */
  CERT_KID?: string;
}

export const CERT_LIFETIME_SECS = 7 * 24 * 3600;


type Context = { request: Request; env: Env };

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' },
  });

const b64url = (bytes: ArrayBuffer | Uint8Array) => {
  const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let bin = '';
  for (const b of arr) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
};

/** An uncompressed SEC1 P-256 point (65 bytes starting with 0x04), as the browser exports it. */
export function validDeviceKey(key: unknown): key is string {
  if (typeof key !== 'string' || key.length !== 87 || !/^[A-Za-z0-9_-]+$/.test(key)) return false;
  const bin = atob(key.replace(/-/g, '+').replace(/_/g, '/') + '=');
  return bin.length === 65 && bin.charCodeAt(0) === 4;
}

// Importing the key costs CPU and the Workers limit is 10 ms: keep it for the life of the isolate.
let cachedKey: { secret: string; key: Promise<CryptoKey> } | null = null;
const signingKey = (secret: string) => {
  if (cachedKey?.secret !== secret) {
    const der = Uint8Array.from(atob(secret), (c) => c.charCodeAt(0));
    cachedKey = { secret, key: crypto.subtle.importKey('pkcs8', der, { name: 'Ed25519' }, false, ['sign']) };
  }
  return cachedKey.key;
};

/** `base64url(payload).base64url(Ed25519 signature of the payload bytes)`, as packages/server/src/cert.rs expects. */
export async function issueCertificate(
  env: Pick<Env, 'CERT_SIGNING_KEY' | 'CERT_KID'>,
  user: { id: number; login: string; provider?: string },
  deviceKey: string,
  nowSecs = Math.floor(Date.now() / 1000),
): Promise<{ cert: string; exp: number }> {
  const exp = nowSecs + CERT_LIFETIME_SECS;
  const payload = new TextEncoder().encode(
    JSON.stringify({
      v: 1,
      kid: env.CERT_KID || 'k1',
      // DECISION-0116: the account is a provider plus a string id. `sub` stays as the numeric
      // GitHub id so the client avatar and servers older than 0116 keep working.
      provider: user.provider || 'github',
      id: String(user.id),
      sub: user.id,
      login: user.login,
      dpk: deviceKey,
      iat: nowSecs,
      exp,
    }),
  );
  const sig = await crypto.subtle.sign({ name: 'Ed25519' }, await signingKey(env.CERT_SIGNING_KEY), payload);
  return { cert: `${b64url(payload)}.${b64url(sig)}`, exp };
}

export const onRequestPost = async ({ request, env }: Context): Promise<Response> => {
  // Same-origin only: the app and this function share a host, so a foreign Origin is never legitimate.
  const origin = request.headers.get('Origin');
  if (origin && origin !== new URL(request.url).origin) return json({ error: 'forbidden' }, 403);

  let code: unknown;
  let deviceKey: unknown;
  try {
    ({ code, device_key: deviceKey } = (await request.json()) as { code?: unknown; device_key?: unknown });
  } catch {
    return json({ error: 'bad request' }, 400);
  }
  if (typeof code !== 'string' || code.length === 0 || code.length > 200) {
    return json({ error: 'bad request' }, 400);
  }
  if (!validDeviceKey(deviceKey)) return json({ error: 'bad request' }, 400);
  if (!env.GITHUB_CLIENT_ID || !env.GITHUB_CLIENT_SECRET || !env.CERT_SIGNING_KEY) return json({ error: 'not configured' }, 500);

  const tokenRes = await fetch('https://github.com/login/oauth/access_token', {
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify({
      client_id: env.GITHUB_CLIENT_ID,
      client_secret: env.GITHUB_CLIENT_SECRET,
      code,
    }),
  });
  const granted = (await tokenRes.json()) as { access_token?: string };
  if (!granted.access_token) return json({ error: 'exchange failed' }, 401);

  const userRes = await fetch('https://api.github.com/user', {
    headers: {
      Authorization: `Bearer ${granted.access_token}`,
      Accept: 'application/vnd.github+json',
      'User-Agent': 'termenbox',
    },
  });
  if (!userRes.ok) return json({ error: 'exchange failed' }, 401);
  const user = (await userRes.json()) as { id: number; login: string };

  // The token goes no further than this function.
  const { cert, exp } = await issueCertificate(env, user, deviceKey);
  return json({ login: user.login, cert, exp });
};
