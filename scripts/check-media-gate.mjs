// Checks that nothing lets a rejected render through (guardrail ux-scenarios-assert-a-state-before-they-capture-it): the render
// workflow runs each case once, `render.mjs` runs the flicker check on every MP4 and fails on a rejection, and the check is run
// with no allowance. It reads the files; it renders nothing. Run: node scripts/check-media-gate.mjs
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => readFileSync(join(root, p), 'utf8');
const errors = [];
const need = (ok, msg) => { if (!ok) errors.push(msg); };

const workflow = read('.github/workflows/docs-media.yaml');
const render = workflow.split('\n').filter((l) => /render\.mjs --case/.test(l));
need(render.length === 1, `the workflow runs render.mjs --case on ${render.length} lines (one: each case runs once)`);
need(!/render\.mjs --case[^\n]*\\\s*\n\s*\|\|/.test(workflow) && !/render\.mjs --case[^\n]*\|\|/.test(workflow), 'the workflow tries a render again after a failure (|| after render.mjs)');
need(!/continue-on-error/.test(workflow), 'the workflow has continue-on-error: a failed render must fail the job');
need(!/\bretry|nick-fields|attempt_limit|max_attempts/i.test(workflow), 'the workflow has a retry');

const script = read('packages/client/e2e/video/render.mjs');
const call = /video-flicker\.mjs[^\n]*/.exec(script)?.[0] ?? '';
need(call !== '', 'render.mjs does not run scripts/video-flicker.mjs');
need(!/--max/.test(script), 'render.mjs gives the flicker check an allowance (--max)');
need(/flicker\.status !== 0[^\n]*process\.exit\(1\)/.test(script), 'render.mjs does not exit non-zero when the flicker check rejects');

const check = read('scripts/video-flicker.mjs');
need(/const max = mi >= 0 \? Number\(args\.splice\(mi, 2\)\[1\]\) : 0;/.test(check), 'video-flicker.mjs no longer defaults to zero bounce frames');

if (errors.length) { for (const e of errors) console.error(`FAIL ${e}`); process.exit(1); }
console.log('ok  the render gate has no retry, no allowance and no skip');
