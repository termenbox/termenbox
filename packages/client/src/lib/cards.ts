// Workspace cards (plan v8): what a card shows and how the list is ordered. Pure functions, so the rules (a state is
// never colour alone, the order holds still under a finger) are tested without a browser. Text from agents is
// plain text: nothing here builds markup.
import { isStale, type WorkspaceAgent } from './agents';
import type { AgentState } from './protocol';

export interface Chip {
  /** The word. A state is never shown by colour alone. */
  label: string;
  icon: string;
  tone: 'need' | 'work' | 'done' | 'error' | 'idle';
}

const CHIPS: Record<AgentState, Chip> = {
  needs_input: { label: 'Needs you', icon: '❗', tone: 'need' },
  working: { label: 'Working', icon: '⚙', tone: 'work' },
  done: { label: 'Done', icon: '✓', tone: 'done' },
  error: { label: 'Error', icon: '✕', tone: 'error' },
  idle: { label: 'Idle', icon: '·', tone: 'idle' },
  unknown: { label: 'No status', icon: '·', tone: 'idle' },
};

export function chipFor(a: WorkspaceAgent, nowMs: number): Chip & { stale: boolean } {
  const stale = isStale(a, nowMs);
  return { ...(CHIPS[a.state] ?? CHIPS.unknown), stale };
}

export const isDone = (status: string) => /^(done|completed|complete|finished)$/i.test(status);

/** "3/7" from a plan, counting finished items; null without a plan. */
export function progress(plan: WorkspaceAgent['plan']): string | null {
  if (!plan || plan.length === 0) return null;
  const done = plan.filter((i) => isDone(i.status)).length;
  return `${done}/${plan.length}`;
}

export function ageText(ts: number, nowMs: number): string {
  const s = Math.max(0, Math.round((nowMs - ts) / 1000));
  if (s < 10) return 'now';
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  return `${Math.floor(s / 3600)} h`;
}

/** "Edit src/app.ts" for a tool that is running, "Last: Bash ls" once it finished. */
export function toolLine(a: WorkspaceAgent): string {
  if (!a.tool || !a.tool.name) return '';
  const what = [a.tool.name, a.tool.summary].filter(Boolean).join(' ');
  return a.tool.phase === 'finished' ? `Last: ${what}` : what;
}

export function staleText(a: WorkspaceAgent, nowMs: number): string {
  return `no update for ${Math.max(1, Math.floor((nowMs - a.ts) / 60_000))} min`;
}

/**
 * The order cards are shown in: the order the caller gives (the user's own, lib/order.ts), never a state's. It follows the
 * caller only when nothing could move under a finger: no touch is down, no card is expanded and the list has not been
 * scrolled for `holdMs`. Inserts and removals also wait for release.
 */
export class FrozenOrder {
  private shown: string[] = [];
  private touches = 0;
  private pointers = new Set<number>();
  private sheets = 0;
  private lastScroll = -Infinity;

  constructor(private holdMs = 2000) {}

  touchDown(pointerId?: number) {
    if (pointerId === undefined) this.touches++;
    else this.pointers.add(pointerId);
  }
  touchUp(pointerId?: number) {
    if (pointerId === undefined) this.touches = Math.max(0, this.touches - 1);
    else this.pointers.delete(pointerId);
  }
  hasPointer(pointerId: number) { return this.pointers.has(pointerId); }
  get pointerCount() { return this.pointers.size; }
  sheet(open: boolean) {
    this.sheets = Math.max(0, this.sheets + (open ? 1 : -1));
  }
  scrolled(nowMs: number) {
    this.lastScroll = nowMs;
  }
  frozen(nowMs: number): boolean {
    return this.touches > 0 || this.pointers.size > 0 || this.sheets > 0 || nowMs - this.lastScroll < this.holdMs;
  }

  order(ids: string[], nowMs: number): string[] {
    if (!this.frozen(nowMs) || this.shown.length === 0) {
      this.shown = [...ids];
    }
    return this.shown;
  }
}
