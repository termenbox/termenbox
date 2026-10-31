import { nativeHost } from './platform';

let wanted = false;

/**
 * Keep the screen awake while `on`, in a wrapper only: the browser has no such outcome to preserve
 * (the PWA leaves the screen to the OS). The app calls this whenever the open workspace, its
 * connection or the page visibility changes; repeated calls with the same value do nothing.
 */
export function setKeepAwake(on: boolean): void {
  if (on === wanted) return;
  wanted = on;
  const host = nativeHost();
  const call = on ? host?.keepAwake : host?.allowSleep;
  // The wrapper may not expose the keep-awake plugin; the screen then sleeps as usual.
  void call?.call(host)?.catch(() => {});
}
