// The one place that decides which browser, viewport and pixel ratio an e2e script runs in, and what it keeps on failure.
//   E2E_ENGINE     chromium (default) | firefox | webkit
//   E2E_VIEWPORT   WxH for the scripts that take it (default per script), e.g. 1024x768
//   E2E_DPR        device pixel ratio for the scripts that take it (default 1)
//   E2E_ARTIFACTS  directory: a failed or crashed scenario leaves <engine>-<label>.png and .zip (a Playwright trace) there
//   CHROME_PATH    Chrome/Chromium binary (default: Playwright's "chrome" channel); Firefox and WebKit come from
//                  `playwright-core install firefox webkit`
import { chromium, firefox, webkit } from 'playwright-core';
import { appendFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const engines = { chromium, firefox, webkit };
export const ENGINE = process.env.E2E_ENGINE ?? 'chromium';
if (!engines[ENGINE]) throw new Error(`E2E_ENGINE must be one of ${Object.keys(engines).join(', ')}, not "${ENGINE}"`);
export const DPR = Number(process.env.E2E_DPR ?? 1);
const ARTIFACTS = process.env.E2E_ARTIFACTS ?? '';

/** `{ width, height }` from E2E_VIEWPORT, or from `fallback` ("1280x800"). */
export function viewport(fallback) {
  const [width, height] = (process.env.E2E_VIEWPORT ?? fallback).split('x').map(Number);
  return { width, height };
}

let announced = false;

/** Launches the selected engine and says which one, so a log or the CI summary names the engine and version. `args` are Chromium flags. */
export async function launch(args = []) {
  const options = ENGINE === 'chromium'
    ? { ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'chrome' }), args: ['--no-sandbox', ...args] }
    : {};
  const browser = await engines[ENGINE].launch(options);
  if (!announced) {
    announced = true;
    const line = `${ENGINE} ${browser.version()}, viewport ${process.env.E2E_VIEWPORT ?? 'default'}, dpr ${DPR}`;
    console.log(`browser: ${line}`);
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `- ${process.argv[1].split('/').pop()}: ${line}\n`);
  }
  return browser;
}

/** Context options for the selected engine: the pixel ratio applied, service workers blocked (no script tests the PWA, and
 *  Playwright's WebKit freezes the page while one registers), and what Firefox does not support dropped. */
export function contextOptions(options = {}) {
  const out = { deviceScaleFactor: DPR, serviceWorkers: 'block', ...options };
  if (ENGINE === 'firefox') delete out.isMobile;
  return out;
}

let live = null; // the scenario in progress: what a crash should save

async function save(ctx, label) {
  const base = join(ARTIFACTS, `${ENGINE}-${label}`);
  mkdirSync(ARTIFACTS, { recursive: true });
  for (const [i, page] of ctx.pages().entries()) await page.screenshot({ path: `${base}-${i}.png` }).catch(() => {});
  await ctx.tracing.stop({ path: `${base}.zip` }).catch(() => {});
}

/** A tap on a locator. Playwright's WebKit and Firefox cannot deliver touch input (a tap times out), so there it is a click, which
 *  gives the same pointer events; only Chromium proves touch. */
export const tap = (locator, options) => (ENGINE === 'chromium' ? locator.tap(options) : locator.click(options));

/** Starts a trace on the context when artifacts are wanted, and saves it if the script dies with an exception mid-scenario. Pair with `keepIfFailed`. */
export async function trace(ctx, label) {
  if (!ARTIFACTS) return;
  await ctx.tracing.start({ screenshots: true, snapshots: true });
  live = { ctx, label };
  if (trace.armed) return;
  trace.armed = true;
  for (const event of ['uncaughtException', 'unhandledRejection']) {
    process.on(event, async (e) => {
      console.error(e);
      if (live) await save(live.ctx, live.label);
      process.exit(1);
    });
  }
}

/** Ends the trace; when the scenario failed, saves a screenshot of each page and the trace under E2E_ARTIFACTS. */
export async function keepIfFailed(ctx, label, failed) {
  if (!ARTIFACTS) return;
  live = null;
  if (failed) await save(ctx, label);
  else await ctx.tracing.stop();
}
