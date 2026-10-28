// Server diagnostics (decision 0144): the owner asks the server for a scrubbed bundle, reads it, and then downloads it or
// opens a prefilled `incident` issue. Nothing leaves the app without a tap. After an unclean exit the app says so once.
import { MAX_URL_TEXT, type Plan } from './report';

const REPO = 'termenbox/termenbox';
/** Where the server's bundle puts the log; the part of the bundle that matters least when the text must be cut. */
const LOG_HEADER = '\nlog (newest last):\n';
const TRIMMED = '\n(trimmed to fit the issue address: attach the downloaded file)\n';

/** Puts the parts of one answer back together. A part 0 starts over; a part out of order drops what was held. */
export class BundleReader {
  private held: string[] = [];

  /** The whole text once the last part is in, else null. */
  add(part: number, parts: number, text: string): string | null {
    if (part === 0) this.held = [];
    if (part !== this.held.length || parts < 1) {
      this.held = [];
      return null;
    }
    this.held.push(text);
    if (this.held.length < parts) return null;
    const whole = this.held.join('');
    this.held = [];
    // The server has already scrubbed it; scrubbing again would also rewrite `acp.rs:12` and `11:51:09`.
    return whole;
  }
}

function encodedLength(text: string): number {
  return encodeURIComponent(text).length;
}

/** Drops a lone high surrogate a slice may leave at the end: encodeURIComponent throws on it. */
function wellFormed(text: string): string {
  const last = text.charCodeAt(text.length - 1);
  return last >= 0xd800 && last <= 0xdbff ? text.slice(0, -1) : text;
}

function shrinkTo(text: string, max: number, note = ''): string {
  let cut = text;
  while (cut.length > 0 && encodedLength(cut + note) > max) cut = wellFormed(cut.slice(0, Math.floor(cut.length * 0.9)));
  return cut;
}

/**
 * Cuts the text until its encoded form fits. The versions and exit report at the top keep at most 40% of the room,
 * so the newest log lines always get the rest, even after a crash file made the head long.
 */
export function trimForUrl(text: string, max = MAX_URL_TEXT): { text: string; trimmed: boolean } {
  if (encodedLength(text) <= max) return { text, trimmed: false };
  const at = text.indexOf(LOG_HEADER);
  const pre = at < 0 ? text : text.slice(0, at);
  const log = at < 0 ? '' : text.slice(at + LOG_HEADER.length);
  const head = shrinkTo(pre, Math.floor(max * 0.4)) + (at < 0 ? '' : LOG_HEADER);
  const lines = log.split('\n');
  let keep = lines.length;
  const tail = (n: number) => lines.slice(lines.length - n).join('\n');
  while (keep > 0 && encodedLength(head + tail(keep) + TRIMMED) > max) keep = Math.max(0, keep - Math.max(1, Math.floor(keep / 8)));
  return { text: shrinkTo(head + tail(keep), max, TRIMMED) + TRIMMED, trimmed: true };
}

/** The `version:` line of the bundle, for the template's version field. */
function versionOf(text: string): string {
  return /^version: (\S+)/m.exec(text)?.[1] ?? '';
}

/** The prefilled issue. The text shown to the user is what goes in; what does not fit is theirs to attach. */
export function incidentPlan(text: string, what = ''): Plan & { trimmed: boolean } {
  // The user's words are bounded by their encoded size too, and come out of the diagnostics' share.
  const said = what ? encodeURIComponent(shrinkTo(what, 1500)) : '';
  const { text: fit, trimmed } = trimForUrl(text, MAX_URL_TEXT - said.length);
  const version = versionOf(text);
  const query = [
    'template=incident.yml',
    said ? `what=${said}` : '',
    version ? `version=${encodeURIComponent(version)}` : '',
    `diagnostics=${encodeURIComponent(fit)}`,
  ].filter(Boolean).join('&');
  return { url: `https://github.com/${REPO}/issues/new?${query}`, trimmed };
}

export function bundleFileName(now = new Date()): string {
  return `termenbox-diagnostics-${now.toISOString().replace(/[:.]/g, '-')}.txt`;
}

/** Remembers the unclean exits that were already announced, so a reconnect does not repeat the banner. */
export class SeenExits {
  private seen = new Map<string, number>();

  constructor(private storage: Pick<Storage, 'getItem' | 'setItem'> | null = null, private key = 'termenbox-seen-exits') {
    try {
      const saved = JSON.parse(storage?.getItem(key) ?? '{}') as Record<string, unknown>;
      for (const [id, at] of Object.entries(saved)) if (typeof at === 'number') this.seen.set(id, at);
    } catch { /* unreadable storage: every exit is new once */ }
  }

  /** True the first time this server reports this `at_ms`. */
  announce(serverId: string, atMs: number): boolean {
    if (this.seen.get(serverId) === atMs) return false;
    this.seen.set(serverId, atMs);
    try { this.storage?.setItem(this.key, JSON.stringify(Object.fromEntries(this.seen))); } catch { /* kept for this page load only */ }
    return true;
  }
}

export interface ExitNotice { serverId: string; atMs: number; streak: number; crashed: boolean }

/** One line for the banner: when it was last seen, and how it ended if it left a record. */
export function exitText(n: ExitNotice, serverName: string, now = Date.now()): string {
  const mins = Math.max(1, Math.round((now - n.atMs) / 60000));
  const ago = mins < 90 ? `${mins} min ago` : `${Math.round(mins / 60)} h ago`;
  const name = serverName || 'The server';
  const how = n.crashed ? 'crashed' : 'stopped without a clean exit';
  const again = n.streak > 1 ? `, ${n.streak} times in a row` : '';
  return `${name} ${how} (last seen ${ago}${again}).`;
}
