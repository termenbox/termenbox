// Builds the deployable site into site-dist (git-ignored): the landing page as it is, the approved videos and posters in
// /media/, and the course (docs/course/*.md) as pages under /course/ with a real <video> player in place of each media
// block (GitHub plays no repository MP4 inline). The repository stays the source. Run: node scripts/build-site.mjs [outDir] | --self-test
import { readFileSync, writeFileSync, mkdirSync, rmSync, cpSync, readdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const REPO = 'https://github.com/termenbox/termenbox/blob/master/docs';
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** A chapter link or a guide link, as the page it should open on the site. */
export function href(url) {
  if (/^(https?:|#|mailto:)/.test(url)) return url;
  const chapter = /^(\d\d-[a-z0-9-]+)\.md(#.*)?$/.exec(url);
  if (chapter) return `/course/${chapter[1]}/${chapter[2] ?? ''}`;
  if (url === 'README.md') return '/course/';
  const guide = /^\.\.\/(guide\/[a-z0-9-]+\.md)(#.*)?$/.exec(url);
  if (guide) return `${REPO}/${guide[1]}${guide[2] ?? ''}`;
  throw new Error(`link not mapped to the site: ${url}`);
}

const inline = (s) => esc(s)
  .replace(/`([^`]+)`/g, '<code>$1</code>')
  .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  .replace(/\*([^*]+)\*/g, '<em>$1</em>')
  .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_m, text, url) => `<a href="${href(url.replace(/&amp;/g, '&'))}">${text}</a>`);

/** The player that replaces a media block: the video, its poster and its length. */
function player(id, body) {
  const stem = id.replace(/^(.*?)(-phone|-desktop)?$/, (_m, c, p) => `${c}${p ?? '-phone'}`);
  const seconds = /Watch, (\d+) s/.exec(body)?.[1];
  const label = /\*([^*]+)\*\s*$/.exec(body.trim())?.[1] ?? '';
  return `<figure class="player"><video controls playsinline preload="metadata" poster="/media/${stem}-poster.png"><source src="/media/${stem}.mp4" type="video/mp4"><a href="/media/${stem}.mp4">Watch${seconds ? `, ${seconds} s` : ''}</a></video>` +
    `<figcaption>${seconds ? `${seconds} s` : ''}${label ? `. ${esc(label)}` : ''}</figcaption></figure>`;
}

/** Markdown of a course page to HTML: headings, paragraphs, lists, links, code and the media blocks. */
export function render(md) {
  const out = [];
  const lines = md.replace(/<!-- media:([a-z0-9-]+) -->([\s\S]*?)<!-- \/media -->/g, (_m, id, body) => `\n@@PLAYER ${id}\n${player(id, body)}\n@@END\n`).split('\n');
  let title = '', list = null, para = [], table = [];
  const flush = () => {
    if (para.length) out.push(`<p>${inline(para.join(' '))}</p>`);
    para = [];
    if (table.length) {
      const rows = table.filter((r) => !/^\|[-| ]+\|$/.test(r)).map((r) => r.slice(1, -1).split('|').map((c) => inline(c.trim())));
      out.push(`<div class="table"><table><thead><tr>${rows[0].map((c) => `<th>${c}</th>`).join('')}</tr></thead><tbody>${rows.slice(1).map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`);
      table = [];
    }
  };
  const close = () => { flush(); if (list) { out.push(`</${list}>`); list = null; } };
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    if (l.startsWith('@@PLAYER ')) { close(); out.push(lines[++i]); i++; continue; }
    if (/^<!--.*-->$/.test(l.trim())) continue;
    const h = /^(#{1,3}) (.*)$/.exec(l);
    const li = /^(?:(\d+)\.|-) (.*)$/.exec(l);
    if (h) { close(); if (h[1] === '#') { title = h[2]; out.push(`<h1>${inline(h[2])}</h1>`); } else out.push(`<h${h[1].length}>${inline(h[2])}</h${h[1].length}>`); }
    else if (li) { flush(); const kind = li[1] ? 'ol' : 'ul'; if (list !== kind) { close(); list = kind; out.push(`<${kind}>`); } out.push(`<li>${inline(li[2])}</li>`); }
    else if (l.startsWith('|')) { if (list) close(); table.push(l); }
    else if (!l.trim()) close();
    else { if (list) close(); para.push(l.trim()); }
  }
  close();
  return { title, html: out.join('\n') };
}

const page = (title, description, path, body) => `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${esc(title)} | Termenbox course</title>
  <meta name="description" content="${esc(description)}">
  <link rel="canonical" href="https://termenbox.dev${path}">
  <link rel="icon" href="/favicon.svg" type="image/svg+xml">
  <link rel="stylesheet" href="/site.css">
</head>
<body>
  <header class="top">
    <a class="brand" href="/">Termenbox</a>
    <nav aria-label="Main"><a href="/course/">Course</a><a class="cta" href="https://app.termenbox.dev">Open the app</a></nav>
  </header>
  <main class="course">
${body}
  </main>
</body>
</html>
`;

export function build(out) {
  rmSync(out, { recursive: true, force: true });
  cpSync(join(root, 'packages/site'), out, { recursive: true});
  const media = join(root, 'docs/media');
  mkdirSync(join(out, 'media'), { recursive: true });
  for (const f of readdirSync(media)) if (/\.mp4$|-poster\.png$/.test(f)) cpSync(join(media, f), join(out, 'media', f));
  const sitemap = ['https://termenbox.dev/', 'https://termenbox.dev/course/'];
  for (const f of readdirSync(join(root, 'docs/course')).filter((n) => n.endsWith('.md'))) {
    const md = readFileSync(join(root, 'docs/course', f), 'utf8');
    const { title, html } = render(md);
    for (const m of html.matchAll(/(?:src|poster|href)="\/media\/([^"]+)"/g)) assert.ok(existsSync(join(out, 'media', m[1])), `${f}: /media/${m[1]} is not in docs/media`);
    const slug = f === 'README.md' ? '' : f.replace(/\.md$/, '/');
    const lead = /<\/h1>\s*<p>([^<]+)/.exec(html)?.[1] ?? title;
    mkdirSync(join(out, 'course', slug), { recursive: true });
    writeFileSync(join(out, 'course', slug, 'index.html'), page(title, lead.slice(0, 155), `/course/${slug}`, html));
    if (slug) sitemap.push(`https://termenbox.dev/course/${slug}`);
  }
  writeFileSync(join(out, 'sitemap.xml'), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${sitemap.map((u) => `  <url><loc>${u}</loc></url>`).join('\n')}\n</urlset>\n`);
}

function selfTest() {
  assert.equal(href('02-connect-your-phone.md'), '/course/02-connect-your-phone/');
  assert.equal(href('README.md'), '/course/');
  assert.ok(href('../guide/install.md#firewalls').endsWith('/docs/guide/install.md#firewalls'));
  assert.throws(() => href('../elsewhere.md'), /not mapped/);
  const r = render('# T\n\nHi **b** [x](01-a.md)\n\n<!-- media:install-desktop -->\n[![W](../media/install-desktop-poster.png)](../media/install-desktop.mp4)\n\nWatch, 32 s\n\n*Scripted.*\n<!-- /media -->\n\n1. one\n2. two\n');
  assert.equal(r.title, 'T');
  assert.match(r.html, /<video controls playsinline preload="metadata" poster="\/media\/install-desktop-poster.png">/);
  assert.match(r.html, /<ol>\n<li>one<\/li>\n<li>two<\/li>\n<\/ol>/);
  assert.ok(!r.html.includes('.md'), 'no link keeps its .md');
  console.log('ok  build-site self-test');
}

const arg = process.argv[2];
if (arg === '--self-test') selfTest();
else if (process.argv[1] === fileURLToPath(import.meta.url)) { const out = join(root, arg ?? 'site-dist'); build(out); console.log(`ok  site built in ${out}`); }
