// What the Updates card and the update banner say about a server's updater (decision 0153). Pure, so each phase and
// each missing field is tested without a component.
import type { UpdateInfo } from './protocol';

/** A good check older than this is a health warning: the server is not learning about releases. */
export const STALE_AFTER_S = 72 * 3600;
/** How long "Checking…" waits for the updater before giving up; the helper answers within a poll (20 s) in practice. */
export const CHECK_WAIT_MS = 90_000;

export type UpdateAction = 'check' | 'postpone' | 'skip';

const ago = (seconds: number) => {
  if (seconds < 90) return 'just now';
  if (seconds < 3600) return `${Math.round(seconds / 60)} min ago`;
  if (seconds < 36 * 3600) return `${Math.round(seconds / 3600)} h ago`;
  return `${Math.round(seconds / 86400)} d ago`;
};

/** "Last checked 3 h ago", or why there is nothing to say yet. */
export function lastChecked(info: UpdateInfo | undefined, nowMs: number): string {
  const ok = info?.checked_ok_at ?? 0;
  if (!ok) return info?.checked_at ? 'No successful check yet' : 'Not checked yet';
  return `Last checked ${ago(Math.max(0, nowMs / 1000 - ok))}`;
}

/** True when the last good check is over 72 h old, or checks have only failed. A server that reports nothing is not stale. */
export function checkIsStale(info: UpdateInfo | undefined, nowMs: number): boolean {
  if (!info) return false;
  const ok = info.checked_ok_at ?? 0;
  if (ok) return nowMs / 1000 - ok > STALE_AFTER_S;
  return (info.checked_at ?? 0) > 0 && !!info.error;
}

/** "03:00" for today, "Sat 03:00" for another day. */
export function deadlineText(deadline: number, nowMs: number): string {
  const at = new Date(deadline * 1000);
  const time = at.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
  return at.toDateString() === new Date(nowMs).toDateString() ? time : `${at.toLocaleDateString([], { weekday: 'short' })} ${time}`;
}

/** The line under the version: what happens next and what the owner can do about it. */
export function nextStep(info: UpdateInfo | undefined, available: string, nowMs: number, can: (a: UpdateAction) => boolean): string {
  if (info?.phase === 'installing') return 'Installing…';
  if (info?.phase === 'rolled-back') return 'The last update was rolled back. Update now tries again.';
  if (!available) return 'Up to date.';
  const deadline = info?.deadline ?? 0;
  if (deadline <= 0) return `v${available} is available.`;
  const options = [can('postpone') && info?.can_postpone ? 'postpone' : '', can('skip') ? 'skip' : ''].filter(Boolean).join(' or ');
  return `v${available} installs at ${deadlineText(deadline, nowMs)}${options ? ` unless you ${options}` : ''}.`;
}

/** Whether a check asked for at `baseline` (the `checked_at` then) has been answered. */
export const checkAnswered = (info: UpdateInfo | undefined, baseline: number) => (info?.checked_at ?? 0) > baseline;
