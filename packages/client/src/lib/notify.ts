// Proactive signals (plan H.7): who needs you, how the page title and app badge say so, and which answers the
// banner may offer. Pure functions; nothing here chooses the active session.
import { isStale, type AgentMap, type SessionAgent } from './agents';
import { needsYou } from './tabs';

export interface Waiting { key: string; name: string; since: number; agent: SessionAgent }

/** Sessions that need you, the longest wait first. */
export function waitingList(sessions: { key: string; name: string }[], agents: AgentMap): Waiting[] {
  return sessions
    .filter(s => needsYou(agents[s.key]))
    .map(s => ({ key: s.key, name: s.name, since: agents[s.key].attention?.raised_at ?? agents[s.key].waitingSince ?? agents[s.key].ts, agent: agents[s.key] }))
    .sort((a, b) => a.since - b.since);
}

export const titleFor = (count: number) => count > 0 ? `(${count}) Termenbox` : 'Termenbox';
export const bannerText = (count: number) => count === 1 ? '1 needs you' : `${count} need you`;
/** What the first item of the banner is about, when it is not a question. */
export const bannerKind = (a: SessionAgent) => a.attention?.kind === 'finished_unseen' ? 'finished' : a.attention?.kind === 'failed' ? 'failed' : '';

/** The choices the banner may offer: only for a prompt the agent's own hook confirmed, and never a stale one. */
export function bannerChoices(a: SessionAgent, now: number): string[] {
  if (a.attention ? a.attention.kind !== 'needs_input' : a.state !== 'needs_input') return [];
  if (!a.prompt || a.prompt.confidence !== 'high' || isStale(a, now)) return [];
  return a.prompt.choices.length > 0 && a.prompt.choices.length <= 3 ? a.prompt.choices : [];
}

/** Sets the title and, where the browser has it, the app icon badge. */
export function announce(count: number, doc: { title: string } = document, nav: { setAppBadge?: (n?: number) => Promise<void>; clearAppBadge?: () => Promise<void> } = navigator) {
  doc.title = titleFor(count);
  try {
    if (count > 0) void nav.setAppBadge?.(count)?.catch?.(() => {});
    else void nav.clearAppBadge?.()?.catch?.(() => {});
  } catch { /* Badging is optional. */ }
}

/** The parts of the page the attention signal touches: the title and the favicon link, when there is one. */
export interface AttentionPage { title: string; icon: { href: string } | null }

/** The favicon that carries the red dot, next to the normal one. */
export const attentionIcon = (href: string) => href.replace(/favicon\.svg(?=$|[?#])/, 'favicon-attention.svg');

/**
 * Makes a tab that needs you noticeable: while the page is hidden the title and favicon alternate between the
 * attention form and the normal one, and while it is visible they stay still. One controller lives as long as the
 * page; `update` is cheap to call on every change.
 */
export function createAttention(page: AttentionPage, every = 1000) {
  const home = { title: 'Termenbox', icon: page.icon?.href ?? '' };
  let count = 0;
  let hidden = false;
  let on = true;
  let timer: ReturnType<typeof setInterval> | undefined;

  function paint() {
    const lit = count > 0 && (!hidden || on);
    page.title = lit ? titleFor(count) : home.title;
    if (page.icon) page.icon.href = lit ? attentionIcon(home.icon) : home.icon;
  }
  function update(nextCount: number, nextHidden: boolean) {
    count = nextCount;
    hidden = nextHidden;
    const flashing = count > 0 && hidden;
    if (flashing && !timer) timer = setInterval(() => { on = !on; paint(); }, every);
    if (!flashing && timer) { clearInterval(timer); timer = undefined; }
    on = true;
    paint();
  }
  return { update, stop: () => update(0, false) };
}

/**
 * Raises one browser notification for each session that starts needing you while the page is hidden, for browsers
 * without Web Push. It never asks for permission (that needs a gesture, and the push toggle already has one) and
 * stays quiet when this device is subscribed to push, which would notify twice.
 */
export function createNotifier(
  show: (title: string, options: NotificationOptions) => { onclick: ((e: Event) => void) | null; close(): void } | void,
  permission: () => string,
  onOpen: (key: string) => void,
) {
  const told = new Set<string>();
  return function update(waiting: Waiting[], hidden: boolean, pushOn: boolean) {
    const keys = new Set(waiting.map(w => w.key));
    for (const k of told) if (!keys.has(k)) told.delete(k); // a session that needed you again later is news again
    if (!hidden || pushOn || permission() !== 'granted') return;
    for (const w of waiting) {
      if (told.has(w.key)) continue;
      told.add(w.key);
      const n = show('Termenbox', { body: `${w.name} needs you`, tag: `termenbox-${w.key}`, icon: '/icons/icon-192.png' });
      if (n) n.onclick = () => { onOpen(w.key); n.close(); };
    }
  };
}
