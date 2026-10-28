// A small fake home for the scripted server's `list_dir` (DECISION-0147): the same answers the real server gives, from a
// tree in memory. Shared by the functional e2e (newworkspace.mjs) and the video recorder (video/), so both see one home.
export const HOME = '/home/tester';

const names = (n) => Array.from({ length: n }, (_, i) => `proj-${String(i).padStart(3, '0')}`);

/** Directory name -> children (directories only). `null` marks a folder whose listing fails until `FakeTree.broken` is cleared. */
const TREE = {
  [HOME]: ['code', 'empty', 'big', 'locked', '.config', 'notes'],
  [`${HOME}/code`]: ['termenbox', 'app', 'api'],
  [`${HOME}/code/termenbox`]: ['packages', 'docs'],
  [`${HOME}/code/app`]: [],
  [`${HOME}/code/api`]: [],
  [`${HOME}/empty`]: [],
  [`${HOME}/big`]: names(501),
  [`${HOME}/locked`]: [],
  [`${HOME}/.config`]: ['git'],
  [`${HOME}/notes`]: [],
};

export class FakeTree {
  /** Paths whose listing answers an error; clear one to let a Retry succeed. */
  broken = new Set([`${HOME}/locked`]);
  /** Every request received, for assertions. */
  requests = [];

  /** The reply for a `list_dir` request, shaped like the server's `dir_listing` / `dir_error`. */
  reply(req) {
    this.requests.push(req);
    const path = (req.path ?? HOME).replace(/\/+$/, '') || '/';
    if (path !== HOME && !path.startsWith(`${HOME}/`)) return { type: 'dir_error', reason: 'That folder is outside your home folder.' };
    if (this.broken.has(path)) return { type: 'dir_error', reason: 'Permission denied.' };
    const children = TREE[path];
    if (!children) return { type: 'dir_error', reason: 'No such folder.' };
    const prefix = (req.prefix ?? '').toLowerCase();
    let entries = children.map((name) => ({ name, symlink: false, hidden: name.startsWith('.') }));
    if (!req.hidden) entries = entries.filter((e) => !e.hidden);
    if (prefix) entries = entries.filter((e) => e.name.toLowerCase().startsWith(prefix));
    entries.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
    const truncated = entries.length > 500;
    return { type: 'dir_listing', path, parent: path === HOME ? null : path.slice(0, path.lastIndexOf('/')), entries: entries.slice(0, 500), truncated, home: HOME };
  }
}
