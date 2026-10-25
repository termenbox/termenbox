import { describe, expect, it } from 'vitest';
import vectors from '../../../core/testdata/control-vectors.json';
import { CONTROL_TYPES } from './protocol';

describe('control message types', () => {
  it('are the ones the shared vector lists (the server checks ClientMessage::is_control against it)', () => {
    expect([...CONTROL_TYPES].sort()).toEqual([...new Set(vectors.control.map((m) => m.type))].sort());
    for (const m of vectors.bulk_or_other) expect(CONTROL_TYPES.has(m.type), m.type).toBe(false);
  });
});
