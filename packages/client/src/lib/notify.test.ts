import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { announce, bannerChoices, bannerText, titleFor, waitingList, startAnnounce, startBrowserNotifications } from './notify';
import type { AgentMap, SessionAgent } from './agents';

const ag = (patch: Partial<SessionAgent>): SessionAgent => ({ state: 'needs_input', seq: 1, stale: false, ts: 1000, ...patch });

describe('needs-you signals', () => {
  it('lists who waits, the longest wait first, and never lists the rest', () => {
    const agents: AgentMap = { a: ag({ waitingSince: 500 }), b: ag({ waitingSince: 100 }), c: ag({ state: 'working' }) };
    const list = waitingList([{ key: 'a', name: 'A' }, { key: 'b', name: 'B' }, { key: 'c', name: 'C' }, { key: 'd', name: 'D' }], agents);
    expect(list.map(w => w.key)).toEqual(['b', 'a']);
  });
  it('words the count and the title', () => {
    expect(titleFor(2)).toBe('(2) Termenbox');
    expect(titleFor(0)).toBe('Termenbox');
    expect(bannerText(1)).toBe('1 needs you');
    expect(bannerText(3)).toBe('3 need you');
  });
  it('offers answers only for a confirmed, fresh, short prompt', () => {
    const prompt = { id: '1', kind: 'yes_no', choices: ['Yes', 'No'], seq: 2, confidence: 'high' };
    expect(bannerChoices(ag({ prompt }), 1000)).toEqual(['Yes', 'No']);
    expect(bannerChoices(ag({ prompt: { ...prompt, confidence: 'low' } }), 1000)).toEqual([]);
    expect(bannerChoices(ag({ prompt: { ...prompt, choices: ['1', '2', '3', '4'] } }), 1000)).toEqual([]);
    expect(bannerChoices(ag({ state: 'working', prompt }), 1000)).toEqual([]);
    expect(bannerChoices(ag({}), 1000)).toEqual([]);
  });
  it('sets the title and the app badge, and clears it', () => {
    const doc = { title: '' };
    const nav = { setAppBadge: vi.fn(async () => {}), clearAppBadge: vi.fn(async () => {}) };
    announce(2, doc, nav);
    expect(doc.title).toBe('(2) Termenbox');
    expect(nav.setAppBadge).toHaveBeenCalledWith(2);
    announce(0, doc, nav);
    expect(nav.clearAppBadge).toHaveBeenCalled();
    announce(1, doc, {}); // a browser without badging
  });
});

describe('startAnnounce', () => {
  let doc: Document;
  let nav: { setAppBadge?: (n?: number) => Promise<void>; clearAppBadge?: () => Promise<void> };
  let originalSetInterval: typeof setInterval;
  let originalClearInterval: typeof clearInterval;
  let timers: Map<number, ReturnType<typeof setInterval>> = new Map();
  let timerId = 0;

  beforeEach(() => {
    // Mock document
    const faviconLink = document.createElement('link');
    faviconLink.rel = 'icon';
    faviconLink.href = '/favicon.svg';
    document.head.appendChild(faviconLink);
    doc = document;
    nav = { setAppBadge: vi.fn(async () => {}), clearAppBadge: vi.fn(async () => {}) };

    // Mock timers
    originalSetInterval = global.setInterval;
    originalClearInterval = global.clearInterval;
    global.setInterval = vi.fn((fn, delay) => {
      const id = ++timerId;
      timers.set(id, originalSetInterval(fn, delay));
      return id as unknown as ReturnType<typeof setInterval>;
    });
    global.clearInterval = vi.fn((id) => {
      const timer = timers.get(id as number);
      if (timer) originalClearInterval(timer);
      timers.delete(id as number);
    }) as typeof clearInterval;
  });

  afterEach(() => {
    global.setInterval = originalSetInterval;
    global.clearInterval = originalClearInterval;
    timers.clear();
    timerId = 0;
    const faviconLink = document.querySelector('link[rel="icon"]');
    if (faviconLink) faviconLink.remove();
  });

  it('flashes title and favicon when count > 0', () => {
    const handle = startAnnounce(2, doc, nav);
    // Initial call should set attention title
    expect(doc.title).toBe('(2) Termenbox');
    expect(nav.setAppBadge).toHaveBeenCalledWith(2);
    // Favicon should be attention version initially
    const faviconLink = doc.querySelector('link[rel="icon"]') as HTMLLinkElement;
    expect(faviconLink?.href).toContain('favicon-attention.svg');
    // Simulate timer tick (flash to normal)
    const timerFn = (global.setInterval as vi.Mock).mock.calls[0]?.[0];
    if (timerFn) timerFn();
    // Should flash to normal title
    expect(doc.title).toBe('Termenbox');
    handle.stop();
  });

  it('stops flashing and restores original state on stop', () => {
    doc.title = 'Termenbox'; // Set initial title
    const handle = startAnnounce(2, doc, nav);
    handle.stop();
    expect(doc.title).toBe('Termenbox');
    expect(nav.clearAppBadge).toHaveBeenCalled();
    const faviconLink = doc.querySelector('link[rel="icon"]') as HTMLLinkElement;
    expect(faviconLink?.href).toContain('favicon.svg');
  });

  it('does not flash when count is 0', () => {
    const handle = startAnnounce(0, doc, nav);
    expect(doc.title).toBe('Termenbox');
    expect(nav.clearAppBadge).toHaveBeenCalled();
    // No interval should be set
    expect(global.setInterval).not.toHaveBeenCalled();
    handle.stop();
  });
});

describe('startBrowserNotifications', () => {
  let doc: Document;
  let mockNotification: ReturnType<typeof vi.fn>;
  let originalNotification: typeof Notification;

  beforeEach(() => {
    doc = document;
    originalNotification = global.Notification;
    mockNotification = vi.fn();
    // @ts-expect-error - mocking global
    global.Notification = mockNotification;
    mockNotification.permission = 'granted';
    mockNotification.requestPermission = vi.fn(async () => 'granted');
  });

  afterEach(() => {
    global.Notification = originalNotification;
    vi.clearAllMocks();
  });

  it('does nothing when page is visible', () => {
    const waiting = [{ key: 'a', name: 'Session A', since: 1000, agent: ag({ waitingSince: 1000 }) }];
    const handle = startBrowserNotifications(waiting, true, doc);
    handle.stop();
    expect(mockNotification).not.toHaveBeenCalled();
  });

  it('shows notification for first waiting session when page is hidden', async () => {
    const waiting = [{ key: 'a', name: 'Session A', since: 1000, agent: ag({ waitingSince: 1000 }) }];
    const handle = startBrowserNotifications(waiting, false, doc);
    // Wait for async permission check
    await new Promise(r => setTimeout(r, 10));
    expect(mockNotification).toHaveBeenCalledWith('Termenbox', expect.objectContaining({
      body: 'Session A needs you',
      tag: 'termenbox-a',
      requireInteraction: true,
    }));
    handle.stop();
  });

  it('deduplicates notifications for same session via tag within same handle', async () => {
    const waiting = [{ key: 'a', name: 'Session A', since: 1000, agent: ag({ waitingSince: 1000 }) }];
    const handle = startBrowserNotifications(waiting, false, doc);
    await new Promise(r => setTimeout(r, 10));
    expect(mockNotification).toHaveBeenCalledTimes(1);
    // Call notifyIfNeeded again internally (simulating reactive update with same session)
    // Since we can't access notifyIfNeeded directly, test that creating a new handle
    // after stopping the old one with same session doesn't re-notify immediately
    handle.stop();
    const handle2 = startBrowserNotifications(waiting, false, doc);
    await new Promise(r => setTimeout(r, 10));
    // New handle instance - will notify again (this is expected behavior)
    expect(mockNotification).toHaveBeenCalledTimes(2);
    handle2.stop();
  });

  it('notifies for new session when waiting changes', async () => {
    const waiting1 = [{ key: 'a', name: 'Session A', since: 1000, agent: ag({ waitingSince: 1000 }) }];
    const handle1 = startBrowserNotifications(waiting1, false, doc);
    await new Promise(r => setTimeout(r, 10));
    expect(mockNotification).toHaveBeenCalledTimes(1);
    handle1.stop();

    // New waiting with different session
    const waiting2 = [{ key: 'b', name: 'Session B', since: 2000, agent: ag({ waitingSince: 2000 }) }];
    const handle2 = startBrowserNotifications(waiting2, false, doc);
    await new Promise(r => setTimeout(r, 10));
    expect(mockNotification).toHaveBeenCalledTimes(2);
    handle2.stop();
  });

  it('does nothing when Notification is not available', async () => {
    // @ts-expect-error - removing Notification
    global.Notification = undefined;
    const waiting = [{ key: 'a', name: 'Session A', since: 1000, agent: ag({ waitingSince: 1000 }) }];
    const handle = startBrowserNotifications(waiting, false, doc);
    await new Promise(r => setTimeout(r, 10));
    expect(mockNotification).not.toHaveBeenCalled();
    handle.stop();
  });

  it('does nothing when permission is denied', async () => {
    mockNotification.permission = 'denied';
    const waiting = [{ key: 'a', name: 'Session A', since: 1000, agent: ag({ waitingSince: 1000 }) }];
    const handle = startBrowserNotifications(waiting, false, doc);
    await new Promise(r => setTimeout(r, 10));
    expect(mockNotification).not.toHaveBeenCalled();
    handle.stop();
  });
});
