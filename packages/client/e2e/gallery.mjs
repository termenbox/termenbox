// The component gallery in real Chrome at 390 px, every skin in dark and light: no console error, the audit rules
// from docs/plans/assets/ux/DESIGN-PRINCIPLES.md that apply to components (DR1 target size, DR2 spacing, DR5 type scale,
// DR8 contrast, DR15 overflow, DR9 state words), and a screenshot per skin and mode.
// Usage: node e2e/gallery.mjs   (needs `pnpm e2e:build`; SHOTS=dir for screenshots, CHROME_PATH for the browser)
import { launch } from './browser.mjs';
import { createServer } from 'node:http';
import { readFileSync, existsSync, mkdirSync } from 'node:fs';
import { join, extname, resolve } from 'node:path';

const here = import.meta.dirname;
const dist = join(here, 'dist');
const SHOTS = process.env.SHOTS ?? join(dist, 'gallery-shots');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const server = createServer((req, res) => {
  const file = join(dist, req.url.split('?')[0]);
  if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/gallery.html`;

const fails = new Map();
const add = (rule, msg) => { if (!fails.has(rule)) fails.set(rule, new Set()); fails.get(rule).add(msg); };

function probe() {
  const vis = (e) => { const r = e.getBoundingClientRect(), cs = getComputedStyle(e); return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && cs.display !== 'none'; };
  const name = (e) => (e.getAttribute('aria-label') || e.textContent || e.tagName).trim().replace(/\s+/g, ' ').slice(0, 28);
  const parse = (c) => { const m = c.match(/rgba?\(([^)]+)\)|color\(srgb ([^)]+)\)/); if (!m) return [0, 0, 0, 0]; const v = (m[1] ?? m[2]).split(/[ ,/]+/).filter(Boolean).map(Number); return m[2] ? [v[0] * 255, v[1] * 255, v[2] * 255, v.length > 3 ? v[3] : 1] : [v[0], v[1], v[2], v.length > 3 ? v[3] : 1]; };
  const over = (f, b) => [f[0] * f[3] + b[0] * (1 - f[3]), f[1] * f[3] + b[1] * (1 - f[3]), f[2] * f[3] + b[2] * (1 - f[3]), 1];
  const lum = ([r, g, b]) => { const f = (x) => { x /= 255; return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b); };
  const bgOf = (e) => { const st = []; for (let n = e; n; n = n.parentElement) { const c = parse(getComputedStyle(n).backgroundColor); if (c[3] > 0) st.push(c); if (c[3] === 1) break; } let bg = [0, 0, 0, 1]; for (const c of st.reverse()) bg = over(c, bg); return bg; };
  const targets = [...document.querySelectorAll('button, a[href], input, [role=tab]')].filter(vis);
  const small = targets.filter((e) => { const r = e.getBoundingClientRect(); return r.width < 43.5 || r.height < 43.5; }).map((e) => { const r = e.getBoundingClientRect(); return `${name(e)} ${Math.round(r.width)}x${Math.round(r.height)}`; });
  const gaps = [];
  for (let i = 0; i < targets.length; i++) for (let j = i + 1; j < targets.length; j++) {
    const a = targets[i], c = targets[j]; if (a.contains(c) || c.contains(a)) continue;
    const ra = a.getBoundingClientRect(), rc = c.getBoundingClientRect();
    const dx = Math.max(rc.left - ra.right, ra.left - rc.right, 0), dy = Math.max(rc.top - ra.bottom, ra.top - rc.bottom, 0);
    const g = dx > 0 && dy > 0 ? Math.hypot(dx, dy) : Math.max(dx, dy);
    if (g < 3.5) gaps.push(`${name(a)} | ${name(c)} ${g.toFixed(1)}`);
    else if (g < 7.5 && !(a.closest('form') && a.closest('form') === c.closest('form'))) gaps.push(`${name(a)} | ${name(c)} ${g.toFixed(1)}`);
  }
  const contrast = [], sizes = new Set();
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let t; (t = walker.nextNode());) {
    if (!t.nodeValue.trim()) continue; const e = t.parentElement; if (!vis(e)) continue;
    const cs = getComputedStyle(e); const bg = bgOf(e); const fg = over(parse(cs.color), bg);
    const L1 = lum(fg), L2 = lum(bg); const ratio = (Math.max(L1, L2) + 0.05) / (Math.min(L1, L2) + 0.05);
    const px = parseFloat(cs.fontSize); if (!e.closest('[role=img]')) sizes.add(px); // icon glyphs are images, not reading text
    const need = px >= 24 || (px >= 18.66 && Number(cs.fontWeight) >= 700) ? 3 : 4.5;
    if (ratio < need) contrast.push(`"${t.nodeValue.trim().slice(0, 22)}" ${ratio.toFixed(2)} < ${need} (${px}px)`);
  }
  const smallInputs = [...document.querySelectorAll('input')].filter(vis).filter((e) => parseFloat(getComputedStyle(e).fontSize) < 16).length;
  const unnamed = targets.filter((e) => !(e.getAttribute('aria-label') || e.textContent.trim())).length;
  return { small, gaps, contrast, sizes: [...sizes], smallInputs, unnamed, overflow: document.documentElement.scrollWidth - innerWidth, errorsOnPage: document.querySelectorAll('[data-error]').length,
    stateWords: ['Needs you', 'Working', 'Failed', 'Finished', 'Idle'].filter((w) => !document.body.textContent.includes(w)) };
}

const browser = await launch();
const page = await browser.newPage({ viewport: { width: 390, height: 1800 }, deviceScaleFactor: 2, hasTouch: true });
const consoleErrors = [];
page.on('console', (m) => { if (m.type() === 'error') consoleErrors.push(m.text()); });
page.on('pageerror', (e) => consoleErrors.push(String(e)));
mkdirSync(SHOTS, { recursive: true });

const SIZES = new Set([12, 13, 15, 16, 22]);
for (const skin of ['navy', 'graphite', 'teal']) for (const theme of ['dark', 'light']) {
  await page.goto(url);
  await page.evaluate(([s, t]) => { document.documentElement.dataset.skin = s; document.documentElement.dataset.theme = t; }, [skin, theme]);
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400); // the global 150 ms button background transition must finish before contrast is read
  const r = await page.evaluate(probe);
  const tag = `${skin}/${theme}`;
  r.small.forEach((m) => add('DR1 target under 44 pt', `${tag} ${m}`));
  r.gaps.forEach((m) => add('DR2 gap under 8 pt', `${tag} ${m}`));
  r.contrast.forEach((m) => add('DR8 text contrast', `${tag} ${m}`));
  r.sizes.filter((s) => !SIZES.has(s)).forEach((s) => add('DR5 font size off the scale', `${tag} ${s}px`));
  if (r.smallInputs) add('DR5 input under 16 px', tag);
  if (r.unnamed) add('DR7 control without a name', `${tag} ${r.unnamed}`);
  if (r.overflow > 0) add('DR15 horizontal overflow', `${tag} ${r.overflow}px`);
  r.stateWords.forEach((w) => add('DR9 state without its word', `${tag} ${w}`));
  await page.screenshot({ path: join(SHOTS, `gallery-${skin}-${theme}-390.png`), fullPage: true });
}
await browser.close();
server.close();

if (consoleErrors.length) add('console error', [...new Set(consoleErrors)].join(' | '));
if (fails.size) {
  for (const [rule, msgs] of fails) { console.error(`FAIL ${rule} (${msgs.size})`); [...msgs].slice(0, 8).forEach((m) => console.error('   ' + m)); }
  process.exitCode = 1;
} else console.log(`PASS gallery: 3 skins x 2 modes at 390 px, screenshots in ${SHOTS}`);
