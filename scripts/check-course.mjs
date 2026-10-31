// Checks the chapters of the user course (guardrail course-chapters-keep-the-fixed-anatomy-and-generated-steps, DECISION-0160).
// A chapter is docs/course/NN-*.md. It must hold, in order: a first line "In <time> ..." , the media block, the generated steps
// block, an "If it doesn't work" heading with exactly two entries, a "You can now" heading and a final Next link; at most 250
// words of prose outside the media and steps blocks; and every relative link must resolve. The steps block is compared with the
// case catalog (docs-course.mjs) and the poster block with docs/media (docs-media.mjs).
// Run: node scripts/check-course.mjs [--catalog <json>] [--require-media] [chapter.md...]   (no files: every chapter)
//      node scripts/check-course.mjs --self-test
// Without --require-media a missing video is a warning, so a chapter can land before its media is picked (plan step 5.1).
import { readFileSync, readdirSync, existsSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { check as checkSteps, markers as stepMarkers } from './docs-course.mjs';
import { check as checkMedia, markers as mediaMarkers } from './docs-media.mjs';

export const MAX_WORDS = 250;
const MEDIA_BLOCK = /<!-- media:[a-z0-9-]+ -->[\s\S]*?<!-- \/media -->|<!-- media:[a-z0-9-]+ -->/g;
const STEPS_BLOCK = /<!-- course-steps:[a-z0-9-]+ -->[\s\S]*?<!-- \/course-steps -->|<!-- course-steps:[a-z0-9-]+ -->/g;
const PROMISE = /^In (about |under |just )?\d+(\.\d+)?\s*(seconds?|secs?|minutes?|mins?|hours?)\b/i;
const heading = (re) => new RegExp(`^#{1,6}\\s+${re}\\s*$`, 'im');
const TROUBLE = heading("If it doesn['’]t work");
const OUTCOME = heading('You can now');

/** The words of prose in a chapter: everything outside the media and steps blocks, comments and code, with link targets dropped. */
export function proseWords(text) {
  const prose = text.replace(MEDIA_BLOCK, ' ').replace(STEPS_BLOCK, ' ').replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/```[\s\S]*?```/g, ' ').replace(/\]\([^)]*\)/g, ']').replace(/^\s{0,3}(#{1,6}|[-*]|\d+\.)\s+/gm, '');
  return prose.split(/\s+/).filter((w) => /[\p{L}\p{N}]/u.test(w)).length;
}

/** The relative link targets of `text` outside the media blocks (those are the media check's job). */
export function relativeLinks(text) {
  const t = text.replace(MEDIA_BLOCK, ' ').replace(/```[\s\S]*?```/g, ' ');
  return [...t.matchAll(/\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)].map((m) => m[1])
    .filter((l) => !/^([a-z][a-z0-9+.-]*:|#|\/\/)/i.test(l)).map((l) => decodeURI(l.split('#')[0].split('?')[0])).filter(Boolean);
}

/** Entries in the section that starts at `startIdx`: list items up to the next heading. */
const entries = (text, startIdx) => {
  const rest = text.slice(startIdx).split('\n').slice(1);
  const end = rest.findIndex((l) => /^#{1,6}\s/.test(l));
  return (end < 0 ? rest : rest.slice(0, end)).filter((l) => /^(\s{0,3})([-*]|\d+\.)\s+\S/.test(l)).length;
};

/** Errors of one chapter. `catalog` (docs-course) and `mediaDir` are optional; `requireMedia` turns a missing video into an error. */
export function checkChapter(text, { file, catalog, mediaDir, requireMedia = false } = {}) {
  const errors = [], warnings = [];
  const body = text.split('\n');
  const first = body.slice(body.findIndex((l) => /^#\s/.test(l)) + 1).find((l) => l.trim());
  if (!first || !PROMISE.test(first.trim())) errors.push('the first line after the title must start with "In " and name a time (e.g. "In 2 minutes ...")');

  const at = {
    media: text.search(/<!-- media:/), steps: text.search(/<!-- course-steps:/),
    trouble: text.search(TROUBLE), outcome: text.search(OUTCOME),
  };
  const labels = { media: 'a <!-- media:<id> --> block', steps: 'a <!-- course-steps:<case> --> block', trouble: 'an "If it doesn\'t work" heading', outcome: 'a "You can now" heading' };
  for (const k of Object.keys(at)) if (at[k] < 0) errors.push(`missing ${labels[k]}`);
  const present = Object.keys(at).filter((k) => at[k] >= 0);
  if (present.some((k, i) => i && at[k] < at[present[i - 1]])) errors.push('parts are out of order: promise, media, steps, "If it doesn\'t work", "You can now", Next');
  if (at.trouble >= 0) { const n = entries(text, at.trouble); if (n !== 2) errors.push(`"If it doesn't work" has ${n} entries, not 2`); }

  const last = body.map((l) => l.trim()).filter(Boolean).pop() ?? '';
  if (!/^\W*Next\b.*\]\([^)]+\)/i.test(last)) errors.push('the last line must be a Next link');
  else if (at.outcome >= 0 && text.lastIndexOf(last) < at.outcome) errors.push('the Next link must come after "You can now"');

  const words = proseWords(text);
  if (words > MAX_WORDS) errors.push(`${words} words of prose, at most ${MAX_WORDS}`);

  if (file) for (const l of relativeLinks(text)) if (!existsSync(resolve(dirname(file), l))) errors.push(`broken link: ${l}`);

  if (stepMarkers(text).length) {
    if (catalog) errors.push(...checkSteps(text, catalog));
    else errors.push('no catalog to compare the steps block with (pass --catalog)');
  }
  if (mediaDir && mediaMarkers(text).length) {
    const m = checkMedia(text, { mediaDir });
    (requireMedia ? errors : warnings).push(...m.errors);
    warnings.push(...m.warnings);
  }
  return { errors, warnings };
}

/** The chapter files under `dir`: NN-*.md, never README.md; none when the folder does not exist yet. */
export const chapters = (dir) => (existsSync(dir) ? readdirSync(dir).filter((f) => /^\d\d-.*\.md$/.test(f)).sort().map((f) => join(dir, f)) : []);

function selfTest() {
  const root = mkdtempSync(join(tmpdir(), 'tb-course-'));
  try {
    const catalog = [{ id: 'install', steps: [{ id: 'a', caption: 'Open a terminal', label: null }, { id: 'b', caption: 'Paste the command', label: 'Paste' }] }];
    const steps = '<!-- course-steps:install -->\n1. Open a terminal.\n2. Paste the command. Tap **Paste**.\n<!-- /course-steps -->';
    const good = (extra = '') => `# Install\n\nIn 2 minutes your server is running.\n\n<!-- media:install-desktop -->\n\n${steps}\n\nOne picture: the QR appears.${extra}\n\n## If it doesn't work\n\n- Nothing shows. The port is closed. Open it.\n- It stops. You are root. Run as a user.\n\n## You can now\n\n- See the QR code.\n\n[Next: connect your phone](02-connect.md)\n`;
    mkdirSync(join(root, 'media'));
    writeFileSync(join(root, '02-connect.md'), '# Two\n');
    const file = join(root, '01-install.md');
    const run = (text, o = {}) => checkChapter(text, { file, catalog, mediaDir: join(root, 'media'), ...o });
    assert.deepEqual(run(good()).errors, [], 'a good chapter passes');
    assert.match(run(good()).warnings[0], /no approved video/, 'a missing video is a warning');
    assert.match(run(good(), { requireMedia: true }).errors[0], /no approved video/, 'and an error with --require-media');
    assert.match(run(good().replace('## You can now', '## Done')).errors.join('|'), /missing a "You can now"/);
    assert.match(run(good().replace('In 2 minutes', 'Soon')).errors.join('|'), /name a time/);
    assert.match(run(good().replace('- It stops. You are root. Run as a user.\n', '')).errors.join('|'), /1 entries, not 2/);
    assert.match(run(good().replace('[Next: connect your phone](02-connect.md)', 'The end.')).errors.join('|'), /Next link/);
    assert.match(run(good().replace('02-connect.md', '09-gone.md')).errors.join('|'), /broken link: 09-gone.md/);
    assert.match(run(good(` ${'word '.repeat(MAX_WORDS)}`)).errors.join('|'), /words of prose/);
    assert.match(run(good().replace('Paste the command. Tap **Paste**.', 'Paste it.')).errors.join('|'), /stale/);
    const swapped = good().replace(/## If it doesn't work[\s\S]*## You can now\n\n- See the QR code\.\n/, (s) => s.split('## You can now')[1].replace(/^/, '## You can now') + s.split('## You can now')[0]);
    assert.match(run(swapped).errors.join('|'), /out of order|Next link/);
    assert.ok(proseWords(good()) < MAX_WORDS && proseWords(good()) > 20, 'blocks and link targets are not counted');
    assert.deepEqual(chapters(join(root, 'nope')), [], 'no folder is no chapters');
    writeFileSync(join(root, 'README.md'), 'x');
    assert.deepEqual(chapters(root), [join(root, '02-connect.md')], 'README is not a chapter');
  } finally { rmSync(root, { recursive: true, force: true }); }
  console.log('ok  check-course self-test');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) selfTest();
  else {
    const take = (flag) => { const i = args.indexOf(flag); return i >= 0 ? args.splice(i, 2)[1] : undefined; };
    const requireMedia = args.includes('--require-media') && !!args.splice(args.indexOf('--require-media'), 1);
    const catalogPath = take('--catalog');
    const files = args.length ? args : chapters('docs/course');
    const catalog = catalogPath ? JSON.parse(readFileSync(catalogPath, 'utf8')) : undefined;
    let failed = false;
    for (const f of files) {
      const r = checkChapter(readFileSync(f, 'utf8'), { file: f, catalog, mediaDir: resolve('docs/media'), requireMedia });
      r.warnings.forEach((w) => console.warn(`warn ${f}: ${w}`));
      r.errors.forEach((e) => { console.error(`FAIL ${f}: ${e}`); failed = true; });
    }
    if (failed) process.exit(1);
    console.log(`ok  ${files.length} chapter(s)`);
  }
}
