import { App } from '@capacitor/app';
import { KeepAwake } from '@capacitor-community/keep-awake';

type BackHandler = (() => boolean) | null;

let onBack: BackHandler = null;
let awake = false;

/**
 * The bridge the client reads as `globalThis.__termenboxPlatform`. It is bundled as a classic
 * script and injected into the copied client build, so the client keeps no native import.
 */
const host = {
  async keepAwake(): Promise<void> {
    if (awake) return;
    await KeepAwake.keepAwake();
    awake = true;
  },
  async allowSleep(): Promise<void> {
    if (!awake) return;
    await KeepAwake.allowSleep();
    awake = false;
  },
  onBack(handler: BackHandler): void {
    onBack = handler;
  },
};

(globalThis as { __termenboxPlatform?: typeof host }).__termenboxPlatform = host;

void App.addListener('backButton', () => {
  if (onBack?.()) return;
  // No screen consumed the press: leave the app instead of trapping the user.
  void App.minimizeApp().catch(() => {
    // minimizeApp is Android-only; the listener never fires on iOS.
  });
}).catch(() => {
  // Without the App plugin there is no back event to handle.
});
