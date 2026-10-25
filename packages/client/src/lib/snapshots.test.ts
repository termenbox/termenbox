import { describe, expect, it } from 'vitest';
import { SNAPSHOT_CAP, SnapshotStore } from './snapshots';

describe('peeked screens', () => {
  it('keeps the newest ones up to the cap and drops the oldest', () => {
    const s = new SnapshotStore();
    for (let i = 0; i < SNAPSHOT_CAP + 5; i++) s.put(`k${i}`, `t${i}`, `h${i}`);
    expect(s.size).toBe(SNAPSHOT_CAP);
    expect(s.text('k0')).toBeNull();
    expect(s.text(`k${SNAPSHOT_CAP + 4}`)).toBe(`t${SNAPSHOT_CAP + 4}`);
  });
  it('lets the server say "unchanged" and refreshes the use of the entry', () => {
    const s = new SnapshotStore();
    s.apply('a', { text: 'screen', hash: 'h1' });
    expect(s.hash('a')).toBe('h1');
    s.apply('a', { text: '', hash: 'h1', unchanged: true });
    expect(s.text('a')).toBe('screen');
  });
  it('treats an empty snapshot as "no preview for this workspace"', () => {
    const s = new SnapshotStore();
    s.apply('a', { text: 'screen', hash: 'h1' });
    s.apply('a', { text: '', hash: '' });
    expect(s.text('a')).toBeNull();
    expect(s.hash('a')).toBeUndefined();
  });
});
