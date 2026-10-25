// The prompt cache of an idle agent (plan H.8, decision 0083): how long until it is cold. Until the server sends
// exact expiry this is an estimate from the last answer, and the text says so ("about").
import type { WorkspaceAgent } from './agents';

/** Milliseconds left, or null when this workspace has no warm cache to speak of (no agent, working, never answered). */
export function cacheRemaining(a: WorkspaceAgent | undefined, now: number, ttlMs: number): number | null {
  if (!a?.agent || (a.state !== 'idle' && a.state !== 'done') || a.lastResponseAt === undefined) return null;
  return Math.max(0, ttlMs - Math.max(0, now - a.lastResponseAt));
}

export function cacheText(remainingMs: number, exact = false): string {
  const about = exact ? '' : 'about ';
  if (remainingMs <= 0) return 'cache cold';
  if (remainingMs < 60_000) return `${about}${Math.ceil(remainingMs / 1000)} s`;
  return `${about}${Math.ceil(remainingMs / 60_000)} min`;
}

/** Inside the last minute: the tab shows a thin ring and the card offers Keep warm. */
export const expiringSoon = (remainingMs: number | null) => remainingMs !== null && remainingMs > 0 && remainingMs <= 60_000;
