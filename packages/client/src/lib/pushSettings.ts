import type { PushSettings } from './protocol';

/** The push settings as the form shows them: minutes (people think in minutes; the wire carries seconds). */
export interface PushForm {
  waitMin: number;
  maxMin: number;
  multiplier: number;
  quiet: string;
  muted: string[];
}

export const toForm = (s: PushSettings): PushForm => ({
  waitMin: Math.round(s.initial_wait_secs / 60),
  maxMin: Math.round(s.max_interval_secs / 60),
  multiplier: s.multiplier,
  quiet: s.quiet_hours ?? '',
  muted: [...s.muted_agents],
});

const num = (v: number, fallback: number) => (Number.isFinite(v) ? v : fallback);

/** What the server is asked to keep. Values are pulled into the server's limits; whatever is still wrong (quiet hours that
 *  do not read as `22:00-07:00`) comes back as `push_settings_rejected`. `utcOffsetMinutes` is the browser's, so quiet hours
 *  mean the owner's local time. */
export function fromForm(f: PushForm, utcOffsetMinutes: number): PushSettings {
  const wait = Math.min(7 * 24 * 60, Math.max(1, Math.round(num(f.waitMin, 10))));
  const max = Math.min(7 * 24 * 60, Math.max(wait, Math.round(num(f.maxMin, 60))));
  const quiet = f.quiet.trim();
  return {
    initial_wait_secs: wait * 60,
    max_interval_secs: max * 60,
    multiplier: Math.min(10, Math.max(1.1, num(f.multiplier, 2))),
    ...(quiet ? { quiet_hours: quiet } : {}),
    utc_offset_minutes: utcOffsetMinutes,
    muted_agents: f.muted,
  };
}

/** The agents to offer as switches: those the server has hooks for, plus any already muted (so a mute can be undone). */
export const mutable = (known: string[], muted: string[]): string[] => [...new Set([...known, ...muted])];

export const toggle = (muted: string[], agent: string): string[] =>
  muted.includes(agent) ? muted.filter((a) => a !== agent) : [...muted, agent];
