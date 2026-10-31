/** What a native wrapper injects before the client bundle runs. Each capability is optional: a host offers only what it implements. */
export interface NativeHost {
  /** Hold the screen on while the user watches a workspace. */
  keepAwake?(): Promise<void>;
  /** Let the screen sleep again. */
  allowSleep?(): Promise<void>;
  /** Install the back handler, or null to clear it. Return true when the press was consumed. */
  onBack?(handler: (() => boolean) | null): void;
}

/**
 * The wrapper host when this page runs inside the native shell, otherwise null. The wrapper sets
 * `globalThis.__termenboxPlatform` with a classic script before the module bundle, so no native
 * import ever reaches the web client (guardrail `native-wrapper-keeps-web-parity`).
 */
export function nativeHost(): NativeHost | null {
  const host = (globalThis as { __termenboxPlatform?: NativeHost }).__termenboxPlatform;
  return host && typeof host === 'object' ? host : null;
}
