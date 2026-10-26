/** What a native wrapper injects before the client bundle runs. */
export interface NativeHost {
  /** Hold the screen on while the user watches a workspace. */
  keepAwake(): Promise<void>;
  /** Let the screen sleep again. */
  allowSleep(): Promise<void>;
  /** Install the back handler, or null to clear it. Return true when the press was consumed. */
  onBack(handler: (() => boolean) | null): void;
}

type MaybeHost = Partial<NativeHost> | undefined;

/**
 * The wrapper host when this page runs inside the native shell, otherwise null. The wrapper sets
 * `globalThis.__termenboxPlatform` with a classic script before the module bundle, so no native
 * import ever reaches the web client (guardrail `native-wrapper-keeps-web-parity`).
 */
export function nativeHost(): NativeHost | null {
  const host = (globalThis as { __termenboxPlatform?: MaybeHost }).__termenboxPlatform;
  if (!host) return null;
  if (typeof host.keepAwake !== 'function' || typeof host.allowSleep !== 'function' || typeof host.onBack !== 'function') return null;
  return host as NativeHost;
}
