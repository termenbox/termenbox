import { describe, it, expect } from 'vitest';
import vectors from '../../core/testdata/extension-vectors.json';
import { Negotiated, Registry, getExtension, key, method, putExtension, route, stripAll, stripUnnegotiated, type Meta } from './lib/extensions';

const negotiated = (names: string[]) => new Negotiated(1, new Map(names.map((n) => [n, 1])));

describe('extension registry (shared vectors with packages/core)', () => {
  it('negotiate: the same protocol and versions as Rust', () => {
    for (const c of vectors.negotiate) {
      const us = Object.entries(c.local as Record<string, number>).reduce((r, [n, v]) => r.with(n, v), new Registry());
      expect(us.negotiate(c.remote_meta as Meta | null).toJSON(), c.name).toEqual(c.expect);
    }
  });

  it('route: the same answer as Rust for every method', () => {
    for (const c of vectors.route) {
      const r = route(c.method, c.request, negotiated(c.supports));
      expect(r.kind === 'extension' ? `extension:${r.name}` : r.kind, c.name).toBe(c.expect);
    }
  });

  it('strip: unnegotiated keys and all keys of our namespace go, the rest stays', () => {
    for (const c of vectors.strip) {
      expect(stripUnnegotiated(c.meta as Meta, negotiated(c.negotiated)), c.name).toEqual(c.unnegotiated_stripped);
      expect(stripAll(c.meta as Meta), c.name).toEqual(c.all_stripped);
    }
  });

  it('a negotiated extension round-trips, an unnegotiated one is skipped in both directions', () => {
    const both = new Registry().with('seq', 2).negotiate({ [`termenbox.dev`]: { protocol: 1, extensions: { seq: 2 } } });
    const meta: Meta = {};
    expect(putExtension(meta, 'seq', { n: 9 }, both)).toBe(true);
    expect(JSON.parse(JSON.stringify(meta))).toEqual({ [key('seq')]: { n: 9 } });
    expect(getExtension(meta, 'seq', both)).toEqual({ n: 9 });

    const plain = new Registry().with('seq', 2).negotiate(null);
    expect(putExtension({}, 'seq', { n: 1 }, plain)).toBe(false);
    expect(getExtension({ [key('seq')]: { n: 1 }, [key('hologram')]: 1 }, 'seq', plain)).toBeUndefined();
    expect(route(method('window'), true, both)).toEqual({ kind: 'reject' });
  });
});
