import { describe, expect, it } from 'vitest';
import { issueCertificate, validDeviceKey, CERT_LIFETIME_SECS } from './exchange';

const dec = (s: string) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0));

describe('issueCertificate', () => {
  it('signs (provider, string id, device key, 7-day expiry, kid) so the public key verifies it', async () => {
    const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
    const secret = btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.exportKey('pkcs8', pair.privateKey))));
    const dpk = 'B' + 'A'.repeat(86);
    const { cert, exp } = await issueCertificate({ CERT_SIGNING_KEY: secret, CERT_KID: 'k9' }, { id: 7, login: 'octo' }, dpk, 1000);
    expect(exp).toBe(1000 + CERT_LIFETIME_SECS);
    const [body, sig] = cert.split('.');
    expect(await crypto.subtle.verify({ name: 'Ed25519' }, pair.publicKey, dec(sig), dec(body))).toBe(true);
    expect(JSON.parse(new TextDecoder().decode(dec(body)))).toEqual({
      v: 1,
      kid: 'k9',
      provider: 'github',
      id: '7',
      sub: 7,
      login: 'octo',
      dpk,
      iat: 1000,
      exp,
    });
  });

  it('names the provider and keeps the GitHub-only `sub` off other providers', async () => {
    const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
    const secret = btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.exportKey('pkcs8', pair.privateKey))));
    const { cert } = await issueCertificate({ CERT_SIGNING_KEY: secret }, { id: 7, login: 'octo', provider: 'gitlab' }, 'B' + 'A'.repeat(86), 1000);
    const body = JSON.parse(new TextDecoder().decode(dec(cert.split('.')[0])));
    expect(body.provider).toBe('gitlab');
    expect(body.id).toBe('7');
    expect(body.sub).toBeUndefined();
  });
});

describe('validDeviceKey', () => {
  it('accepts an uncompressed P-256 point and nothing else', async () => {
    const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign']);
    const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
    const key = btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    expect(validDeviceKey(key)).toBe(true);
    expect(validDeviceKey(key.slice(1))).toBe(false);
    expect(validDeviceKey('!'.repeat(87))).toBe(false);
    expect(validDeviceKey(undefined)).toBe(false);
  });
});

describe('POST /api/oauth/exchange', () => {
  it('returns a certificate and login, never the GitHub token', async () => {
    const { onRequestPost } = await import('./exchange');
    const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
    const secret = btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.exportKey('pkcs8', pair.privateKey))));
    const real = globalThis.fetch;
    globalThis.fetch = (async (url: string) =>
      new Response(JSON.stringify(String(url).includes('access_token') ? { access_token: 'gho_SECRET' } : { id: 5, login: 'octo' }))) as typeof fetch;
    try {
      const raw = new Uint8Array(await crypto.subtle.exportKey('raw', ((await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign'])) as CryptoKeyPair).publicKey));
      const device_key = btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
      const res = await onRequestPost({
        request: new Request('https://app.example/api/oauth/exchange', { method: 'POST', body: JSON.stringify({ code: 'c', device_key }) }),
        env: { GITHUB_CLIENT_ID: 'i', GITHUB_CLIENT_SECRET: 's', CERT_SIGNING_KEY: secret },
      });
      const text = await res.text();
      expect(res.status).toBe(200);
      expect(text).not.toContain('gho_SECRET');
      expect(JSON.parse(text)).toMatchObject({ login: 'octo' });
      expect(JSON.parse(text).cert).toContain('.');
      const bad = await onRequestPost({
        request: new Request('https://app.example/api/oauth/exchange', { method: 'POST', body: JSON.stringify({ code: 'c' }) }),
        env: { GITHUB_CLIENT_ID: 'i', GITHUB_CLIENT_SECRET: 's', CERT_SIGNING_KEY: secret },
      });
      expect(bad.status).toBe(400);
    } finally {
      globalThis.fetch = real;
    }
  });
});
