// Uploads approved videos to GitHub so a chapter can show them as a player on github.com (docs/plans/user-course.md 5.1d).
// `gh issue comment --attach` needs a person's login: the workflow GITHUB_TOKEN is refused ("attaching files requires write
// access to the repository", also with contents: write), so a developer runs this after a re-render is approved.
// Run: node scripts/docs-media-upload.mjs <issue> <case-profile>... | --all <issue> | --self-test
// The one writer of docs/media/github-assets.json: <case>-<profile> -> { url, hash, run } of the uploaded render.json.
import { readFileSync, writeFileSync, existsSync, mkdtempSync, mkdirSync, rmSync, readdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { ASSET_URL, readAssets, splitId } from './docs-media.mjs';

const stem = (id) => { const { case: c, profile } = splitId(id); return `${c}-${profile}`; };

/** Uploads `ids` in one comment on `issue`; returns the records. `gh(args)` returns stdout. Throws before writing anything. */
export function upload(ids, { issue, mediaDir, gh }) {
  const metas = ids.map((id) => {
    const file = join(mediaDir, `${stem(id)}.mp4`), meta = join(mediaDir, `${stem(id)}.render.json`);
    if (!existsSync(file) || !existsSync(meta)) throw new Error(`${stem(id)}: no approved video in ${mediaDir}`);
    return { id: stem(id), file, ...JSON.parse(readFileSync(meta, 'utf8')) };
  });
  const body = `Docs media upload for the course chapters: ${metas.map((m) => m.id).join(', ')}. Keep this comment: the chapters link its files.`;
  const out = gh(['issue', 'comment', String(issue), '--body', body, ...metas.flatMap((m) => ['--attach', m.file])]);
  const commentId = /#issuecomment-(\d+)/.exec(out)?.[1];
  if (!commentId) throw new Error(`gh printed no comment URL: ${out.trim()}`);
  // the attachments are appended to the comment in flag order; read them back instead of trusting the printed text
  const text = JSON.parse(gh(['api', `repos/{owner}/{repo}/issues/comments/${commentId}`])).body;
  const urls = text.split('\n').map((l) => l.trim()).filter((l) => ASSET_URL.test(l));
  if (urls.length !== metas.length) throw new Error(`expected ${metas.length} user-attachments URLs in comment ${commentId}, found ${urls.length}`);
  return Object.fromEntries(metas.map((m, i) => [m.id, { url: urls[i], hash: m.hash, run: m.run }]));
}

/** Merges `records` into docs/media/github-assets.json (keys sorted). */
export function save(mediaDir, records) {
  const all = { ...readAssets(mediaDir), ...records };
  writeFileSync(join(mediaDir, 'github-assets.json'), `${JSON.stringify(Object.fromEntries(Object.entries(all).sort()), null, 2)}\n`);
}

function selfTest() {
  const root = mkdtempSync(join(tmpdir(), 'tb-upload-'));
  try {
    mkdirSync(root, { recursive: true });
    for (const [id, h] of [['a-phone', 'ha'], ['b-desktop', 'hb']]) {
      writeFileSync(join(root, `${id}.mp4`), 'x');
      writeFileSync(join(root, `${id}.render.json`), JSON.stringify({ hash: h, run: `r-${h}` }));
    }
    const u = (n) => `https://github.com/user-attachments/assets/00000000-0000-0000-0000-00000000000${n}`;
    const calls = [];
    const fake = (urls) => (args) => {
      calls.push(args);
      return args[0] === 'issue' ? 'https://github.com/o/r/issues/9#issuecomment-77\n' : JSON.stringify({ body: `text\n\n${urls.join('\n\n')}` });
    };
    const rec = upload(['a', 'b-desktop'], { issue: 9, mediaDir: root, gh: fake([u(1), u(2)]) });
    assert.deepEqual(rec, { 'a-phone': { url: u(1), hash: 'ha', run: 'r-ha' }, 'b-desktop': { url: u(2), hash: 'hb', run: 'r-hb' } }, 'flag order, bare id = phone');
    assert.deepEqual(calls[0].filter((x) => x === '--attach').length, 2);
    assert.throws(() => upload(['a'], { issue: 9, mediaDir: root, gh: fake([]) }), /expected 1 user-attachments URLs/);
    assert.throws(() => upload(['a'], { issue: 9, mediaDir: root, gh: fake(['https://example.com/x.mp4']) }), /found 0/, 'only user-attachments URLs count');
    assert.throws(() => upload(['a'], { issue: 9, mediaDir: root, gh: () => 'no comment' }), /no comment URL/);
    assert.throws(() => upload(['missing'], { issue: 9, mediaDir: root, gh: fake([]) }), /no approved video/);
    save(root, rec);
    save(root, { 'a-phone': { url: u(3), hash: 'ha', run: 'r-ha' } });
    assert.deepEqual(Object.keys(readAssets(root)), ['a-phone', 'b-desktop'], 'a second save merges and sorts');
    assert.equal(readAssets(root)['a-phone'].url, u(3));
  } finally { rmSync(root, { recursive: true, force: true }); }
  console.log('ok  docs-media-upload self-test');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) selfTest();
  else {
    const all = args[0] === '--all';
    const [issue, ...ids] = all ? args.slice(1) : args;
    const mediaDir = resolve('docs/media');
    const list = all ? readdirSync(mediaDir).filter((f) => f.endsWith('.render.json')).map((f) => f.replace(/\.render\.json$/, '')) : ids;
    if (!/^\d+$/.test(issue ?? '') || !list.length) { console.error('usage: docs-media-upload.mjs <issue> <case-profile>... | --all <issue> | --self-test'); process.exit(2); }
    try {
      const records = upload(list, { issue, mediaDir, gh: (a) => execFileSync('gh', a, { encoding: 'utf8' }) });
      save(mediaDir, records);
      Object.entries(records).forEach(([id, r]) => console.log(`ok  ${id} ${r.url}`));
    } catch (e) { console.error(`FAIL ${e.message}`); process.exit(1); }
  }
}
