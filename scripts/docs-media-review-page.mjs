// Writes index.html beside a render's files: one card per video with its stills and metadata, for the owner to review on a
// phone (DECISION-0158). Plain HTML and CSS, no script. Run: node scripts/docs-media-review-page.mjs <dir> | --self-test
import { readdirSync, readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const LABEL = 'Rendered walkthrough on scripted data, not a recording of a real device or agent.';

const CSS = `:root{color-scheme:light dark;--bg:#fff;--fg:#14171a;--mut:#5b6670;--card:#f3f5f7;--line:#d5dbe0}
@media (prefers-color-scheme:dark){:root{--bg:#101316;--fg:#e8ecef;--mut:#99a4ad;--card:#191e22;--line:#2b3238}}
*{box-sizing:border-box}body{margin:0;padding:16px;background:var(--bg);color:var(--fg);font:16px/1.45 system-ui,sans-serif;overflow-wrap:anywhere}
main{max-width:720px;margin:0 auto}h1{font-size:1.3rem;margin:0 0 4px}p{margin:0 0 12px;color:var(--mut)}
article{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:12px;margin:0 0 16px}
h2{font-size:1.05rem;margin:0 0 8px}video{display:block;width:100%;max-height:70vh;background:#000;border-radius:8px}
dl{display:grid;grid-template-columns:auto 1fr;gap:2px 12px;margin:10px 0 0;font-size:.85rem;color:var(--mut)}dd{margin:0;font-family:ui-monospace,monospace}
.stills{display:flex;gap:8px;margin-top:10px}.stills img{flex:1 1 0;min-width:0;max-width:50%;border-radius:6px;border:1px solid var(--line)}`;

/** The cards' source: every `<case>-<profile>.render.json` in `dir`, sorted by case then profile. */
function readRenders(dir) {
  return readdirSync(dir).filter((f) => f.endsWith('.render.json')).sort()
    .map((f) => ({ base: f.slice(0, -'.render.json'.length), ...JSON.parse(readFileSync(join(dir, f), 'utf8')) }));
}

export function buildPage(dir, { title = 'Docs media review' } = {}) {
  const cards = readRenders(dir).map((r) => {
    const stills = (r.files ?? []).filter((f) => f.endsWith('.png'));
    return `<article id="${esc(r.base)}">
<h2>${esc(r.case)} <small>(${esc(r.profile)})</small></h2>
<video controls playsinline muted loop preload="metadata" poster="${esc(r.poster ?? stills[0] ?? '')}" src="${esc(r.base)}.mp4"></video>
${stills.length ? `<div class="stills">${stills.map((s) => `<a href="${esc(s)}"><img src="${esc(s)}" alt="${esc(s)}" loading="lazy"></a>`).join('')}</div>` : ''}
<dl><dt>input hash</dt><dd>${esc(r.hash)}</dd><dt>pace</dt><dd>${esc(r.pace)}</dd><dt>captions</dt><dd>${r.captions === false ? 'off' : 'on'}</dd><dt>version</dt><dd>${esc(r.version)}</dd><dt>rendered</dt><dd>${esc(r.renderedAt)}</dd></dl>
</article>`;
  });
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="robots" content="noindex"><title>${esc(title)}</title><style>${CSS}</style></head>
<body><main><h1>${esc(title)}</h1><p>${LABEL}</p>
${cards.join('\n') || '<p>No renders in this folder.</p>'}
</main></body></html>
`;
}

function selfTest() {
  const dir = mkdtempSync(join(tmpdir(), 'tb-review-'));
  try {
    for (const [c, p] of [['add-server', 'phone'], ['recent-folder', 'desktop']]) {
      const base = `${c}-${p}`;
      writeFileSync(join(dir, `${base}.render.json`), JSON.stringify({ case: c, profile: p, pace: 'normal', captions: true, hash: 'ab12'.repeat(4), version: '1.0.0', renderedAt: '2026-10-31T00:00:00Z', files: [`${base}.mp4`, `${base}-poster.png`, `${base}-step.png`], poster: `${base}-poster.png` }));
    }
    writeFileSync(join(dir, 'x-phone.render.json'), JSON.stringify({ case: '<script>alert(1)</script>', profile: 'phone', hash: 'h', files: [] }));
    const html = buildPage(dir);
    assert.equal((html.match(/<article /g) ?? []).length, 3, 'a card per render');
    assert.ok(html.includes(LABEL), 'the scripted-data label');
    assert.ok(!/<script/i.test(html), 'no script at all, and hostile text is escaped');
    assert.ok(html.includes('&lt;script&gt;'));
    assert.ok(html.includes('name="robots" content="noindex"'));
    assert.ok(html.includes('prefers-color-scheme:dark') && html.includes('width=device-width'));
    assert.ok(html.includes('poster="add-server-phone-poster.png"'), 'the poster is the video poster');
    assert.ok(html.indexOf('add-server-phone') < html.indexOf('recent-folder-desktop'), 'sorted');
  } finally { rmSync(dir, { recursive: true, force: true }); }
  console.log('ok  docs-media-review-page self-test');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const arg = process.argv[2];
  if (arg === '--self-test') selfTest();
  else if (arg) { writeFileSync(join(arg, 'index.html'), buildPage(arg)); console.log(`ok  ${join(arg, 'index.html')}`); }
  else { console.error('usage: docs-media-review-page.mjs <dir> | --self-test'); process.exit(2); }
}
