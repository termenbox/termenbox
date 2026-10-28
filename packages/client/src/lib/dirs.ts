// Path helpers for the folder field and picker. Paths are the server's (absolute, `/`-separated); `home` is the server
// user's home as `dir_listing` reports it.

/** `path` is `home` or inside it. */
export function isUnderHome(path: string, home: string): boolean {
  if (!home) return false;
  const h = home.replace(/\/+$/, '');
  return path === h || path.startsWith(`${h}/`) || h === '';
}

/** The folder one level up, or null at `/`. */
export function parentOf(path: string): string | null {
  const p = path.replace(/\/+$/, '');
  if (!p) return null;
  const i = p.lastIndexOf('/');
  return i <= 0 ? '/' : p.slice(0, i);
}

/** The path as a trail of tappable parts: from `~` for a path under home, else from `/`. */
export function breadcrumb(path: string, home: string): { name: string; path: string }[] {
  const under = isUnderHome(path, home) && home !== '/';
  const root = under ? home.replace(/\/+$/, '') : '';
  const trail = [{ name: under ? '~' : '/', path: under ? root : '/' }];
  let at = root;
  for (const seg of path.slice(root.length).split('/').filter(Boolean)) {
    at = `${at}/${seg}`;
    trail.push({ name: seg, path: at });
  }
  return trail;
}

/** The path for a narrow field: `~/` for home, and when longer than `width` a middle ellipsis that keeps the tail. */
export function displayPath(path: string, home: string, width: number): string {
  const s = home && home !== '/' && isUnderHome(path, home) ? `~${path.slice(home.replace(/\/+$/, '').length)}` || '~' : path;
  if (s.length <= width || width < 5) return s;
  const head = Math.floor((width - 1) / 3);
  return `${s.slice(0, head)}…${s.slice(s.length - (width - 1 - head))}`;
}
