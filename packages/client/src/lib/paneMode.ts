/** Which half of the in-session screen a session shows: the session view (Decision) or the terminal. The terminal is
 *  the default; the choice is kept per session on this device only (localStorage, best effort). */
export type PaneMode = 'decision' | 'terminal';
export const PANE_KEY = 'termenbox_pane_modes';
const MAX_KEPT = 100;

const store = (): Pick<Storage, 'getItem' | 'setItem'> | undefined => {
  try { return typeof localStorage === 'undefined' ? undefined : localStorage; } catch { return undefined; }
};

export function loadPaneModes(storage = store()): Record<string, PaneMode> {
  try {
    const raw = JSON.parse(storage?.getItem(PANE_KEY) ?? '{}');
    if (!raw || typeof raw !== 'object') return {};
    return Object.fromEntries(Object.entries(raw).filter(([, v]) => v === 'decision')) as Record<string, PaneMode>;
  } catch { return {}; }
}

/** The new map; only `decision` is stored, since `terminal` is the default. */
export function withPaneMode(modes: Record<string, PaneMode>, key: string, mode: PaneMode, storage = store()): Record<string, PaneMode> {
  const next = { ...modes };
  delete next[key];
  if (mode === 'decision') next[key] = mode;
  const kept = Object.fromEntries(Object.entries(next).slice(-MAX_KEPT));
  try { storage?.setItem(PANE_KEY, JSON.stringify(kept)); } catch { /* no storage: the choice lasts until reload */ }
  return kept;
}
