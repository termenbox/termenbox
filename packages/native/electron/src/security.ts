// Security decisions for the Electron wrapper, kept free of Electron imports so they can be tested
// without a window: origin matching for navigation and the Content-Security-Policy header.

/**
 * True when `url` uses exactly the wrapper's own scheme. The template used `url.includes(scheme)`,
 * which also matches `capacitor-electron-evil://…` and `https://capacitor-electron.example/`.
 */
export function isOwnUrl(url: string, customScheme: string): boolean {
  try {
    return new URL(url).protocol === `${customScheme}:`;
  } catch {
    return false;
  }
}

/**
 * The packaged app gets the web client's policy (`packages/client/public/_headers`) with the custom
 * scheme in place of `'self'`. `'unsafe-inline'` stays for styles only: the client sets style
 * attributes on dozens of elements, while its scripts are separate files that Vite emits.
 */
export function contentSecurityPolicy(customScheme: string, dev: boolean): string {
  const app = `${customScheme}://*`;
  if (dev) {
    // The live runner loads http://localhost:3000 and needs HMR over a WebSocket.
    return [
      `default-src ${app} http://localhost:* ws://localhost:* data: blob: 'unsafe-inline' 'unsafe-eval'`,
      `script-src ${app} http://localhost:* 'unsafe-inline' 'unsafe-eval'`,
      `connect-src ${app} http://localhost:* ws://localhost:* wss: https:`,
      `img-src ${app} http://localhost:* data: blob: https:`,
    ].join('; ');
  }
  return [
    `default-src ${app}`,
    `script-src ${app}`,
    `style-src ${app} 'unsafe-inline'`,
    `img-src ${app} data: blob: https://avatars.githubusercontent.com`,
    `font-src ${app} data:`,
    // WebRTC is not covered by connect-src; the WebSocket fallback and the assisted signaling path
    // are, and both are `wss:` (the web client's `_headers` policy allows the same).
    `connect-src ${app} wss:`,
    `media-src ${app} blob:`,
    `worker-src ${app} blob:`,
    `manifest-src ${app}`,
    `object-src 'none'`,
    `base-uri 'none'`,
    `form-action 'none'`,
    `frame-ancestors 'none'`,
  ].join('; ');
}
