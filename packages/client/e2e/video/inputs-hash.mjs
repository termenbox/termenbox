// Hash of everything a walkthrough video depends on (DECISION-0158): the case's own source text in cases.mjs, the sorted
// contents of the files its `inputs` globs match, and the app version (the server crate version, read as text).
//   node e2e/video/inputs-hash.mjs --self-test
import { createHash } from 'node:crypto';
import { globSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(here, '../../../..');

export const appVersion = (root = REPO) => /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(join(root, 'packages/server/Cargo.toml'), 'utf8'))?.[1] ?? 'unknown';

/** The source text of one case's entry in cases.mjs, from its key to the next two-space key (or the end of the object). */
export function caseSource(caseId, text = readFileSync(join(here, 'cases.mjs'), 'utf8')) {
  const start = text.search(new RegExp(`^  '${caseId}': \\{`, 'm'));
  if (start < 0) throw new Error(`no case ${caseId} in cases.mjs`);
  const rest = text.slice(start + 1);
  const next = rest.search(/^ {2}'[a-z0-9-]+': \{/m);
  return next < 0 ? text.slice(start) : text.slice(start, start + 1 + next);
}

export function hashFiles(globs, root = REPO) {
  const files = [...new Set(globs.flatMap((g) => globSync(g, { cwd: root })))].filter((f) => statSync(join(root, f)).isFile()).sort();
  return files.map((f) => [f, readFileSync(join(root, f))]);
}

export function hashParts({ source, globs, version, root = REPO }) {
  const h = createHash('sha256');
  h.update(`version\0${version}\0case\0${source}\0`);
  for (const [f, body] of hashFiles(globs, root)) { h.update(`file\0${f}\0`); h.update(body); h.update('\0'); }
  return h.digest('hex');
}

export async function hashInputs(caseId) {
  const { cases } = await import('./cases.mjs');
  if (!cases[caseId]) throw new Error(`unknown case ${caseId}`);
  return hashParts({ source: caseSource(caseId), globs: cases[caseId].inputs, version: appVersion() });
}

function selfTest() {
  const root = mkdtempSync(join(tmpdir(), 'inputs-hash-'));
  const ok = (cond, what) => { if (!cond) { console.error(`FAIL ${what}`); rmSync(root, { recursive: true, force: true }); process.exit(1); } };
  try {
    mkdirSync(join(root, 'src'));
    writeFileSync(join(root, 'src/a.svelte'), 'one');
    writeFileSync(join(root, 'src/b.txt'), 'other');
    const base = { source: "'x': { run }", globs: ['src/*.svelte'], version: '1.0.0', root };
    const h0 = hashParts(base);
    ok(h0 === hashParts(base), 'hash is not stable');
    writeFileSync(join(root, 'src/b.txt'), 'changed');
    ok(h0 === hashParts(base), 'an unmatched file changed the hash');
    writeFileSync(join(root, 'src/a.svelte'), 'two');
    ok(h0 !== hashParts(base), 'a matched file did not change the hash');
    writeFileSync(join(root, 'src/a.svelte'), 'one');
    ok(h0 === hashParts(base), 'restoring a file did not restore the hash');
    ok(h0 !== hashParts({ ...base, source: "'x': { run2 }" }), 'the case text did not change the hash');
    ok(h0 !== hashParts({ ...base, version: '1.0.1' }), 'the version did not change the hash');
    const split = caseSource('b', "export const cases = {\n  'a': {\n    one,\n  },\n\n  'b': {\n    two,\n  },\n};\n");
    ok(split.includes('two') && !split.includes('one'), 'caseSource took a neighbour');
  } finally { rmSync(root, { recursive: true, force: true }); }
  console.log('ok  inputs-hash self-test');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--self-test')) selfTest();
  else { const id = process.argv[2]; console.log(await hashInputs(id)); }
}
