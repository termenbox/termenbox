// A probe page on the recorder: the built e2e/<probe>.html (pnpm run e2e:build), a template on its fixtures with no app and no
// server. Same shape as launchReal/launchReplay/startApp for render.mjs; `server` and `replay` are absent, so a case drives `page` only.
import { contextOptions, launch } from '../browser.mjs';
import { createServer } from 'node:http';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { extname, join } from 'node:path';

const dist = join(import.meta.dirname, '../dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' };

export async function startProbe(_shared, probe, { viewport, scale, phone, recordDir, overlay }) {
  if (!existsSync(join(dist, `${probe}.html`))) throw new Error(`e2e/dist/${probe}.html is missing: run pnpm run e2e:build`);
  const srv = createServer((req, res) => {
    const f = join(dist, req.url.split('?')[0].split('#')[0]);
    if (!f.startsWith(dist) || !existsSync(f) || statSync(f).isDirectory()) return res.writeHead(404).end();
    res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  const browser = await launch();
  const ctx = await browser.newContext(contextOptions({ reducedMotion: 'reduce', viewport, deviceScaleFactor: scale, isMobile: phone, hasTouch: phone, recordVideo: { dir: recordDir, size: viewport } }));
  await ctx.addInitScript(overlay, phone);
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.goto(`http://127.0.0.1:${srv.address().port}/${probe}.html`);
  await page.waitForSelector('[data-fleet-needs-you]');
  return { ctx, page, errors, stop: async () => { await browser.close(); srv.close(); } };
}
