// Fills and checks the media markers in the README and the guide (DECISION-0158, DECISION-0159). A page marks a place with
//   <!-- media:add-server-phone -->...<!-- /media -->     (`add-server` alone means the phone video)
// and `embed` writes a poster image that links to the video in docs/media/, with a path relative to the page. GitHub plays no MP4
// inline in a README, so the chapter shows the poster still, linked to the video, with its length (DECISION-0161). Files reach docs/media only through an approved render's pull request.
// Run: node scripts/docs-media.mjs embed|check [--media <dir>] [--site <dir>] <file.md>... | --self-test
import { readFileSync, writeFileSync, existsSync, mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const SITE = 'https://termenbox.dev';
/** `docs/course/01-install.md` -> `01-install`; any other page -> null. */
export const chapterOf = (file) => /(?:^|[\\/])docs[\\/]course[\\/](\d\d-[a-z0-9-]+)\.md$/.exec(file)?.[1] ?? null;

const MARKER = /<!-- media:([a-z0-9-]+) -->[\s\S]*?<!-- \/media -->|<!-- media:([a-z0-9-]+) -->(?![\s\S]*?<!-- \/media -->)/g;

/** `add-server-phone` -> { case: 'add-server', profile: 'phone' }; a bare case id means the phone profile. */
export function splitId(id) {
  const m = /^(.*)-(phone|desktop)$/.exec(id);
  return m ? { case: m[1], profile: m[2] } : { case: id, profile: 'phone' };
}
const stem = (id) => { const { case: c, profile } = splitId(id); return `${c}-${profile}`; };

/** The markers of `text` as ids. */
export const markers = (text) => [...text.matchAll(MARKER)].map((m) => m[1] ?? m[2]);

/** Which files an approved video has in `mediaDir`. */
const has = (mediaDir, id) => ['.mp4', '-poster.png', '.render.json'].every((tail) => existsSync(join(mediaDir, `${stem(id)}${tail}`)));

export const ASSET_URL = /^https:\/\/github\.com\/user-attachments\/assets\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const readJson = (file) => JSON.parse(readFileSync(file, 'utf8'));

/** The GitHub upload records (`<case>-<profile>` -> { url, hash, run }); written only by docs-media-upload.mjs. */
export const readAssets = (mediaDir) => (existsSync(join(mediaDir, 'github-assets.json')) ? readJson(join(mediaDir, 'github-assets.json')) : {});

/** The upload record of `id` when it still matches the video's render.json (same input hash and render run), else null. */
function freshAsset(id, mediaDir) {
  const rec = readAssets(mediaDir)[stem(id)];
  const meta = readJson(join(mediaDir, `${stem(id)}.render.json`));
  return rec && ASSET_URL.test(rec.url ?? '') && rec.hash === meta.hash && rec.run === meta.run ? rec : null;
}

/** The block for `id` with links relative to the page's directory. */
function block(id, mediaDir, pageDir, chapter) {
  const { case: c } = splitId(id);
  const rel = (tail) => relative(pageDir, join(mediaDir, `${stem(id)}${tail}`)).split('\\').join('/');
  const { seconds } = JSON.parse(readFileSync(join(mediaDir, `${stem(id)}.render.json`), 'utf8'));
  if (!Number.isFinite(seconds)) throw new Error(`media:${id}: its render.json has no length (seconds)`);
  // a course chapter's poster opens the player page on termenbox.dev (GitHub downloads an MP4); other pages link the file
  const target = chapter ? `${SITE}/course/${chapter}/` : rel('.mp4');
  // a chapter also gets the bare user-attachments URL: github.com renders that line as a player (docs/plans/assets/github-video-probe.md)
  const asset = chapter ? freshAsset(id, mediaDir) : null;
  return `<!-- media:${id} -->\n${asset ? `${asset.url}\n\n` : ''}[![Walkthrough: ${c.replace(/-/g, ' ')}](${rel('-poster.png')})](${target})\n\nWatch, ${seconds} s\n<!-- /media -->`;
}

/** `text` with every marker filled from `mediaDir`; throws on a marker whose video is not there. */
export function embed(text, { mediaDir, pageDir, chapter }) {
  return text.replace(MARKER, (_all, a, b) => {
    const id = a ?? b;
    if (!has(mediaDir, id)) throw new Error(`media:${id} has no video in ${relative(process.cwd(), mediaDir) || mediaDir}`);
    return block(id, mediaDir, pageDir, chapter);
  });
}

/** Errors for markers whose video is missing; warnings for an input hash that no longer matches (`hashes` maps case -> hash). */
export function check(text, { mediaDir, hashes = {}, siteDir, chapter }) {
  const errors = [], warnings = [];
  for (const id of markers(text)) {
    if (!has(mediaDir, id)) { errors.push(`media:${id} has no approved video`); continue; }
    if (siteDir) for (const tail of ['.mp4', '-poster.png']) if (!existsSync(join(siteDir, 'media', `${stem(id)}${tail}`))) errors.push(`media:${id} is missing from the site output (${stem(id)}${tail})`);
    const meta = JSON.parse(readFileSync(join(mediaDir, `${stem(id)}.render.json`), 'utf8'));
    if (chapter) {
      const rec = readAssets(mediaDir)[stem(id)];
      if (!rec) errors.push(`media:${id} has no GitHub upload record in github-assets.json (run scripts/docs-media-upload.mjs)`);
      else if (!freshAsset(id, mediaDir)) errors.push(`media:${id} has a stale or malformed GitHub upload: re-run scripts/docs-media-upload.mjs ${stem(id)}`);
      else if (!text.includes(`<!-- media:${id} -->\n${rec.url}\n`)) errors.push(`media:${id} does not show its GitHub upload line (run docs-media.mjs embed)`);
    }
    if (hashes[meta.case] && hashes[meta.case] !== meta.hash) warnings.push(`media:${id} may be stale: its inputs changed since the approval`);
  }
  return { errors, warnings };
}

/** Cases that no page marks: every case outside the `dialog` group (optional extras) and not a template (review only) needs a media marker in some page. */
export function unmarked(catalog, texts) {
  const marked = new Set(texts.flatMap((x) => markers(x).map((id) => splitId(id).case)));
  return catalog.filter((c) => c.group !== 'dialog' && !c.template && !marked.has(c.id)).map((c) => c.id);
}

function selfTest() {
  const root = mkdtempSync(join(tmpdir(), 'tb-media-'));
  try {
    const media = join(root, 'docs/media');
    mkdirSync(media, { recursive: true });
    for (const tail of ['.mp4', '-poster.png']) writeFileSync(join(media, `add-server-phone${tail}`), 'x');
    writeFileSync(join(media, 'add-server-phone.render.json'), JSON.stringify({ case: 'add-server', profile: 'phone', hash: 'h1', seconds: 41 }));
    writeFileSync(join(media, 'half-phone.mp4'), 'x');
    const page = 'Intro\n\n<!-- media:add-server -->\n\nAfter\n';
    const once = embed(page, { mediaDir: media, pageDir: join(root, 'docs/guide') });
    assert.ok(once.includes('](../media/add-server-phone-poster.png)](../media/add-server-phone.mp4)'), 'relative to the guide page');
    assert.ok(once.includes('\nWatch, 41 s\n'), 'the length from render.json');
    assert.ok(embed(page, { mediaDir: media, pageDir: root }).includes('](docs/media/add-server-phone.mp4)'), 'relative to the README');
    assert.ok(once.includes('Watch, ') && once.includes('<!-- /media -->'));
    assert.equal(embed(once, { mediaDir: media, pageDir: join(root, 'docs/guide') }), once, 'a second embed changes nothing');
    assert.throws(() => embed('<!-- media:missing-phone -->', { mediaDir: media, pageDir: root }), /no video/);
    assert.throws(() => embed('<!-- media:half -->', { mediaDir: media, pageDir: root }), /no video/, 'a video without its poster and render.json is refused');
    assert.deepEqual(check(once, { mediaDir: media }), { errors: [], warnings: [] });
    const chap = embed(page, { mediaDir: media, pageDir: join(root, 'docs/course'), chapter: '02-connect-your-phone' });
    assert.ok(chap.includes('](https://termenbox.dev/course/02-connect-your-phone/)'), 'a chapter poster links to its player page');
    assert.equal(chapterOf('docs/course/02-connect-your-phone.md'), '02-connect-your-phone');
    assert.equal(chapterOf('docs/course/README.md'), null);
    // GitHub upload records: a chapter needs a fresh one and shows its URL line; other pages neither
    const url = 'https://github.com/user-attachments/assets/6e77429d-ea0c-41b0-b806-876f711b68f1';
    const ch = '02-connect-your-phone';
    writeFileSync(join(media, 'add-server-phone.render.json'), JSON.stringify({ case: 'add-server', profile: 'phone', hash: 'h1', run: 'r1', seconds: 41 }));
    const plain = embed(page, { mediaDir: media, pageDir: root, chapter: ch });
    assert.ok(!plain.includes('user-attachments'), 'no record, no URL line');
    assert.match(check(plain, { mediaDir: media, chapter: ch }).errors[0], /no GitHub upload record/);
    assert.equal(check(plain, { mediaDir: media }).errors.length, 0, 'a page that is not a chapter needs no record');
    writeFileSync(join(media, 'github-assets.json'), JSON.stringify({ 'add-server-phone': { url, hash: 'h1', run: 'r1' } }));
    const shown = embed(page, { mediaDir: media, pageDir: root, chapter: ch });
    assert.ok(shown.includes(`<!-- media:add-server -->\n${url}\n\n[![`), 'the bare URL line sits above the poster');
    assert.ok(!embed(page, { mediaDir: media, pageDir: root }).includes('user-attachments'), 'a non-chapter page shows no URL line');
    assert.equal(embed(shown, { mediaDir: media, pageDir: root, chapter: ch }), shown, 'a second embed changes nothing');
    assert.deepEqual(check(shown, { mediaDir: media, chapter: ch }).errors, []);
    assert.match(check(plain, { mediaDir: media, chapter: ch }).errors[0], /does not show its GitHub upload line/);
    writeFileSync(join(media, 'add-server-phone.render.json'), JSON.stringify({ case: 'add-server', profile: 'phone', hash: 'h1', run: 'r2', seconds: 41 }));
    assert.match(check(shown, { mediaDir: media, chapter: ch }).errors[0], /stale/, 'a re-render makes the upload stale');
    assert.ok(!embed(page, { mediaDir: media, pageDir: root, chapter: ch }).includes('user-attachments'), 'a stale record is not written');
    const site = join(root, 'site-dist');
    mkdirSync(join(site, 'media'), { recursive: true });
    assert.equal(check(once, { mediaDir: media, siteDir: site }).errors.length, 2, 'a video missing from the site output fails');
    for (const tail of ['.mp4', '-poster.png']) writeFileSync(join(site, 'media', `add-server-phone${tail}`), 'x');
    assert.equal(check(once, { mediaDir: media, siteDir: site }).errors.length, 0);
    assert.match(check('<!-- media:missing -->', { mediaDir: media }).errors[0], /no approved video/);
    assert.match(check(once, { mediaDir: media, hashes: { 'add-server': 'h2' } }).warnings[0], /stale/);
    assert.equal(check('no markers here', { mediaDir: media }).errors.length, 0);
    const cat = [{ id: 'add-server', group: 'core' }, { id: 'gone', group: 'start' }, { id: 'extra', group: 'dialog' }, { id: 'tpl', group: 'core', template: true }];
    assert.deepEqual(unmarked(cat, ['<!-- media:add-server-phone -->']), ['gone'], 'a case without a marker is reported, a dialog case and a template are not');
    assert.deepEqual(unmarked(cat, ['<!-- media:add-server -->', '<!-- media:gone-desktop -->']), []);
    assert.deepEqual(splitId('add-server-desktop'), { case: 'add-server', profile: 'desktop' });
  } finally { rmSync(root, { recursive: true, force: true }); }
  console.log('ok  docs-media self-test');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) selfTest();
  else if (args[0] === 'cases') {
    // cases --catalog <json> <file.md>...: fail when a case that needs a marker has none in any page
    const ci = args.indexOf('--catalog');
    const files = args.slice(1).filter((_, i) => i + 1 !== ci && i + 1 !== ci + 1);
    if (ci < 0 || !files.length) { console.error('usage: docs-media.mjs cases --catalog <json> <file.md>...'); process.exit(2); }
    const missing = unmarked(JSON.parse(readFileSync(args[ci + 1], 'utf8')), files.map((f) => readFileSync(f, 'utf8')));
    missing.forEach((id) => console.error(`FAIL case "${id}" has no <!-- media:${id} --> marker in any page`));
    if (missing.length) process.exit(1);
    console.log('ok  every case that needs a marker has one');
  } else {
    const [mode, ...rest] = args;
    const mi = rest.indexOf('--media');
    const mediaDir = resolve(mi >= 0 ? rest.splice(mi, 2)[1] : 'docs/media');
    const si = rest.indexOf('--site');
    const siteDir = si >= 0 ? resolve(rest.splice(si, 2)[1]) : undefined;
    if (!['embed', 'check'].includes(mode) || !rest.length) { console.error('usage: docs-media.mjs embed|check [--media <dir>] [--site <dir>] <file.md>... | --self-test'); process.exit(2); }
    let failed = false;
    for (const f of rest) {
      const text = readFileSync(f, 'utf8');
      if (!markers(text).length) continue;
      if (mode === 'embed') { try { writeFileSync(f, embed(text, { mediaDir, pageDir: dirname(resolve(f)), chapter: chapterOf(f) })); console.log(`ok  embedded ${f}`); } catch (e) { console.error(`FAIL ${f}: ${e.message}`); failed = true; } }
      else { const r = check(text, { mediaDir, siteDir, chapter: chapterOf(f) }); r.warnings.forEach((w) => console.warn(`warn ${f}: ${w}`)); r.errors.forEach((e) => { console.error(`FAIL ${f}: ${e}`); failed = true; }); }
    }
    if (failed) process.exit(1);
  }
}
