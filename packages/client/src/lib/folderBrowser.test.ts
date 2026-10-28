import { describe, it, expect } from 'vitest';
import { FolderBrowser } from './folderBrowser';
import { DirClient, type DirReply, type ListDir } from './dirClient';

const HOME = '/home/u';
// A fake tree; every request is recorded.
const tree: Record<string, string[]> = {
  '/home/u': ['code', 'empty', 'big'],
  '/home/u/code': ['app', 'termenbox'],
  '/home/u/empty': [],
  '/home/u/code/termenbox': [],
};
function fake() {
  const calls: { path?: string; prefix?: string; hidden?: boolean }[] = [];
  const list: ListDir = async (req) => {
    calls.push(req);
    const path = req.path ?? HOME;
    if (path === '/home/u/bad') return { ok: false, reason: 'permission denied' };
    if (!tree[path]) return { ok: false, reason: 'not a folder' };
    const names = tree[path].filter(n => !req.prefix || n.toLowerCase().startsWith(req.prefix.toLowerCase()));
    const hidden = req.hidden ? [{ name: '.config', hidden: true }] : [];
    return { ok: true, path, parent: path === HOME ? null : path.slice(0, path.lastIndexOf('/')), home: HOME, entries: [...names.map(name => ({ name })), ...hidden], truncated: path === HOME && names.includes('big') && !req.prefix };
  };
  return { list, calls };
}

describe('FolderBrowser', () => {
  it('lists home first and reports the root', async () => {
    const { list, calls } = fake();
    const b = new FolderBrowser(list);
    await b.open();
    expect(calls[0].path).toBeUndefined();
    expect(b.state).toMatchObject({ status: 'ready', path: HOME, home: HOME });
    expect(b.state.entries.map(e => e.name)).toEqual(['code', 'empty', 'big']);
    expect(b.selection).toBe(HOME);
  });
  it('enters nested folders and selects the one shown', async () => {
    const b = new FolderBrowser(fake().list);
    await b.open();
    await b.enter('code');
    expect(b.state.path).toBe('/home/u/code');
    await b.enter('termenbox');
    expect(b.selection).toBe('/home/u/code/termenbox');
  });
  it('shows an error for an unlistable folder and Retry asks again', async () => {
    const f = fake();
    const b = new FolderBrowser(f.list);
    await b.open();
    await b.open('/home/u/bad');
    expect(b.state).toMatchObject({ status: 'error', error: 'permission denied' });
    expect(b.selection).toBeNull();
    const n = f.calls.length;
    await b.retry();
    expect(f.calls.length).toBe(n + 1);
    expect(f.calls.at(-1)?.path).toBe('/home/u/bad');
  });
  it('goes up and to home', async () => {
    const b = new FolderBrowser(fake().list);
    await b.open('/home/u/code');
    await b.up();
    expect(b.state.path).toBe('/home/u');
    await b.open('/home/u/code');
    await b.toHome();
    expect(b.state.path).toBe(HOME);
  });
  it('sends the filter as a prefix and keeps it on the same folder', async () => {
    const f = fake();
    const b = new FolderBrowser(f.list);
    await b.open('/home/u/code');
    await b.setFilter('te');
    expect(f.calls.at(-1)).toMatchObject({ path: '/home/u/code', prefix: 'te' });
    expect(b.state.entries.map(e => e.name)).toEqual(['termenbox']);
    await b.enter('x'); // entering a folder clears the filter
    expect(b.state.filter).toBe('');
  });
  it('re-requests when hidden folders are toggled', async () => {
    const f = fake();
    const b = new FolderBrowser(f.list);
    await b.open();
    await b.setHidden(true);
    expect(f.calls.at(-1)?.hidden).toBe(true);
    expect(b.state.entries.some(e => e.name === '.config')).toBe(true);
  });
  it('shows the empty folder and the truncated notice', async () => {
    const b = new FolderBrowser(fake().list);
    await b.open('/home/u/empty');
    expect(b.state).toMatchObject({ status: 'ready', truncated: false });
    expect(b.state.entries).toEqual([]);
    await b.open();
    expect(b.state.truncated).toBe(true);
  });
  it('navigates to a typed path in home, accepts one outside as typed, rejects a relative one', async () => {
    const f = fake();
    const b = new FolderBrowser(f.list);
    await b.open();
    expect(b.go('code/app')).toMatch(/absolute/);
    expect(b.go('')).toMatch(/absolute/);
    expect(b.go('/home/u/code/')).toBeNull();
    await new Promise(r => setTimeout(r));
    expect(b.state).toMatchObject({ status: 'ready', path: '/home/u/code', outside: false });
    const n = f.calls.length;
    expect(b.go('/srv/work')).toBeNull();
    expect(f.calls.length).toBe(n);
    expect(b.state).toMatchObject({ status: 'ready', path: '/srv/work', outside: true });
    expect(b.selection).toBe('/srv/work');
  });
  it('ignores an answer that a newer request replaced', async () => {
    let release!: (r: DirReply) => void;
    let first = true;
    const list: ListDir = (req) => first ? (first = false, new Promise<DirReply>(r => (release = r))) : Promise.resolve({ ok: true, path: req.path ?? HOME, home: HOME, entries: [{ name: 'new' }] });
    const b = new FolderBrowser(list);
    const slow = b.open('/home/u/code');
    await b.open('/home/u/empty');
    release({ ok: true, path: '/home/u/code', home: HOME, entries: [{ name: 'old' }] });
    await slow;
    expect(b.state.path).toBe('/home/u/empty');
    expect(b.state.entries).toEqual([{ name: 'new' }]);
  });
});

describe('DirClient', () => {
  it('answers requests in order and fails the waiting ones on a drop', async () => {
    const sent: unknown[] = [];
    const c = new DirClient(m => sent.push(m));
    const a = c.list({ path: '/a' });
    const b = c.list({ prefix: 'x', hidden: true });
    expect(sent).toEqual([{ type: 'list_dir', path: '/a' }, { type: 'list_dir', prefix: 'x', hidden: true }]);
    expect(c.handle({ type: 'workspaces', workspaces: [] })).toBe(false);
    c.handle({ type: 'dir_listing', path: '/a', entries: [], home: '/h' });
    expect(await a).toMatchObject({ ok: true, path: '/a' });
    c.fail('connection lost');
    expect(await b).toEqual({ ok: false, reason: 'connection lost' });
    c.handle({ type: 'dir_error', reason: 'stray' }); // nobody waiting: ignored
  });
});
