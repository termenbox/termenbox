import { describe, it, expect } from 'vitest';
import vectors from '../../../core/testdata/bundle-vectors.json';
import { BundleRejected, decodeBundle, signedBytes, verifyBundle } from './bundle';
import { b64urlDecode, b64urlEncode } from '../../functions/api/directory/_lib';
import type { Bundle } from './protocol';

const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
const NOW = 1_800_000_000;
const reason = async (p: Promise<unknown>) => (await p.then(() => 'ok', (e) => (e instanceof BundleRejected ? e.reason : String(e))));

/** A bundle this side signs itself, as the test of the signer-independent layout. */
async function sign(bundle: Bundle): Promise<string> {
  const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as CryptoKeyPair;
  const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
  const b = { ...bundle, server_id: b64urlEncode(raw) };
  const body = signedBytes(b);
  const sig = new Uint8Array(await crypto.subtle.sign({ name: 'Ed25519' }, pair.privateKey, body as BufferSource));
  const out = new Uint8Array(body.length + sig.length);
  out.set(body);
  out.set(sig, body.length);
  return b64urlEncode(out);
}

describe('candidate bundle (shared vector with packages/core)', () => {
  it('bundle_signature_verifies: the Rust-signed vector decodes to the same fields, bytes and verifies', async () => {
    const { bundle } = decodeBundle(vectors.wire);
    expect(bundle).toEqual(vectors.bundle);
    expect(hex(signedBytes(bundle))).toBe(vectors.signed_hex);
    expect(await verifyBundle(vectors.wire, { nowSecs: NOW, pin: vectors.server_id })).toEqual(vectors.bundle);
  });

  it('tampered_bundle_fails: flipping any bit of the signed vector fails', async () => {
    const bytes = b64urlDecode(vectors.wire);
    for (let i = 0; i < bytes.length * 8; i++) {
      const t = Uint8Array.from(bytes);
      t[i >> 3] ^= 1 << (i & 7);
      expect(await reason(verifyBundle(b64urlEncode(t), { nowSecs: NOW })), `bit ${i}`).not.toBe('ok');
    }
  });

  it('stale_seq_rejected: a bundle at or below the stored seq is ignored for a known server', async () => {
    const seq = vectors.bundle.seq;
    expect(await reason(verifyBundle(vectors.wire, { nowSecs: NOW, storedSeq: seq }))).toBe('stale');
    expect(await reason(verifyBundle(vectors.wire, { nowSecs: NOW, storedSeq: seq + 10 }))).toBe('stale');
    expect(await reason(verifyBundle(vectors.wire, { nowSecs: NOW, storedSeq: seq - 1 }))).toBe('ok');
  });

  it('refuses a bundle for another server, an expired one (unless kept as a hint) and garbage', async () => {
    expect(await reason(verifyBundle(vectors.wire, { nowSecs: NOW, pin: 'A'.repeat(43) }))).toBe('wrong_server');
    expect(await reason(verifyBundle(vectors.wire, { nowSecs: 1_900_000_000 }))).toBe('expired');
    expect(await reason(verifyBundle(vectors.wire, { nowSecs: 1_900_000_000, allowExpired: true }))).toBe('ok');
    for (const junk of ['', 'AAAA', 'not base64 !', 'x'.repeat(400), vectors.wire.slice(0, -4)]) {
      expect(await reason(verifyBundle(junk, { nowSecs: NOW })), junk).toBe('malformed');
    }
  });

  it('a bundle signed in the browser verifies and carries every candidate kind', async () => {
    const wire = await sign({
      server_id: '', seq: 3, expires: NOW + 100, published: false, fingerprint: vectors.bundle.fingerprint, name: 'häus ✓',
      candidates: [
        { kind: 'lan', addr: '192.168.1.2', port: 4443 }, { kind: 'wan4', addr: '8.8.8.8', port: 50000 },
        { kind: 'wan6', addr: '2001:db8::1', port: 4443 }, { kind: 'overlay', addr: '100.64.1.1', port: 4443 },
        { kind: 'tunnel', addr: 'wss://t.example/ws', port: 443 },
      ],
    });
    const b = await verifyBundle(wire, { nowSecs: NOW });
    expect(b.name).toBe('häus ✓');
    expect(b.candidates.map((c) => c.kind)).toEqual(['lan', 'wan4', 'wan6', 'overlay', 'tunnel']);
  });
});

describe('candidate addresses are checked by kind before anything uses them', () => {
  const lan = (addr: string, kind: Bundle['candidates'][number]['kind'] = 'lan') => ({ kind, addr, port: 4443 });
  const withCandidate = (c: Bundle['candidates'][number]) => sign({ server_id: '', seq: 1, expires: NOW + 100, published: false, fingerprint: vectors.bundle.fingerprint, name: 'x', candidates: [c] });

  it('rejects a line break, a hostname or a stray URL where an IP address belongs', async () => {
    for (const c of [
      lan('10.0.0.2\r\na=evil'), lan('10.0.0.2 extra'), lan('example.com'), lan('http://10.0.0.2'), lan('999.1.1.1'), lan(''),
      lan('8.8.8.8\n', 'wan4'), lan('2001:db8::1', 'wan4'), lan('8.8.8.8', 'wan6'), lan('fe80::1%eth0', 'wan6'), lan('internal-host', 'overlay'),
    ]) {
      expect(await reason(verifyBundle(await withCandidate(c), { nowSecs: NOW, allowExpired: true })), JSON.stringify(c.addr)).toBe('malformed');
    }
  });

  it('rejects a tunnel that is not a wss URL', async () => {
    for (const addr of ['ws://internal-host/', 'https://t.example/ws', 'wss://', 'wss://t.example/ws\r\nx', 'wss://user:pw@t.example/', 'javascript:alert(1)', 'internal-host']) {
      expect(await reason(verifyBundle(await withCandidate(lan(addr, 'tunnel')), { nowSecs: NOW, allowExpired: true })), addr).toBe('malformed');
    }
  });

  it('accepts what a server really advertises', async () => {
    for (const c of [lan('192.168.1.20'), lan('8.8.8.8', 'wan4'), lan('2001:4860::9', 'wan6'), lan('100.64.1.2', 'overlay'), lan('fd7a:115c::1', 'overlay'), lan('wss://t.example/ws', 'tunnel')]) {
      expect(await reason(verifyBundle(await withCandidate(c), { nowSecs: NOW, allowExpired: true })), c.addr).toBe('ok');
    }
  });
});

describe('the published flag (tbxb2)', () => {
  const base = (published: boolean): Bundle => ({ server_id: '', seq: 1, expires: NOW + 100, published, fingerprint: vectors.bundle.fingerprint, name: 'x', candidates: [{ kind: 'lan', addr: '10.0.0.2', port: 4443 }] });
  const FLAGS_AT = 5 + 32 + 8 + 8; // magic, server_id, seq, expires

  it('is read back as signed: a server that publishes says so, one that does not says so', async () => {
    for (const published of [true, false]) {
      const { bundle } = decodeBundle(await sign(base(published)));
      expect(bundle.published).toBe(published);
    }
  });

  it('cannot be switched by anyone but the server: flipping the flag byte breaks the signature', async () => {
    const wire = await sign(base(false));
    const bytes = b64urlDecode(wire);
    expect(bytes[FLAGS_AT]).toBe(0);
    bytes[FLAGS_AT] = 1;
    expect(await reason(verifyBundle(b64urlEncode(bytes), { nowSecs: NOW }))).toBe('bad_signature');
  });

  it('refuses flag bits it does not know, instead of ignoring them', async () => {
    const bytes = b64urlDecode(await sign(base(false)));
    bytes[FLAGS_AT] = 0b10;
    expect(await reason(verifyBundle(b64urlEncode(bytes), { nowSecs: NOW }))).toBe('malformed');
  });

  it('the shared vector is a publishing server', () => {
    expect(vectors.bundle.published).toBe(true);
    expect(decodeBundle(vectors.wire).bundle.published).toBe(true);
  });
});
