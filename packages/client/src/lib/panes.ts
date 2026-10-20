// The desktop workspace layout (decision 0101, roadmap U15): whether the docked sidebar and the side-by-side terminal
// are open, and how wide each is. Kept per device in localStorage; every storage access is guarded because Safari
// private mode, blocked site data and previews make it throw or come back empty.
import { get, writable } from 'svelte/store';

/** At and below this width the phone layout applies (bottom bar, sheet); above it the desktop workspace. */
export const PHONE_MAX = 720;
export const WIDE_QUERY = `(min-width: ${PHONE_MAX + 1}px)`;

export interface Range { min: number; max: number; def: number; step: number }
export const SIDEBAR: Range = { min: 200, max: 480, def: 260, step: 16 };
/** The terminal beside the structured view. */
export const SPLIT: Range = { min: 280, max: 960, def: 480, step: 16 };
/** The part of the window the content must keep when the sidebar is open: below this the terminal is not usable. */
export const MIN_CONTENT = 420;
/** Each half of the side-by-side view keeps at least this much; narrower than twice this, the view is not split. */
export const MIN_HALF = 280;
export const LARGE_STEP = 64;

export interface Layout { sidebarOpen: boolean; sidebarWidth: number; splitOpen: boolean; splitWidth: number }
export const DEFAULT_LAYOUT: Layout = { sidebarOpen: true, sidebarWidth: SIDEBAR.def, splitOpen: false, splitWidth: SPLIT.def };

const KEY = 'termenbox_workspace';

export const clamp = (n: number, min: number, max: number) => Math.min(Math.max(n, min), Math.max(min, max));

/** The sidebar's width as drawn: inside its own range, and leaving the content its minimum in a narrow window. */
export const sidebarWidthFor = (width: number, viewport: number) => clamp(Math.round(width), SIDEBAR.min, Math.min(SIDEBAR.max, viewport - MIN_CONTENT));
/** The terminal's width in the side-by-side view, leaving the structured view its minimum. */
export const splitWidthFor = (width: number, available: number) => clamp(Math.round(width), SPLIT.min, Math.min(SPLIT.max, available - MIN_HALF));
/** Whether a workspace this wide can hold both halves. */
export const canSplit = (available: number) => available >= MIN_HALF * 2;

const num = (v: unknown, r: Range) => (typeof v === 'number' && Number.isFinite(v) ? clamp(Math.round(v), r.min, r.max) : r.def);

/** Reads a saved layout; anything missing or malformed falls back to the default for that field. */
export function parseLayout(raw: string | null | undefined): Layout {
  try {
    const v = raw ? JSON.parse(raw) : null;
    if (!v || typeof v !== 'object') return { ...DEFAULT_LAYOUT };
    return {
      sidebarOpen: v.sidebarOpen !== false,
      sidebarWidth: num(v.sidebarWidth, SIDEBAR),
      splitOpen: v.splitOpen === true,
      splitWidth: num(v.splitWidth, SPLIT),
    };
  } catch {
    return { ...DEFAULT_LAYOUT };
  }
}

type Reader = Pick<Storage, 'getItem'>;
type Writer = Pick<Storage, 'setItem'>;
const storage = (): Storage | undefined => { try { return typeof localStorage === 'undefined' ? undefined : localStorage; } catch { return undefined; } };

export function loadLayout(store: Reader | undefined = storage()): Layout {
  try { return parseLayout(store?.getItem(KEY)); } catch { return { ...DEFAULT_LAYOUT }; }
}
export function saveLayout(layout: Layout, store: Writer | undefined = storage()) {
  try { store?.setItem(KEY, JSON.stringify(layout)); } catch { /* kept for this visit only */ }
}

/** The one layout the frame and the workspace share. Changing it moves the panes; `commitLayout` saves it. */
export const layout = writable<Layout>(loadLayout());
export const commitLayout = () => saveLayout(get(layout));
export function patchLayout(patch: Partial<Layout>, save = true) {
  layout.update((l) => ({ ...l, ...patch }));
  if (save) commitLayout();
}
export const toggleSidebar = () => patchLayout({ sidebarOpen: !get(layout).sidebarOpen });
export const toggleSplit = () => patchLayout({ splitOpen: !get(layout).splitOpen });
