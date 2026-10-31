// Fills and checks the media markers in the README and the guide (DECISION-0158, DECISION-0159). A page marks a place with
//   <!-- media:add-server-phone -->...<!-- /media -->     (`add-server` alone means the phone video)
// and `embed` writes a poster image that links to the approved video. GitHub plays no MP4 inline, so the poster is the GIF.
// Run: node scripts/docs-media.mjs embed|check [--manifest <file>] <file.md>... | --self-test
import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

export const SITE = 'https://termenbox.dev';
const MARKER = /<!-- media:([a-z0-9-]+) -->[\s\S]*?<!-- \/media -->|<!-- media:([a-z0-9-]+) -->(?![\s\S]*?<!-- \/media -->)/g;

/** `add-server-phone` -> { case: 'add-server', profile: 'phone' }; a bare case id means the phone profile. */
export function splitId(id) {
  const m = /^(.*)-(phone|desktop)$/.exec(id);
  return m ? { case: m[1], profile: m[2] } : { case: id, profile: 'phone' };
}
const findEntry = (manifest, id) => { const { case: c, profile } = splitId(id); return (manifest?.entries ?? []).find((e) => e.case === c && e.profile === profile); };
const block = (id) => {
  const { case: c, profile } = splitId(id);
  const base = `${SITE}/media/${c}-${profile}`;
  return `<!-- media:${id} -->\n[![Walkthrough: ${c.replace(/-/g, ' ')}](${base}.gif)](${base}.mp4)\n\n*A rendered walkthrough on scripted data, not a recording of a real device.*\n<!-- /media -->`;
};

/** The markers of `text` as ids. */
export const markers = (text) => [...text.matchAll(MARKER)].map((m) => m[1] ?? m[2]);

/** `text` with every marker filled from the approved manifest entries; throws on a marker with no approved entry. */
export function embed(text, manifest) {
  return text.replace(MARKER, (_all, a, b) => {
    const id = a ?? b;
    const e = findEntry(manifest, id);
    if (!e?.approver) throw new Error(`media:${id} has no approved entry in the manifest`);
    return block(id);
  });
}

/** Errors for markers with no approved entry; warnings for an input hash that no longer matches (`hashes` maps case -> hash). */
export function check(text, manifest, hashes = {}) {
  const errors = [], warnings = [];
  for (const id of markers(text)) {
    const e = findEntry(manifest, id);
    if (!e) errors.push(`media:${id} is not in the manifest`);
    else if (!e.approver) errors.push(`media:${id} has no approver`);
    else if (hashes[e.case] && hashes[e.case] !== e.hash) warnings.push(`media:${id} may be stale: its inputs changed since the approval`);
  }
  return { errors, warnings };
}

function selfTest() {
  const manifest = { version: 1, entries: [{ case: 'add-server', profile: 'phone', hash: 'h1', approver: 'owner', date: '2026-10-31', files: [] }, { case: 'draft', profile: 'phone', hash: 'h', approver: '', date: '2026-10-31', files: [] }] };
  const page = 'Intro\n\n<!-- media:add-server -->\n\nAfter\n';
  const once = embed(page, manifest);
  assert.ok(once.includes(`${SITE}/media/add-server-phone.gif`) && once.includes(`${SITE}/media/add-server-phone.mp4`));
  assert.ok(once.includes('rendered walkthrough on scripted data') && once.includes('<!-- /media -->'));
  assert.equal(embed(once, manifest), once, 'a second embed changes nothing');
  assert.throws(() => embed('<!-- media:missing-phone -->', manifest), /no approved entry/);
  assert.throws(() => embed('<!-- media:draft -->', manifest), /no approved entry/, 'an unapproved id is refused');
  assert.deepEqual(check(once, manifest), { errors: [], warnings: [] });
  assert.match(check('<!-- media:missing -->', manifest).errors[0], /not in the manifest/);
  assert.match(check('<!-- media:draft -->', manifest).errors[0], /no approver/);
  assert.match(check(once, manifest, { 'add-server': 'h2' }).warnings[0], /stale/);
  assert.equal(check('no markers here', manifest).errors.length, 0);
  assert.deepEqual(splitId('add-server-desktop'), { case: 'add-server', profile: 'desktop' });
  console.log('ok  docs-media self-test');
}

function loadManifest(path) {
  if (path) return JSON.parse(readFileSync(path, 'utf8'));
  try { return JSON.parse(execFileSync('git', ['show', 'origin/docs-media:manifest.json'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })); } catch { return null; }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) selfTest();
  else {
    const [mode, ...rest] = args;
    const mi = rest.indexOf('--manifest');
    const manifestPath = mi >= 0 ? rest.splice(mi, 2)[1] : null;
    if (!['embed', 'check'].includes(mode) || !rest.length) { console.error('usage: docs-media.mjs embed|check [--manifest <file>] <file.md>... | --self-test'); process.exit(2); }
    const manifest = loadManifest(manifestPath);
    let failed = false;
    for (const f of rest) {
      const text = readFileSync(f, 'utf8');
      if (!markers(text).length) continue;
      if (!manifest) { console.error(`FAIL ${f}: has media markers but no manifest is available (fetch origin/docs-media)`); failed = true; continue; }
      if (mode === 'embed') { try { writeFileSync(f, embed(text, manifest)); console.log(`ok  embedded ${f}`); } catch (e) { console.error(`FAIL ${f}: ${e.message}`); failed = true; } }
      else { const r = check(text, manifest); r.warnings.forEach((w) => console.warn(`warn ${f}: ${w}`)); r.errors.forEach((e) => { console.error(`FAIL ${f}: ${e}`); failed = true; }); }
    }
    if (failed) process.exit(1);
  }
}
