#!/usr/bin/env node
// Template review (decision 0167, amended 2026-11-01): the owner accepts a UI slice's templates on a probe page with one tap.
//   node scripts/template-review.mjs body <sha> <base-url> <page>...   the review issue body: a link and a checkbox per page
//   BEFORE=... AFTER=... node scripts/template-review.mjs ticked        JSON {ticked:[{page, sha}]} newly ticked lines
//   gh issue list --label template-review --state all --json body | node scripts/template-review.mjs status <page>   exit 0 if ticked
// The line format `- [ ] accept <page> <sha>` is the record; the review workflow reads it back with `ticked`.
import { pathToFileURL } from 'node:url';

const LINE = /^- \[([ xX])\] accept ([a-z0-9-]+) ([0-9a-f]{7,40})[ \t]*$/;

export const body = (sha, base, pages) => [
  `Review the templates on their fixtures, on the device you care about (phone pages: use your phone). No video: this is the probe page.`,
  '',
  ...pages.map((p) => `- ${p}: ${base.replace(/\/$/, '')}/${p}.html`),
  '',
  'Tick a line to accept that page. To send it back, comment what to change and leave it unticked.',
  '',
  ...pages.map((p) => `- [ ] accept ${p} ${sha}`),
  '',
].join('\n');

const lines = (text) => (text ?? '').split('\n').map((l) => LINE.exec(l.replace(/\r$/, ''))).filter(Boolean).map((m) => ({ on: m[1] !== ' ', page: m[2], sha: m[3] }));

export const ticked = (before, after) => {
  const was = new Set(lines(before).filter((l) => l.on).map((l) => `${l.page} ${l.sha}`));
  return lines(after).filter((l) => l.on && !was.has(`${l.page} ${l.sha}`)).map(({ page, sha }) => ({ page, sha }));
};

/** True when any review issue body has the page's line ticked. */
export const accepted = (bodies, page) => bodies.some((b) => lines(b).some((l) => l.on && l.page === page));

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const [cmd, ...rest] = process.argv.slice(2);
  if (cmd === 'body') {
    const [sha, base, ...pages] = rest;
    if (!sha || !base || pages.length === 0 || !pages.every((p) => /^[a-z0-9-]+$/.test(p))) { console.error('usage: body <sha> <base-url> <page>...'); process.exit(2); }
    process.stdout.write(body(sha, base, pages));
  } else if (cmd === 'ticked') {
    console.log(JSON.stringify({ ticked: ticked(process.env.BEFORE, process.env.AFTER) }));
  } else if (cmd === 'status') {
    const bodies = JSON.parse(await new Response(process.stdin).text()).map((i) => i.body);
    const ok = accepted(bodies, rest[0] ?? '');
    console.log(ok ? `accepted ${rest[0]}` : `not accepted ${rest[0]}`);
    process.exit(ok ? 0 : 1);
  } else { console.error('usage: body|ticked|status'); process.exit(2); }
}
