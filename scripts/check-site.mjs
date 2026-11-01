// Checks the landing page the way a crawler and a visitor would meet it: the metadata is present and sized, the
// structured data parses and matches the visible text, every local asset exists, and the install command is the
// one the app and the README show. Run: node scripts/check-site.mjs
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const here = join(root, 'packages/site');
const html = readFileSync(join(here, 'index.html'), 'utf8');
const errors = [];
const need = (ok, msg) => { if (!ok) errors.push(msg); };

const title = /<title>([^<]+)<\/title>/.exec(html)?.[1] ?? '';
need(title.length >= 20 && title.length <= 60, `title is ${title.length} characters (20-60)`);
const desc = /<meta name="description" content="([^"]+)"/.exec(html)?.[1] ?? '';
need(desc.length >= 70 && desc.length <= 160, `description is ${desc.length} characters (70-160)`);
need(/<link rel="canonical" href="https:\/\/termenbox\.dev\/">/.test(html), 'canonical link to https://termenbox.dev/ is missing');
need((html.match(/<h1[ >]/g) ?? []).length === 1, 'the page needs exactly one h1');
need(/<html lang="en">/.test(html), 'html lang is missing');
for (const p of ['og:title', 'og:description', 'og:url', 'og:type']) need(html.includes(`property="${p}"`), `missing ${p}`);

// Only the JSON-LD data block may be inline; scripts are external so the CSP can stay script-src 'self'.
for (const m of html.matchAll(/<script([^>]*)>/g)) need(/type="application\/ld\+json"|src="/.test(m[1]), `inline script: <script${m[1]}>`);

const ld = JSON.parse(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/.exec(html)?.[1] ?? 'null');
const faq = ld?.['@graph']?.find((n) => n['@type'] === 'FAQPage');
need(Array.isArray(faq?.mainEntity) && faq.mainEntity.length > 0, 'FAQPage structured data is missing');
const summaries = [...html.matchAll(/<summary>([^<]+)<\/summary>/g)].map((m) => m[1]);
for (const q of faq?.mainEntity ?? []) need(summaries.includes(q.name), `structured question not visible on the page: ${q.name}`);

for (const m of html.matchAll(/(?:href|src)="(\/[^"#]*)"/g)) {
  if (m[1].startsWith('/course/')) continue; // built from docs/course by scripts/build-site.mjs
  const file = m[1] === '/' ? 'index.html' : m[1].slice(1);
  need(existsSync(join(here, file)), `local asset missing: ${m[1]}`);
}
for (const f of ['robots.txt', 'sitemap.xml', '_headers']) need(existsSync(join(here, f)), `${f} is missing`);
// Cloudflare merges every matching _headers rule and a browser applies each CSP, so a path rule can only tighten the
// policy: the player's media-src belongs in the one global policy, not in a /course/* rule.
{
  const headers = readFileSync(join(here, '_headers'), 'utf8');
  need((headers.match(/Content-Security-Policy:/g) ?? []).length === 1, '_headers must carry exactly one Content-Security-Policy');
  need(/media-src 'self'/.test(headers), "_headers CSP needs media-src 'self' for the course player");
}

const command = 'curl -fsSL https://app.termenbox.dev/i | sh';
need(html.includes(`<code id="cmd">${command}</code>`), 'the hero command differs from the documented one');
need(readFileSync(join(root, 'README.md'), 'utf8').includes(command), 'the README does not show the same command');

if (errors.length) { console.error(errors.map((e) => `FAIL ${e}`).join('\n')); process.exit(1); }
console.log('site checks passed');
