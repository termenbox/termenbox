// The entry points the docs-media workflows call. Everything arrives through the environment or argv, never spliced into a shell
// line. Modes: parse, ticked, approve, issue-body. See docs-media-review.mjs for the rules.
import { readFileSync, writeFileSync, copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseCommand, diffChecklist, manifestEntry, addEntry } from './docs-media-review.mjs';
import { validate, listMedia } from './docs-media-manifest.mjs';

/** The review issue's body: the preview link and one checklist line per render in the fixed form `- [ ] <case> <profile> <hash>`. */
export function issueBody({ dir, previewUrl, runId }) {
  const lines = readdirSync(dir).filter((f) => f.endsWith('.render.json')).sort()
    .map((f) => JSON.parse(readFileSync(join(dir, f), 'utf8')))
    .map((r) => `- [ ] ${r.case} ${r.profile} ${r.hash}`);
  return `Rendered walkthroughs on scripted data, run ${runId}. Watch them on your phone: ${previewUrl}\n\nTick a line to approve that video. Comment \`/regen <case> [pace=slow] [profile=phone|desktop] [caption=off] <what to change>\` to send one back, or \`/approve all\`.\n\n${lines.join('\n')}\n`;
}

/**
 * Copies one approved render into the docs-media checkout and records it. The hash the owner ticked must be the hash the render
 * was made with, so a re-render after the ticking is never published under the old approval.
 */
export function approve({ artifactDir, mediaDir, case: id, profile, hash, approver, date }) {
  const meta = JSON.parse(readFileSync(join(artifactDir, `${id}-${profile}.render.json`), 'utf8'));
  if (meta.hash !== hash) throw new Error(`${id} ${profile}: the render has input hash ${meta.hash}, not the ticked ${hash}`);
  const names = (meta.files ?? []).filter((f) => !f.includes('/') && !f.startsWith('.'));
  if (!names.some((f) => f.endsWith('.mp4'))) throw new Error(`${id} ${profile}: the render has no video`);
  const files = names.map((f) => ({ path: f, data: readFileSync(join(artifactDir, f)) }));
  const entry = manifestEntry({ case: id, profile, hash, version: meta.version, approver, date, files });
  mkdirSync(mediaDir, { recursive: true });
  for (const f of files) copyFileSync(join(artifactDir, f.path), join(mediaDir, f.path));
  const mpath = join(mediaDir, 'manifest.json');
  const before = existsSync(mpath) ? JSON.parse(readFileSync(mpath, 'utf8')) : { version: 1, entries: [] };
  const after = addEntry(before, entry);
  writeFileSync(mpath, `${JSON.stringify(after, null, 2)}\n`);
  const errors = validate(after, listMedia(mediaDir));
  if (errors.length) throw new Error(errors.join('; '));
  return after;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [mode, ...args] = process.argv.slice(2);
  const env = process.env;
  try {
    if (mode === 'parse') console.log(JSON.stringify(parseCommand(env.COMMENT, (env.CATALOG ?? '').split(/\s+/).filter(Boolean))));
    else if (mode === 'ticked') console.log(JSON.stringify(diffChecklist(env.BEFORE, env.AFTER)));
    else if (mode === 'issue-body') process.stdout.write(issueBody({ dir: args[0], previewUrl: env.PREVIEW_URL, runId: env.RUN_ID }));
    else if (mode === 'approve') {
      const [artifactDir, mediaDir, id, profile, hash] = args;
      approve({ artifactDir, mediaDir, case: id, profile, hash, approver: env.APPROVER, date: env.APPROVED_ON });
      console.log(`ok  approved ${id} ${profile}`);
    } else { console.error('usage: docs-media-review-cli.mjs parse|ticked|approve|issue-body'); process.exit(2); }
  } catch (e) { console.error(`FAIL ${e.message}`); process.exit(1); }
}
