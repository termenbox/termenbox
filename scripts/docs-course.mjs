// Generates and checks the numbered steps of a course chapter from the recorded case (DECISION-0161). A chapter marks the place with
//   <!-- course-steps:<case> -->...<!-- /course-steps -->
// and the block is rebuilt from the case's `steps` in a catalog, the JSON that CI writes with
//   node packages/client/e2e/video/render.mjs --list --json > catalog.json
// so reading it needs no Playwright. Run:
//   node scripts/docs-course.mjs steps|check --catalog <catalog.json> <chapter.md>...  |  --self-test
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const BLOCK = /<!-- course-steps:([a-z0-9-]+) -->[\s\S]*?<!-- \/course-steps -->|<!-- course-steps:([a-z0-9-]+) -->(?![\s\S]*?<!-- \/course-steps -->)/g;

/** The case ids named by the markers of `text`. */
export const markers = (text) => [...text.matchAll(BLOCK)].map((m) => m[1] ?? m[2]);

/** Markdown-escapes a label so it cannot start emphasis, a link or HTML inside the line. */
export const escapeLabel = (s) => s.replace(/[\\`*_{}[\]()#+!|<>~]/g, '\\$&');

/** The line of one step: the caption, then the tap when there is one. */
const line = (n, { caption, label }) => {
  const sentence = /[.!?]$/.test(caption) ? caption : `${caption}.`;
  return label ? `${n}. ${sentence} Tap **${escapeLabel(label)}**.` : `${n}. ${sentence}`;
};

/** The marked block for `caseId`, built from the catalog; throws when the catalog does not know the case. */
export function block(caseId, catalog) {
  const c = catalog.find((x) => x.id === caseId);
  if (!c) throw new Error(`course-steps names an unknown case: ${caseId}`);
  if (!c.steps?.length) throw new Error(`case ${caseId} declares no steps in the catalog`);
  return `<!-- course-steps:${caseId} -->\n${c.steps.map((s, i) => line(i + 1, s)).join('\n')}\n<!-- /course-steps -->`;
}

/** `text` with every block rebuilt. */
export const generate = (text, catalog) => text.replace(BLOCK, (_, a, b) => block(a ?? b, catalog));

/** Errors for a chapter: no block, an unknown case, or a block that differs from what the catalog gives. */
export function check(text, catalog) {
  const ids = markers(text);
  if (!ids.length) return ['no course-steps block'];
  const errors = [];
  for (const m of text.matchAll(BLOCK)) {
    const id = m[1] ?? m[2];
    try { if (m[0] !== block(id, catalog)) errors.push(`stale steps block for ${id}: run docs-course.mjs steps`); } catch (e) { errors.push(e.message); }
  }
  return errors;
}

function selfTest() {
  const catalog = [{ id: 'demo', steps: [
    { id: 'a', caption: 'Start a new workspace', label: '+ New' },
    { id: 'b', caption: 'Read the summary.', label: null },
    { id: 'c', caption: 'Pick it', label: 'Use *this* [one]_' },
  ] }];
  const page = 'Intro\n\n<!-- course-steps:demo -->\n\nAfter\n';
  const once = generate(page, catalog);
  assert.ok(once.includes('1. Start a new workspace. Tap **\\+ New**.'), 'a tap step names its label');
  assert.ok(once.includes('2. Read the summary.\n'), 'a step without a label is just the caption');
  assert.ok(once.includes('Tap **Use \\*this\\* \\[one\\]\\_**.'), 'Markdown characters in a label are escaped');
  assert.deepEqual(check(once, catalog), [], 'a fresh block passes');
  assert.equal(generate(once, catalog), once, 'a second run changes nothing');
  const stale = once.replace('Read the summary.', 'Read the old summary.');
  assert.match(check(stale, catalog)[0], /stale steps block for demo/);
  assert.equal(generate(stale, catalog), once, 'a stale block is rebuilt');
  assert.match(check('<!-- course-steps:gone -->\n<!-- /course-steps -->', catalog)[0], /unknown case: gone/);
  assert.throws(() => generate('<!-- course-steps:gone -->', catalog), /unknown case/);
  assert.deepEqual(check('no block here', catalog), ['no course-steps block']);
  assert.throws(() => block('demo', [{ id: 'demo', steps: [] }]), /no steps/);
  console.log('ok  docs-course self-test');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) selfTest();
  else {
    const [mode, ...rest] = args;
    const ci = rest.indexOf('--catalog');
    const catalogFile = ci >= 0 ? rest.splice(ci, 2)[1] : undefined;
    if (!['steps', 'check'].includes(mode) || !catalogFile || !rest.length) { console.error('usage: docs-course.mjs steps|check --catalog <catalog.json> <chapter.md>... | --self-test'); process.exit(2); }
    const catalog = JSON.parse(readFileSync(catalogFile, 'utf8'));
    let failed = false;
    for (const f of rest) {
      const text = readFileSync(f, 'utf8');
      if (mode === 'steps') {
        try { writeFileSync(f, generate(text, catalog)); console.log(`ok  steps ${f}`); } catch (e) { console.error(`FAIL ${f}: ${e.message}`); failed = true; }
      } else for (const e of check(text, catalog)) { console.error(`FAIL ${f}: ${e}`); failed = true; }
    }
    if (failed) process.exit(1);
  }
}
