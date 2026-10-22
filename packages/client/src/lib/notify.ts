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

/** Handle returned by startAnnounce to stop the flashing. */
export interface AnnounceHandle {
  stop: () => void;
}

/** Starts flashing the tab title and favicon when count > 0. Returns a handle to stop it. */
export function startAnnounce(
  count: number,
  doc: Document = document,
  nav: { setAppBadge?: (n?: number) => Promise<void>; clearAppBadge?: () => Promise<void> } = navigator,
): AnnounceHandle {
  let stopped = false;
  let flashTimer: ReturnType<typeof setInterval> | undefined;
  let originalTitle = doc.title;
  let originalFavicon: HTMLLinkElement | null = null;
  let flashState = true;

  // Find the current favicon
  const faviconLink = doc.querySelector('link[rel="icon"]') as HTMLLinkElement | null;
  if (faviconLink) {
    originalFavicon = faviconLink.cloneNode(true) as HTMLLinkElement;
  }

  function update() {
    if (stopped) return;
    if (count > 0) {
      // Flash title: alternate between attention title and normal
      doc.title = flashState ? titleFor(count) : 'Termenbox';
      // Flash favicon: alternate between default and attention (red dot)
      if (faviconLink) {
        faviconLink.href = flashState ? '/favicon-attention.svg' : '/favicon.svg';
      }
      flashState = !flashState;
      // Badging API
      void nav.setAppBadge?.(count)?.catch?.(() => {});
    } else {
      doc.title = 'Termenbox';
      if (faviconLink && originalFavicon) {
        faviconLink.href = originalFavicon.href;
      }
      void nav.clearAppBadge?.()?.catch?.(() => {});
    }
  }

  if (count > 0) {
    flashTimer = setInterval(update, 1000);
    // Initial update
    update();
  } else {
    update();
  }

  return {
    stop: () => {
      stopped = true;
      if (flashTimer) clearInterval(flashTimer);
      // Restore original state
      doc.title = originalTitle;
      if (faviconLink && originalFavicon) {
        faviconLink.href = originalFavicon.href;
      }
      void nav.clearAppBadge?.()?.catch?.(() => {});
    },
  };
}

/** Legacy single-call announce (no flashing). Kept for backward compatibility. */
export function announce(count: number, doc: { title: string } = document, nav: { setAppBadge?: (n?: number) => Promise<void>; clearAppBadge?: () => Promise<void> } = navigator) {
  doc.title = titleFor(count);
  try {
    if (count > 0) void nav.setAppBadge?.(count)?.catch?.(() => {});
    else void nav.clearAppBadge?.()?.catch?.(() => {});
  } catch { /* Badging is optional. */ }
}

/** Browser Notifications (non-PWA) — works on desktop Chrome/Edge/Firefox, Android Chrome.
 *  Requires user permission. Does NOT work on iOS Safari unless installed as PWA. */
export interface NotificationHandle {
  stop: () => void;
}

/** Shows a browser notification for the first waiting session when page is hidden.
 *  Returns a handle to stop watching. */
export function startBrowserNotifications(
  waiting: Waiting[],
  pageVisible: boolean,
  doc: Document = document,
): NotificationHandle {
  let stopped = false;
  let lastNotifiedKey: string | null = null;
  let pendingKey: string | null = null;

  async function ensurePermission(): Promise<boolean> {
    if (typeof Notification === 'undefined') return false;
    if (Notification.permission === 'granted') return true;
    if (Notification.permission === 'denied') return false;
    // Request permission on first user gesture (caller should trigger on interaction)
    const perm = await Notification.requestPermission();
    return perm === 'granted';
  }

  function notifyIfNeeded() {
    if (stopped || pageVisible) return;
    const first = waiting[0];
    if (!first) return;
    if (lastNotifiedKey === first.key || pendingKey === first.key) return; // already notified or pending for this one
    pendingKey = first.key;
    ensurePermission().then((granted) => {
      pendingKey = null;
      if (!granted || stopped || pageVisible) return;
      const n = new Notification('Termenbox', {
        body: `${first.name} needs you`,
        tag: `termenbox-${first.key}`,
        icon: '/icons/icon-192.png',
        badge: '/icons/icon-192.png',
        data: { sessionKey: first.key },
        requireInteraction: true, // stay until dismissed/clicked
      });
      n.onclick = () => {
        doc.title = 'Termenbox'; // stop flashing
        window.focus();
        n.close();
      };
      lastNotifiedKey = first.key;
    });
  }

  // Initial check
  notifyIfNeeded();

  return {
    stop: () => {
      stopped = true;
      lastNotifiedKey = null;
      pendingKey = null;
    },
  };
}

/** Shows a one-time nudge to pin the tab for native attention glow (pinned tabs only).
 *  Call when waiting sessions exist. Returns a handle to stop/cleanup. */
export interface PinNudgeHandle {
  stop: () => void;
}

const PIN_NUDGE_KEY = 'termenbox_pin_nudge_shown';

export function startPinNudge(
  waiting: Waiting[],
  doc: Document = document,
  showToast: (msg: string) => void,
): PinNudgeHandle {
  let stopped = false;

  // Only show if there are waiting sessions and we haven't shown it this session
  if (waiting.length === 0) return { stop: () => {} };

  const alreadyShown = sessionStorage.getItem(PIN_NUDGE_KEY);
  if (alreadyShown) return { stop: () => {} };

  // Show nudge after a short delay (so it doesn't compete with other UI)
  const timer = setTimeout(() => {
    if (stopped) return;
    sessionStorage.setItem(PIN_NUDGE_KEY, '1');
    showToast('Pin this tab for native attention glow 📌', 'success');
  }, 2000);

  return {
    stop: () => {
      stopped = true;
      clearTimeout(timer);
    },
  };
}
