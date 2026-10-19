import { afterEach, describe, expect, it, vi } from 'vitest';
import { accountMessage } from '../../functions/api/me/_lib';
import { deleteMyData, DeleteFailed, registerServer } from './account';
import { b64url } from './device';

async function auth() {
  const pair = (await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify'])) as CryptoKeyPair;
  return { cert: 'the-cert', pair };
}

const stub = (res: Response | Error) => vi.stubGlobal('fetch', vi.fn(async () => { if (res instanceof Error) throw res; return res; }));
afterEach(() => vi.unstubAllGlobals());

describe('deleteMyData', () => {
  it('posts with the certificate and a device signature the directory can verify, and returns the count', async () => {
    stub(new Response(JSON.stringify({ ok: true, servers: 2 })));
    const a = await auth();
    expect(await deleteMyData('https://app.example/', a)).toBe(2);
    const [url, init] = (fetch as ReturnType<typeof vi.fn>).mock.calls[0] as [string, RequestInit];
    const h = init.headers as Record<string, string>;
    expect(url).toBe('https://app.example/api/me/delete');
    expect(h.Authorization).toBe('Bearer the-cert');
    const sig = Uint8Array.from(atob(h['X-Device-Signature'].replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (h['X-Device-Signature'].length % 4)) % 4)), (c) => c.charCodeAt(0));
    expect(await crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, a.pair.publicKey, sig, new TextEncoder().encode(accountMessage('POST /api/me/delete', Number(h['X-Device-Time']))))).toBe(true);
    expect(b64url(sig)).toBe(h['X-Device-Signature']);
  });

  it.each([
    [new Response('{}', { status: 401 }), /sign-in has expired.*Nothing was deleted/],
    [new Response('{}', { status: 500 }), /try again/],
    [new TypeError('fetch failed'), /Could not reach the service.*Nothing was deleted/],
  ])('explains a failure in words: %#', async (res, message) => {
    stub(res);
    const err = await deleteMyData('https://app.example', await auth()).catch((e) => e);
    expect(err).toBeInstanceOf(DeleteFailed);
    expect(err.message).toMatch(message);
  });
});

describe('registerServer', () => {
  it('sends the claim secret in the body and throws when the directory refuses', async () => {
    stub(new Response('{}', { status: 404 }));
    await expect(registerServer('https://app.example', await auth(), 'S'.repeat(43))).rejects.toThrow('404');
    const [, init] = (fetch as ReturnType<typeof vi.fn>).mock.calls[0] as [string, RequestInit];
    expect(JSON.parse(init.body as string)).toEqual({ claim_secret: 'S'.repeat(43) });
  });
});
