// Shared browser launch configuration for e2e tests.
// Supports Chromium, Firefox, and WebKit with desktop and mobile contexts.

import { chromium, firefox, webkit } from 'playwright-core';

export const BROWSER_ENGINES = ['chromium', 'firefox', 'webkit'];
export const DESKTOP_VIEWPORTS = [
  { name: '1024x768', width: 1024, height: 768 },
  { name: '1440x900', width: 1440, height: 900 },
];
export const MOBILE_VIEWPORTS = [
  { name: 'mobile-390x800', width: 390, height: 800 },
];
export const DPR_VALUES = [1, 2];

export function getBrowser(engine, options = {}) {
  const launchOptions = {
    args: [
      '--no-sandbox',
      '--disable-features=WebRtcHideLocalIpsWithMdns',
      '--allow-loopback-in-peer-connection',
      '--disable-crash-reporter',
      '--disable-breakpad',
      '--no-crash-upload',
    ],
    ...options,
  };

  switch (engine) {
    case 'chromium':
      return chromium.launch({
        ...launchOptions,
        ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'chromium' }),
      });
    case 'firefox':
      return firefox.launch({
        ...launchOptions,
        ...(process.env.FIREFOX_PATH ? { executablePath: process.env.FIREFOX_PATH } : {}),
      });
    case 'webkit':
      return webkit.launch({
        ...launchOptions,
        ...(process.env.WEBKIT_PATH ? { executablePath: process.env.WEBKIT_PATH } : {}),
      });
    default:
      throw new Error(`Unknown browser engine: ${engine}`);
  }
}

export function getContextOptions(viewport, isMobile = false, hasTouch = false, dpr = 1) {
  return {
    viewport,
    isMobile,
    hasTouch,
    deviceScaleFactor: dpr,
    permissions: ['clipboard-read', 'clipboard-write'],
    ...(isMobile ? { userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 16_4 like Mac OS X) AppleWebKit/605.1.15' } : {}),
  };
}

export function makeDesktopContexts(browser, dpr = 1) {
  return DESKTOP_VIEWPORTS.map(({ name, width, height }) => ({
    name: `desktop-${name}-dpr${dpr}`,
    context: browser.newContext(getContextOptions({ width, height }, false, false, dpr)),
  }));
}

export function makeMobileContexts(browser, dpr = 1) {
  return MOBILE_VIEWPORTS.map(({ name, width, height }) => ({
    name: `mobile-${name}-dpr${dpr}`,
    context: browser.newContext(getContextOptions({ width, height }, true, true, dpr)),
  }));
}

export async function runForEachEngine(fn) {
  const engines = process.env.E2E_ENGINES?.split(',') ?? BROWSER_ENGINES;
  for (const engine of engines) {
    console.log(`\n=== Testing with ${engine} ===`);
    const browser = await getBrowser(engine);
    try {
      await fn(engine, browser);
    } finally {
      await browser.close().catch(() => {});
    }
  }
}

export async function runForEachContext(engine, browser, fn) {
  // In CI, run only mobile context by default (like original run.mjs); full matrix on demand via E2E_FULL_MATRIX=1
  const fullMatrix = process.env.E2E_FULL_MATRIX === '1';
  const dprs = fullMatrix ? DPR_VALUES : [1];

  if (fullMatrix) {
    // Desktop contexts at all DPRs
    for (const dpr of dprs) {
      for (const { name, context: ctxPromise } of makeDesktopContexts(browser, dpr)) {
        const context = await ctxPromise;
        try {
          await fn(engine, context, { viewport: 'desktop', width: name.split('x')[0], height: name.split('x')[1], dpr });
        } finally {
          await context.close().catch(() => {});
        }
      }
    }
  }

  // Mobile contexts (always run, DPR 1 in CI, both in full matrix)
  for (const dpr of dprs) {
    for (const { name, context: ctxPromise } of makeMobileContexts(browser, dpr)) {
      const context = await ctxPromise;
      try {
        await fn(engine, context, { viewport: 'mobile', width: name.split('-')[1].split('x')[0], height: name.split('x')[1], dpr });
      } finally {
        await context.close().catch(() => {});
      }
    }
  }
}