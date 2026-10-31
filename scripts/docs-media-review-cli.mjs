// The entry points the docs-media workflows call. Everything arrives through the environment or argv, never spliced into a shell
// line. Modes: parse, ticked, approve, issue-body. See docs-media-review.mjs for the rules.
import { readFileSync, copyFileSync, mkdirSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseCommand, diffChecklist } from './docs-media-review.mjs';

/** The review issue's body: the preview link and one checklist line per render in the fixed form `- [ ] <case> <profile> <hash>`. */
export function issueBody({ dir, previewUrl, runId }) {
  const lines = readdirSync(dir).filter((f) => f.endsWith('.render.json')).sort()
    .map((f) => JSON.parse(readFileSync(join(dir, f), 'utf8')))
    .map((r) => `- [ ] ${r.case} ${r.profile} ${r.hash}`);
  return `Rendered walkthroughs on scripted data, run ${runId}. Watch them on your phone: ${previewUrl}\n\nTick a line to queue that video for the docs (a branch is pushed for you to open as a pull request). Comment \`/regen <case> [pace=slow] [profile=phone|desktop] [caption=off] <what to change>\` to send one back, or \`/approve all\`.\n\n${lines.join('\n')}\n`;
}

/**
 * Copies one ticked render (its video, GIF, stills and render.json) into the docs media folder and returns the file names. The hash
 * the reviewer ticked must be the hash the render was made with, so a re-render after the tick never lands under the old approval.
 */
export function approve({ artifactDir, mediaDir, case: id, profile, hash }) {
  const meta = JSON.parse(readFileSync(join(artifactDir, `${id}-${profile}.render.json`), 'utf8'));
  if (meta.hash !== hash) throw new Error(`${id} ${profile}: the render has input hash ${meta.hash}, not the ticked ${hash}`);
  const names = (meta.files ?? []).filter((f) => !f.includes('/') && !f.startsWith('.') && f.startsWith(`${id}-${profile}`));
  if (!names.some((f) => f.endsWith('.mp4'))) throw new Error(`${id} ${profile}: the render has no video`);
  names.push(`${id}-${profile}.render.json`);
  mkdirSync(mediaDir, { recursive: true });
  for (const f of names) copyFileSync(join(artifactDir, f), join(mediaDir, f));
  return names;
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
      console.log(approve({ artifactDir, mediaDir, case: id, profile, hash }).join('\n'));
    } else { console.error('usage: docs-media-review-cli.mjs parse|ticked|approve|issue-body'); process.exit(2); }
  } catch (e) { console.error(`FAIL ${e.message}`); process.exit(1); }
}
