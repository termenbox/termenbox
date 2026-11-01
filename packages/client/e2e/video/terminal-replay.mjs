// A terminal that replays the real installer's captured output (DECISION-0161). The page is a plain xterm.js terminal; the
// bytes and their timing come from fixtures/installer.{typescript,timing}, captured by scripts/capture-installer-output.sh from
// the real installer in a clean container and scrubbed of every real address. Nothing here runs the installer.
import { launch, contextOptions } from '../browser.mjs';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const here = import.meta.dirname;
const xtermDir = resolve(here, '../../node_modules/@xterm/xterm');
const addonDir = (name) => resolve(here, `../../node_modules/@xterm/addon-${name}/lib/addon-${name}.js`);
const fixtures = resolve(here, 'fixtures');

/** The longest a quiet stretch of the real output is shown for: the installer waits up to eight seconds for its server. */
const MAX_GAP_MS = 4000;

const pageHtml = `<!doctype html><meta charset=utf-8><style>
  html,body{margin:0;height:100%;background:#15171c}
  #win{box-sizing:border-box;margin:0 auto;width:min(100%,760px);height:100%;display:flex;flex-direction:column}
  #bar{height:30px;flex:none;background:#2a2d35;color:#aeb4c0;font:13px/30px system-ui,sans-serif;text-align:center;border-bottom:1px solid #000}
  #term{flex:1;padding:6px 8px;background:#1b1d23}
  #bottom{position:fixed;left:0;bottom:0;width:1px;height:1px}
</style><div id=win><div id=bar>alice@workstation: ~</div><div id=term></div></div><div id=bottom></div>`;

/** Reads the fixtures: the output bytes and the [delayMs, byteCount] chunks they arrived in. */
export function loadCapture(dir = fixtures) {
  const bytes = readFileSync(resolve(dir, 'installer.typescript'));
  const chunks = readFileSync(resolve(dir, 'installer.timing'), 'utf8').split('\n').filter(Boolean).map((l) => {
    const [d, n] = l.split(/\s+/);
    return [Math.round(Number.parseFloat(d) * 1000), Number.parseInt(n, 10)];
  });
  const total = chunks.reduce((a, [, n]) => a + n, 0);
  if (total !== bytes.length) throw new Error(`installer.timing covers ${total} bytes but installer.typescript has ${bytes.length}`);
  return { bytes, chunks };
}

/** A recording context on a page holding an empty terminal; `replay` drives it. Same shape as launchReal/startApp for render.mjs. */
export async function launchReplay({ viewport, scale, phone, recordDir, overlay }) {
  const capture = loadCapture();
  const browser = await launch([]);
  const ctx = await browser.newContext(contextOptions({ viewport, deviceScaleFactor: scale, isMobile: phone, hasTouch: phone, recordVideo: { dir: recordDir, size: viewport } }));
  await ctx.addInitScript(overlay, phone);
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.setContent(pageHtml);
  await page.addStyleTag({ path: resolve(xtermDir, 'css/xterm.css') });
  await page.addScriptTag({ path: resolve(xtermDir, 'lib/xterm.js') });
  await page.addScriptTag({ path: addonDir('fit') });
  await page.addScriptTag({ path: addonDir('webgl') });
  await page.evaluate(() => {
    const term = new window.Terminal({
      cols: 80, rows: 24, fontSize: 12, lineHeight: 1.0, fontFamily: '"DejaVu Sans Mono", "Liberation Mono", monospace',
      cursorBlink: false, scrollback: 500, theme: { background: '#1b1d23', foreground: '#d6dae2', cursor: '#d6dae2' },
    });
    const fit = new window.FitAddon.FitAddon();
    term.loadAddon(fit);
    term.open(document.getElementById('term'));
    // The WebGL renderer draws block characters itself, so the QR has no gaps between rows (the DOM renderer leaves them).
    term.loadAddon(new window.WebglAddon.WebglAddon());
    // Rows fill the window; the 80 columns stay (the installer's output is laid out for them).
    term.resize(80, fit.proposeDimensions().rows);
    window.__term = term;
  });

  const replay = {
    capture,
    /** Shows the shell prompt, as when a terminal has just been opened. */
    prompt: () => page.evaluate(() => window.__term.write('\x1b[32malice@workstation\x1b[0m:\x1b[34m~\x1b[0m$ ')),
    /** Puts `text` on the command line at once, as a paste does. */
    paste: (text) => page.evaluate((t) => window.__term.write(t), text),
    enter: () => page.evaluate(() => window.__term.write('\r\n')),
    /** Plays the captured output with its own timing (quiet stretches shortened to MAX_GAP_MS); `wait` is the pace-aware sleep. */
    async play(wait) {
      let at = 0;
      for (const [delay, n] of capture.chunks) {
        await wait(Math.min(delay, MAX_GAP_MS));
        const b64 = capture.bytes.subarray(at, at + n).toString('base64');
        at += n;
        await page.evaluate((x) => window.__term.write(Uint8Array.from(atob(x), (c) => c.charCodeAt(0))), b64);
      }
      await page.evaluate(() => new Promise((r) => window.__term.write('', r)));
    },
    /** Every line on the terminal, rows that wrapped from the one above joined to it (the pairing link is longer than a row). */
    async lines() {
      return page.evaluate(() => {
        const buf = window.__term.buffer.active;
        const out = [];
        for (let y = 0; y < buf.length; y++) {
          const line = buf.getLine(y);
          const text = line.translateToString(true);
          if (line.isWrapped && out.length) out[out.length - 1] += text; else out.push(text);
        }
        return out;
      });
    },
    /** Polls until some line matches `re` (the terminal parses writes after they are queued); null after `ms`. */
    async waitForLine(re, ms = 5000) {
      const until = Date.now() + ms;
      for (;;) {
        const hit = (await replay.lines()).find((l) => re.test(l));
        if (hit || Date.now() > until) return hit ?? null;
        await new Promise((r) => setTimeout(r, 100));
      }
    },
    /** A zero-size element at the bottom of the page: captions avoid it, so they sit at the top, clear of the pairing block. */
    bottom: () => page.locator('#bottom'),
  };
  return { page, ctx, errors, replay, stop: async () => { await browser.close().catch(() => {}); } };
}
