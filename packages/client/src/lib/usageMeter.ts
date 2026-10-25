import { writable } from 'svelte/store';
import { addTime, load, save, tickMs, type Weeks } from './usage';

/** This browser's weekly foreground time; the badge and stats read it (plan T.2). */
export const usageWeeks = writable<Weeks>({});

const STEP_MS = 5000;

/**
 * Counts time while the page is visible and `isAttached()` is true (connected, with a workspace open).
 * A suspended tab produces one long gap, which `tickMs` caps. Returns the stop function.
 */
export function startUsageMeter(isAttached: () => boolean, now: () => Date = () => new Date()): () => void {
  let weeks = load();
  usageWeeks.set(weeks);
  let last = performance.now();
  const timer = setInterval(() => {
    const t = performance.now();
    const ms = tickMs(t - last, document.visibilityState === 'visible', isAttached());
    last = t;
    if (ms > 0) {
      weeks = addTime(weeks, ms, now());
      usageWeeks.set(weeks);
      save(weeks);
    }
  }, STEP_MS);
  return () => clearInterval(timer);
}
