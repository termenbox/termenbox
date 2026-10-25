import { describe, expect, it } from 'vitest';
import vectors from '../../../core/testdata/legacy-vectors.json';
import { fromLegacy, toLegacy } from './legacy';

// LEGACY SHIM test: deleted with the shim in step 8.1.
describe('the legacy shim', () => {
  it('translates every shared vector both ways (the server runs the same ones)', () => {
    for (const p of vectors.client_to_server) {
      expect(toLegacy(p.new), JSON.stringify(p.new)).toEqual(p.old);
      expect(fromLegacy(p.old), JSON.stringify(p.old)).toEqual(p.new);
    }
    for (const p of vectors.server_to_client) {
      expect(fromLegacy(p.old), JSON.stringify(p.old)).toEqual(p.new);
      expect(toLegacy(p.new), JSON.stringify(p.new)).toEqual(p.old);
    }
  });

  it('leaves values, other shapes and the map keyed by workspace ids alone', () => {
    expect(fromLegacy({ type: 'snapshot', session: 's', text: 'session_id' })).toEqual({ type: 'snapshot', workspace: 's', text: 'session_id' });
    expect(fromLegacy(null)).toBeNull();
    expect(fromLegacy([1])).toEqual([1]);
    expect(fromLegacy('x')).toBe('x');
  });
});
