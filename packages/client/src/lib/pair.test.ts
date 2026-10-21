import { describe, it, expect } from 'vitest';
import vectors from '../../../core/testdata/bundle-vectors.json';
import { describeCandidates, pairedServer, parsePairLink, previewPair } from './pair';
import { bundleOf, newServerKey } from './bundleFixture';

const CLAIM = 'c'.repeat(43);
const NOW = Math.floor(Date.now() / 1000);

describe('the pairing link', () => {
  it('is read from a fragment, a full URL or a pasted link, with the claim token when there is one', () => {
    expect(parsePairLink('#/pair/AbC-_123')).toEqual({ payload: 'AbC-_123', claim: undefined });
    expect(parsePairLink(`https://app.example/#/pair/AbC?claim=${CLAIM}`)).toEqual({ payload: 'AbC', claim: CLAIM });
    expect(parsePairLink('  https://app.example/#/pair/AbC  ')?.payload).toBe('AbC');
    expect(parsePairLink('#/sessions')).toBeNull();
    expect(parsePairLink('#claim=' + CLAIM)).toBeNull();
    expect(parsePairLink('')).toBeNull();
  });

  it('previews the name, the fingerprint and how the server can be reached, from the signed link alone', async () => {
    const key = await newServerKey();
    const payload = await key.sign(bundleOf(1, [{ kind: 'lan', addr: '192.168.1.20', port: 4443 }, { kind: 'wan6', addr: '2001:db8::1', port: 4443 }], { name: 'home-box' }));
    const r = await previewPair({ payload, claim: CLAIM }, NOW);
    if (!r.ok) throw new Error('did not verify');
    expect(r.preview).toMatchObject({ name: 'home-box', serverId: key.serverId, claim: CLAIM });
    expect(r.preview.fingerprint).toMatch(/^sha-256 /);
    expect(describeCandidates(r.preview.candidates)).toBe('LAN, internet (IPv6)');
    // Confirming pins the key from the link and keeps the bundle as the hint to race.
    expect(pairedServer(r.preview, 'mine')).toMatchObject({ server_id: key.serverId, server_name: 'mine', claim: CLAIM, bundle: payload, bundle_seq: 1, server_url: '' });
  });

  it('e2e_corrupted_sig_never_previews (unit level): a corrupted signature or field is "invalid", never a preview', async () => {
    const key = await newServerKey();
    const payload = await key.sign(bundleOf(1, [{ kind: 'lan', addr: '192.168.1.20', port: 4443 }]));
    for (const bad of [payload.slice(0, -3) + (payload.endsWith('AAA') ? 'BBB' : 'AAA'), payload.slice(0, 40) + (payload[40] === 'A' ? 'B' : 'A') + payload.slice(41), payload.slice(0, -10), '']) {
      expect(await previewPair({ payload: bad }, NOW), bad).toEqual({ ok: false, reason: 'invalid' });
    }
  });

  it('refuses an expired link with its own reason, and accepts the Rust-signed vector', async () => {
    const key = await newServerKey();
    const old = await key.sign(bundleOf(1, [{ kind: 'lan', addr: '10.0.0.2', port: 4443 }], { expires: NOW - 10 }));
    expect(await previewPair({ payload: old }, NOW)).toEqual({ ok: false, reason: 'expired' });
    const r = await previewPair({ payload: vectors.wire }, 1_800_000_000);
    expect(r.ok && r.preview.name).toBe('home-box');
  });
});
