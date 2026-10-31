import type { DirEntry } from './protocol';
import type { ListDir } from './dirClient';
import { isUnderHome, parentOf } from './dirs';

export interface BrowserState {
  status: 'loading' | 'ready' | 'error';
  /** The folder shown (or being asked for); what Select returns. */
  path: string;
  home: string;
  entries: DirEntry[];
  truncated: boolean;
  error: string;
  filter: string;
  hidden: boolean;
  /** The path was typed and lies outside home: it cannot be listed, it is used as typed. */
  outside: boolean;
}

/** Up goes to a folder the server will list: inside home, below it. At home (or outside it) there is nowhere to go; Home returns. */
export function canGoUp(s: Pick<BrowserState, 'path' | 'home'>): boolean {
  if (!s.path || parentOf(s.path) === null) return false;
  return !s.home || (s.path !== s.home && isUnderHome(s.path, s.home));
}

/** The state machine behind the folder picker, kept apart from the component so it runs without a DOM. */
export class FolderBrowser {
  state: BrowserState = { status: 'loading', path: '', home: '', entries: [], truncated: false, error: '', filter: '', hidden: false, outside: false };
  private seq = 0;
  constructor(private list: ListDir, private onchange: (s: BrowserState) => void = () => {}) {}

  private set(patch: Partial<BrowserState>) {
    this.state = { ...this.state, ...patch };
    this.onchange(this.state);
  }

  /** List `path` (home when absent), keeping the filter and hidden toggle. A newer call makes an older answer irrelevant. */
  async open(path?: string, opts: { keepFilter?: boolean } = {}) {
    const mine = ++this.seq;
    const filter = opts.keepFilter ? this.state.filter : '';
    this.set({ status: 'loading', error: '', outside: false, filter, ...(path !== undefined ? { path } : {}) });
    const r = await this.list({ path, prefix: filter || undefined, hidden: this.state.hidden || undefined });
    if (mine !== this.seq) return;
    if (r.ok) this.set({ status: 'ready', path: r.path, home: r.home, entries: r.entries, truncated: !!r.truncated, error: '' });
    else this.set({ status: 'error', entries: [], truncated: false, error: r.reason });
  }

  /** Into a child of the folder shown. While a folder loads, `path` is already the target and the old rows are still on screen, so a second click would nest it. */
  enter(name: string) { return this.state.status === 'loading' ? Promise.resolve() : this.open(`${this.state.path.replace(/\/+$/, '')}/${name}`); }
  /** One level up; a no-op at home or outside it (see canGoUp). */
  up() { const p = canGoUp(this.state) ? parentOf(this.state.path) : null; return p ? this.open(p) : Promise.resolve(); }
  toHome() { return this.open(undefined); }
  retry() { return this.open(this.state.path || undefined, { keepFilter: true }); }
  setFilter(filter: string) { this.state = { ...this.state, filter }; return this.open(this.state.path || undefined, { keepFilter: true }); }
  setHidden(hidden: boolean) { this.state = { ...this.state, hidden }; return this.open(this.state.path || undefined, { keepFilter: true }); }

  /** A typed path. Returns a message for one that is rejected, else navigates (under home) or accepts it as typed (elsewhere). */
  go(typed: string): string | null {
    const p = typed.trim().replace(/(.)\/+$/, '$1');
    if (!p.startsWith('/')) return 'Enter an absolute path, like /home/you/project.';
    const { home } = this.state;
    if (home && !isUnderHome(p, home)) {
      this.seq++; // an answer still on its way is for a path no longer shown
      this.set({ status: 'ready', path: p, entries: [], truncated: false, error: '', filter: '', outside: true });
      return null;
    }
    void this.open(p);
    return null;
  }

  /** What Select would choose now, or null while the folder is loading or failed. */
  get selection(): string | null {
    const s = this.state;
    return s.status === 'ready' && s.path ? s.path : null;
  }
}
