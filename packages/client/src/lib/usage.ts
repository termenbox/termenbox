// On-device usage meter (decision 0073, guardrail usage-stays-on-device): foreground time with an attached
// workspace, per ISO week, kept in this browser only. Nothing here talks to a network.

export const TIERS = [
  { name: 'copper', hours: 1 },
  { name: 'silver', hours: 5 },
  { name: 'gold', hours: 15 },
  { name: 'platinum', hours: 30 },
] as const;
export type Tier = (typeof TIERS)[number]['name'];

export const USAGE_KEY = 'termenbox_usage';
const KEEP_WEEKS = 8;
const HOUR = 3_600_000;
/** A tick longer than this means the tab was suspended; the gap is not counted. */
export const MAX_TICK_MS = 6_000;

export type Weeks = Record<string, number>;

/** ISO 8601 week of a local date, as `YYYY-Www` (weeks start on Monday; week 1 holds the year's first Thursday). */
export function weekKey(d: Date): string {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = t.getUTCDay() || 7;
  t.setUTCDate(t.getUTCDate() + 4 - day); // the Thursday of this week decides the year
  const year = t.getUTCFullYear();
  const week = Math.ceil(((t.getTime() - Date.UTC(year, 0, 1)) / 86_400_000 + 1) / 7);
  return `${year}-W${String(week).padStart(2, '0')}`;
}

export function tierOf(hours: number): Tier | null {
  let tier: Tier | null = null;
  for (const t of TIERS) if (hours >= t.hours) tier = t.name;
  return tier;
}

/** Time to add for one tick: nothing while hidden or unattached, and a suspended tab's gap is capped. */
export function tickMs(elapsedMs: number, visible: boolean, attached: boolean): number {
  if (!visible || !attached || elapsedMs <= 0) return 0;
  return Math.min(elapsedMs, MAX_TICK_MS);
}

export function addTime(weeks: Weeks, ms: number, now: Date): Weeks {
  if (ms <= 0) return weeks;
  const key = weekKey(now);
  const next = { ...weeks, [key]: (weeks[key] ?? 0) + ms };
  const keep = Object.keys(next).sort().slice(-KEEP_WEEKS);
  return Object.fromEntries(keep.map((k) => [k, next[k]]));
}

export interface Summary {
  /** Tier earned in the last completed week, if any. */
  tier: Tier | null;
  lastWeekHours: number;
  weekHours: number;
  next: { name: Tier; hoursLeft: number } | null;
  history: { week: string; hours: number }[];
}

export function summary(weeks: Weeks, now: Date): Summary {
  const hours = (k: string) => (weeks[k] ?? 0) / HOUR;
  const previous = new Date(now);
  previous.setDate(previous.getDate() - 7);
  const lastWeekHours = hours(weekKey(previous));
  const weekHours = hours(weekKey(now));
  const nextTier = TIERS.find((t) => t.hours > weekHours);
  const history = Object.keys(weeks)
    .sort()
    .slice(-KEEP_WEEKS)
    .map((week) => ({ week, hours: hours(week) }));
  return {
    tier: tierOf(lastWeekHours),
    lastWeekHours,
    weekHours,
    next: nextTier ? { name: nextTier.name, hoursLeft: nextTier.hours - weekHours } : null,
    history,
  };
}

export function load(storage: Pick<Storage, 'getItem'> = localStorage): Weeks {
  try {
    const parsed = JSON.parse(storage.getItem(USAGE_KEY) || '{}');
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

export function save(weeks: Weeks, storage: Pick<Storage, 'setItem'> = localStorage): void {
  try {
    storage.setItem(USAGE_KEY, JSON.stringify(weeks));
  } catch {
    /* private mode or full storage: the meter just stops counting */
  }
}
