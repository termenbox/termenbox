import { beforeEach, describe, expect, it, vi } from 'vitest';
import { applyOrder, dropIndex, forgetOrders, loadOrder, moveWorkspace, movedText, orderWorkspaces, placementOf, reorder, saveOrder } from './order';
import { dropTarget, edgeScroll } from './dragsort';

const w = (serverId: string, id: string) => ({ serverId, key: `${serverId}/${id}`, name: id });
const keys = (list: { key: string }[]) => list.map((x) => x.key);

class Mem implements Pick<Storage, 'getItem' | 'setItem' | 'removeItem'> {
  data = new Map<string, string>();
  getItem(k: string) { return this.data.get(k) ?? null; }
  setItem(k: string, v: string) { this.data.set(k, v); }
  removeItem(k: string) { this.data.delete(k); }
}

describe('reorder', () => {
  const k = ['a', 'b', 'c', 'd'];
  it('moves one step, to the top and to the bottom', () => {
    expect(reorder(k, 'c', 'up')).toEqual(['a', 'c', 'b', 'd']);
    expect(reorder(k, 'b', 'down')).toEqual(['a', 'c', 'b', 'd']);
    expect(reorder(k, 'c', 'top')).toEqual(['c', 'a', 'b', 'd']);
    expect(reorder(k, 'b', 'bottom')).toEqual(['a', 'c', 'd', 'b']);
  });
  it('places before or after another, up and down the list', () => {
    expect(reorder(k, 'a', { after: 'c' })).toEqual(['b', 'c', 'a', 'd']);
    expect(reorder(k, 'd', { before: 'b' })).toEqual(['a', 'd', 'b', 'c']);
    expect(reorder(k, 'a', { before: 'b' })).toBe(k); // already there
  });
  it('returns the same array when nothing moves or the key or target is unknown', () => {
    expect(reorder(k, 'a', 'up')).toBe(k);
    expect(reorder(k, 'd', 'down')).toBe(k);
    expect(reorder(k, 'zzz', 'top')).toBe(k);
    expect(reorder(k, 'a', { before: 'zzz' })).toBe(k);
    expect(reorder(k, 'a', { before: 'a' })).toBe(k);
  });
});

describe('applyOrder', () => {
  const list = [w('s1', 'one'), w('s1', 'two'), w('s1', 'three'), w('s2', 'x'), w('s2', 'y')];
  it('follows the saved order per server and appends a new workspace at the end', () => {
    const saved: Record<string, string[]> = { s1: ['s1/three', 's1/one'], s2: ['s2/y', 's2/x'] };
    expect(keys(applyOrder(list, (id) => saved[id] ?? []))).toEqual(['s1/three', 's1/one', 's1/two', 's2/y', 's2/x']);
  });
  it('ignores a saved id that is gone and keeps the given order without a saved one', () => {
    expect(keys(applyOrder(list, (id) => (id === 's1' ? ['s1/gone', 's1/two'] : [])))).toEqual(['s1/two', 's1/one', 's1/three', 's2/x', 's2/y']);
  });
  it('keeps servers in the order they appear and never mixes their workspaces', () => {
    const mixed = [w('s2', 'x'), w('s1', 'one'), w('s2', 'y')];
    expect(keys(applyOrder(mixed, () => []))).toEqual(['s2/x', 's2/y', 's1/one']);
  });
});

describe('the order the server sent', () => {
  it('wins over the device order when every workspace has one', () => {
    const server = (key: string, order: number) => ({ key, serverId: 's1', name: key, order });
    expect(keys(orderWorkspaces([server('s1/c', 2), server('s1/a', 0), server('s1/b', 1)]))).toEqual(['s1/a', 's1/b', 's1/c']);
    // A server that predates order keeps the device-local one.
    expect(keys(orderWorkspaces([w('s1', 'c'), w('s1', 'a')]))).toEqual(['s1/c', 's1/a']);
  });
});

describe('the order kept on the device', () => {
  it('reads and writes per server, survives garbage and a storage that throws', () => {
    const mem = new Mem();
    saveOrder('s1', ['a', 'b'], mem);
    expect(loadOrder('s1', mem)).toEqual(['a', 'b']);
    expect(loadOrder('s2', mem)).toEqual([]);
    mem.setItem('termenbox_workspace_order:s3', '{not json');
    expect(loadOrder('s3', mem)).toEqual([]);
    mem.setItem('termenbox_workspace_order:s4', JSON.stringify(['a', 3, 'a', null, 'b']));
    expect(loadOrder('s4', mem)).toEqual(['a', 'b']);
    const broken = { getItem() { throw new Error('denied'); }, setItem() { throw new Error('denied'); }, removeItem() { throw new Error('denied'); } };
    expect(loadOrder('s1', broken)).toEqual([]);
    expect(() => saveOrder('s1', ['a'], broken)).not.toThrow();
    saveOrder('s1', [], mem);
    expect(mem.data.has('termenbox_workspace_order:s1')).toBe(false);
  });
});

describe('moving a workspace', () => {
  beforeEach(() => { forgetOrders(); vi.stubGlobal('localStorage', new Mem()); });
  const list = () => orderWorkspaces([w('s1', 'one'), w('s1', 'two'), w('s1', 'three'), w('s2', 'x'), w('s2', 'y')]);

  it('moves inside its own server, keeps the result on the device and reports the place for the screen reader', () => {
    const moved = moveWorkspace(list(), 's1/three', 'up');
    expect(moved).toEqual({ position: 2, count: 3 });
    expect(movedText('three', moved!)).toBe('three moved to position 2 of 3');
    expect(keys(list())).toEqual(['s1/one', 's1/three', 's1/two', 's2/x', 's2/y']);
    forgetOrders(); // a reload
    expect(keys(list())).toEqual(['s1/one', 's1/three', 's1/two', 's2/x', 's2/y']);
    expect(JSON.parse(localStorage.getItem('termenbox_workspace_order:s1')!)).toEqual(['s1/one', 's1/three', 's1/two']);
    expect(localStorage.getItem('termenbox_workspace_order:s2')).toBeNull();
  });
  it('says nothing moved at the ends', () => {
    expect(moveWorkspace(list(), 's1/one', 'up')).toBeNull();
    expect(moveWorkspace(list(), 's2/y', 'bottom')).toBeNull();
    expect(moveWorkspace(list(), 'nope', 'top')).toBeNull();
  });
  it('appends a workspace that appears later and prunes one that went away', () => {
    moveWorkspace(list(), 's1/three', 'top');
    const later = orderWorkspaces([w('s1', 'two'), w('s1', 'three'), w('s1', 'one'), w('s1', 'fresh')]);
    expect(keys(later)).toEqual(['s1/three', 's1/one', 's1/two', 's1/fresh']);
    orderWorkspaces([w('s1', 'three'), w('s1', 'fresh')]);
    expect(JSON.parse(localStorage.getItem('termenbox_workspace_order:s1')!)).toEqual(['s1/three']);
  });
  it('does not forget a server that is not listed right now', () => {
    moveWorkspace(list(), 's2/y', 'top');
    orderWorkspaces([w('s1', 'one')]); // s2 is offline
    expect(keys(list())).toEqual(['s1/one', 's1/two', 's1/three', 's2/y', 's2/x']);
  });
  it('places a dragged workspace beside another and finds its place', () => {
    moveWorkspace(list(), 's1/one', { after: 's1/three' });
    expect(keys(list()).slice(0, 3)).toEqual(['s1/two', 's1/three', 's1/one']);
    expect(placementOf(list(), 's1/one')).toEqual({ position: 3, count: 3 });
    expect(placementOf(list(), 's2/x')).toEqual({ position: 1, count: 2 });
    expect(placementOf(list(), 'nope')).toBeNull();
  });
});

describe('dragging by the handle', () => {
  const rows = [{ key: 'a', top: 0, bottom: 100 }, { key: 'b', top: 100, bottom: 200 }, { key: 'c', top: 200, bottom: 300 }];
  it('drops before the first row whose middle is below the pointer, else after the last', () => {
    expect(dropTarget(rows, 20, 'c')).toEqual({ key: 'a', side: 'before' });
    expect(dropTarget(rows, 160, 'a')).toEqual({ key: 'c', side: 'before' });
    expect(dropTarget(rows, 140, 'a')).toEqual({ key: 'b', side: 'before' });
    expect(dropTarget(rows, 280, 'a')).toEqual({ key: 'c', side: 'after' });
    expect(dropTarget(rows, 500, 'a')).toEqual({ key: 'c', side: 'after' });
    expect(dropTarget([rows[0]], 10, 'a')).toBeNull();
  });
  it('scrolls only near the edges, faster the closer to them', () => {
    expect(edgeScroll(300, 100, 500)).toBe(0);
    expect(edgeScroll(100, 100, 500)).toBe(-18);
    expect(edgeScroll(130, 100, 500)).toBeGreaterThan(-18);
    expect(edgeScroll(130, 100, 500)).toBeLessThan(0);
    expect(edgeScroll(500, 100, 500)).toBe(18);
    expect(edgeScroll(900, 100, 500)).toBe(18);
    expect(edgeScroll(-50, 100, 500)).toBe(-18);
  });
});

describe('dropIndex', () => {
  const list = [{ key: 'a/1', serverId: 'a' }, { key: 'a/2', serverId: 'a' }, { key: 'b/1', serverId: 'b' }, { key: 'a/3', serverId: 'a' }];
  it('is the place among the server\'s own workspaces once the moved one is out', () => {
    expect(dropIndex(list, 'a/3', { key: 'a/1', side: 'before' })).toBe(0);
    expect(dropIndex(list, 'a/1', { key: 'a/2', side: 'after' })).toBe(1);
    expect(dropIndex(list, 'a/1', { key: 'a/3', side: 'after' })).toBe(2);
  });
  it('is null for an unknown row', () => {
    expect(dropIndex(list, 'a/1', { key: 'zzz', side: 'before' })).toBeNull();
    expect(dropIndex(list, 'nope', { key: 'a/2', side: 'before' })).toBeNull();
  });
});
