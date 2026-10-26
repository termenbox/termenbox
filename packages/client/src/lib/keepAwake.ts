import { nativeHost } from './platform';

interface WakeLockSentinelLike {
  release(): Promise<void>;
}

interface WakeLockLike {
  request(type: 'screen'): Promise<WakeLockSentinelLike>;
}

let wanted = false;
let generation = 0;
let sentinel: WakeLockSentinelLike | null = null;

function browserWakeLock(): WakeLockLike | null {
  const nav = globalThis.navigator as (Navigator & { wakeLock?: WakeLockLike }) | undefined;
  return nav?.wakeLock ?? null;
}

async function acquire(gen: number): Promise<void> {
  const host = nativeHost();
  if (host) {
    if (gen !== generation) return;
    try {
      await host.keepAwake();
    } catch {
      // The wrapper may not expose the keep-awake plugin; the screen simply sleeps as usual.
    }
    return;
  }
  const lock = browserWakeLock();
  if (!lock) return;
  try {
    const held = await lock.request('screen');
    if (gen !== generation) {
      await held.release();
      return;
    }
    sentinel = held;
  } catch {
    // A rejected request (low power mode, no user gesture yet) is retried on the next toggle.
  }
}

async function release(): Promise<void> {
  const host = nativeHost();
  if (host) {
    try {
      await host.allowSleep();
    } catch {
      // Nothing to release when the plugin is absent.
    }
    return;
  }
  const held = sentinel;
  sentinel = null;
  if (!held) return;
  try {
    await held.release();
  } catch {
    // The browser may have released it when the page hid.
  }
}

/**
 * Keep the screen awake while `on`. The app calls this whenever the open workspace, its connection
 * or the page visibility changes; repeated calls with the same value do nothing. In a wrapper the
 * host uses the native keep-awake plugin; in the PWA the Screen Wake Lock API is the web path.
 */
export function setKeepAwake(on: boolean): void {
  if (on === wanted) return;
  wanted = on;
  generation += 1;
  void (on ? acquire(generation) : release());
}
