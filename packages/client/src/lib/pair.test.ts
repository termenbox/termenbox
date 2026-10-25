import { describe, it, expect } from 'vitest';
import vectors from '../../../core/testdata/bundle-vectors.json';
import { describeCandidates, linksToAccount, mergePaired, pairedServer, parsePairLink, previewPair } from './pair';
import type { ServerListItem } from './protocol';
import { bundleOf, newServerKey } from './bundleFixture';

const CLAIM = 'c'.repeat(43);
const NOW = Math.floor(Date.now() / 1000);

describe('the pairing link', () => {
  it('is read from a fragment, a full URL or a pasted link, with the claim token when there is one', () => {
    expect(parsePairLink('#/pair/AbC-_123')).toEqual({ payload: 'AbC-_123', claim: undefined });
    expect(parsePairLink(`https://app.example/#/pair/AbC?claim=${CLAIM}`)).toEqual({ payload: 'AbC', claim: CLAIM });
    expect(parsePairLink('  https://app.example/#/pair/AbC  ')?.payload).toBe('AbC');
    expect(parsePairLink('#/workspaces')).toBeNull();
    expect(parsePairLink('#claim=' + CLAIM)).toBeNull();
    expect(parsePairLink('')).toBeNull();
  });

  it('previews the name, the fingerprint and how the server can be reached, from the signed link alone', async () => {
    const key = await newServerKey();
    const payload = await key.sign(bundleOf(1, [{ kind: 'lan', addr: '192.168.1.20', port: 4443 }, { kind: 'wan6', addr: '2001:db8::1', port: 4443 }], { name: 'home-box' }));
    const r = await previewPair({ payload, claim: CLAIM }, NOW);
    if (!r.ok) throw new Error('did not verify');
    expect(r.preview).toMatchObject({ name: 'home-box', serverId: key.serverId, claim: CLAIM, published: false });
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

  describe('pairing a server that is already saved', () => {
    const lan = [{ kind: 'lan' as const, addr: '10.0.0.2', port: 4443 }];
    const saved = (id: string): ServerListItem => ({
      server_url: 'wss://t.example/ws', server_id: id, server_name: 'old name', registered_at: 5, winner: 'lan 10.0.0.2:4443', bundle: 'OLD', bundle_seq: 9,
      record: { ip: '8.8.8.8', udp_port: 50000, fingerprint: 'sha-256 AB', ws_url: 'https://ws.example' } as never,
    });
    const paired = async (key: Awaited<ReturnType<typeof newServerKey>>, seq: number, claim?: string) => {
      const payload = await key.sign(bundleOf(seq, lan, { name: 'fresh' }));
      const r = await previewPair({ payload, claim }, NOW);
      if (!r.ok) throw new Error('did not verify');
      return pairedServer(r.preview);
    };

    it('keeps the tunnel URL, the directory record, the winner and a newer stored bundle, and does not let an older link overwrite it', async () => {
      const key = await newServerKey();
      const merged = mergePaired(saved(key.serverId), await paired(key, 1));
      expect(merged).toMatchObject({ server_url: 'wss://t.example/ws', winner: 'lan 10.0.0.2:4443', bundle: 'OLD', bundle_seq: 9, registered_at: 5 });
      expect(merged.record).toBeDefined();
    });

    it('takes a newer bundle from the link and the claim it carries, and the name given on this device', async () => {
      const key = await newServerKey();
      const merged = mergePaired(saved(key.serverId), await paired(key, 12, CLAIM));
      expect(merged).toMatchObject({ bundle_seq: 12, claim: CLAIM, server_name: 'fresh', server_url: 'wss://t.example/ws', winner: 'lan 10.0.0.2:4443' });
      expect(merged.record).toBeDefined();
    });

    it('is the link itself for a server not saved yet', async () => {
      const key = await newServerKey();
      const p = await paired(key, 1);
      expect(mergePaired(undefined, p)).toEqual(p);
    });
  });

  it('links a claimed server to the account when its signed bundle says it publishes to the directory, or it has a record', () => {
    const base: ServerListItem = { server_url: '', server_name: 'x', registered_at: 0 };
    expect(linksToAccount({ ...base, claim: CLAIM, record: {} as never }, true)).toBe(true);
    expect(linksToAccount({ ...base, claim: CLAIM, published: true }, true)).toBe(true);
    expect(linksToAccount({ ...base, claim: CLAIM, published: true }, false)).toBe(false);
    expect(linksToAccount({ ...base, claim: CLAIM, published: false, bundle: 'B' }, true)).toBe(false); // LAN-only: zero directory requests
    expect(linksToAccount({ ...base, claim: CLAIM, bundle: 'B' }, true)).toBe(false);
    expect(linksToAccount({ ...base, published: true }, true)).toBe(false);
  });

  it('a pairing link of a publishing server is saved as one, and re-pairing never turns that off', async () => {
    const key = await newServerKey();
    const lan = [{ kind: 'lan' as const, addr: '10.0.0.2', port: 4443 }];
    const yes = await previewPair({ payload: await key.sign(bundleOf(2, lan, { published: true })), claim: CLAIM }, NOW);
    const no = await previewPair({ payload: await key.sign(bundleOf(3, lan, { published: false })) }, NOW);
    if (!yes.ok || !no.ok) throw new Error('did not verify');
    const publishing = pairedServer(yes.preview);
    expect(publishing.published).toBe(true);
    expect(pairedServer(no.preview).published).toBe(false);
    // The newer signed bundle decides.
    expect(mergePaired(publishing, pairedServer(no.preview)).published).toBe(false);
    expect(mergePaired(pairedServer(no.preview), { ...publishing, bundle_seq: 1 }).published).toBe(false);
  });
});
