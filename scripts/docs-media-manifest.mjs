// The audit manifest of the orphan `docs-media` branch (DECISION-0158): every approved video, GIF and still is listed with
// its sha256, and nothing on the branch is unlisted. Run: node scripts/docs-media-manifest.mjs --check <dir> | --self-test
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, statSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

export const sha256 = (data) => createHash('sha256').update(data).digest('hex');
const SKIP = new Set(['manifest.json', 'README.md', '.git', '.gitignore']);

/** Every media file under `dir` as { path, sha256, bytes }, sorted by path; the manifest, the README and git files are not media. */
export function listMedia(dir) {
  const out = [];
  const walk = (d) => {
    for (const name of readdirSync(d)) {
      if (SKIP.has(name)) continue;
      const p = join(d, name);
      if (statSync(p).isDirectory()) walk(p);
      else { const data = readFileSync(p); out.push({ path: relative(dir, p).split('\\').join('/'), sha256: sha256(data), bytes: data.length }); }
    }
  };
  walk(dir);
  return out.sort((a, b) => (a.path < b.path ? -1 : 1));
}

/** The errors of `manifest` against the files actually present (`present` from listMedia); an empty list means it is valid. */
export function validate(manifest, present) {
  const errors = [];
  if (manifest?.version !== 1 || !Array.isArray(manifest.entries)) return ['manifest must be { version: 1, entries: [...] }'];
  const listed = new Map();
  const keys = new Set();
  for (const e of manifest.entries) {
    const key = `${e.case}/${e.profile}`;
    if (keys.has(key)) errors.push(`duplicate entry ${key}`);
    keys.add(key);
    if (!e.approver) errors.push(`${key}: no approver`);
    if (!/^\d{4}-\d{2}-\d{2}/.test(e.date ?? '')) errors.push(`${key}: no date`);
    if (!e.hash) errors.push(`${key}: no input hash`);
    if (!Array.isArray(e.files) || e.files.length === 0) errors.push(`${key}: no files`);
    for (const f of e.files ?? []) {
      if (listed.has(f.path)) errors.push(`${f.path} is listed twice`);
      listed.set(f.path, f);
    }
  }
  const have = new Map(present.map((f) => [f.path, f]));
  for (const [path, f] of listed) {
    const real = have.get(path);
    if (!real) errors.push(`${path} is listed but missing`);
    else if (real.sha256 !== f.sha256) errors.push(`${path} does not match its sha256`);
    else if (real.bytes !== f.bytes) errors.push(`${path} does not match its size`);
  }
  for (const path of have.keys()) if (!listed.has(path)) errors.push(`${path} is on the branch but not in the manifest`);
  return errors;
}

function selfTest() {
  const root = mkdtempSync(join(tmpdir(), 'tb-manifest-'));
  try {
    writeFileSync(join(root, 'a-phone.mp4'), 'video');
    writeFileSync(join(root, 'a-phone.gif'), 'gif');
    writeFileSync(join(root, 'README.md'), 'about');
    const present = listMedia(root);
    assert.deepEqual(present.map((f) => f.path), ['a-phone.gif', 'a-phone.mp4'], 'README and manifest are not media');
    const good = { version: 1, entries: [{ case: 'a', profile: 'phone', hash: 'h', approver: 'owner', date: '2026-10-31', files: present }] };
    assert.deepEqual(validate(good, present), []);
    assert.ok(validate({ ...good, entries: [{ ...good.entries[0], approver: '' }] }, present).some((e) => /no approver/.test(e)));
    assert.ok(validate({ ...good, entries: [...good.entries, good.entries[0]] }, present).some((e) => /duplicate/.test(e)));
    writeFileSync(join(root, 'stray.png'), 'x');
    assert.ok(validate(good, listMedia(root)).some((e) => /not in the manifest/.test(e)), 'an unlisted file fails');
    rmSync(join(root, 'stray.png'));
    writeFileSync(join(root, 'a-phone.mp4'), 'changed');
    assert.ok(validate(good, listMedia(root)).some((e) => /sha256/.test(e)), 'a changed file fails');
    rmSync(join(root, 'a-phone.mp4'));
    assert.ok(validate(good, listMedia(root)).some((e) => /missing/.test(e)), 'a missing file fails');
    assert.ok(validate({ version: 2 }, []).length > 0);
  } finally { rmSync(root, { recursive: true, force: true }); }
  console.log('ok  docs-media-manifest self-test');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) selfTest();
  else if (args[0] === '--check' && args[1]) {
    const errors = validate(JSON.parse(readFileSync(join(args[1], 'manifest.json'), 'utf8')), listMedia(args[1]));
    if (errors.length) { console.error(errors.map((e) => `FAIL ${e}`).join('\n')); process.exit(1); }
    console.log('ok  docs media manifest');
  } else { console.error('usage: docs-media-manifest.mjs --check <dir> | --self-test'); process.exit(2); }
}
