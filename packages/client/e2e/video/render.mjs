// Renders one New workspace dialog walkthrough as a captioned MP4 (H.264) and a GIF poster.
//   node e2e/video/render.mjs --list [--json]   prints the case ids (or their metadata)
//   --pace slow|normal (slow stretches every delay by 1.6) and --no-caption (nothing drawn) go with --case
//   node e2e/video/render.mjs --case recent-folder|other-agent|browse-folder|type-path --profile phone|desktop --out DIR
// phone is 390 x 844 at DPR 2 with touch; desktop is 1280 x 800. The page is the built client (pnpm run e2e:build) against the
// scripted server and its fake home, recorded with Playwright and converted with ffmpeg. A case whose final assertion fails
// exits non-zero and leaves no file in DIR. The video says it is a rendered walkthrough on scripted data, not a real device.
import { startShared, startApp } from '../app-lib.mjs';
import { humanPage, overlayScript, paceFactor, sleep } from './lib.mjs';
import { cases } from './cases.mjs';
import { launchReal } from './real-server.mjs';
import { appVersion, hashInputs } from './inputs-hash.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const arg = (name) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : undefined; };
if (process.argv.includes('--list')) {
  const rows = Object.entries(cases).map(([id, c]) => ({ id, title: c.title, group: c.group, page: c.page, inputs: c.inputs, real: !!c.real }));
  console.log(process.argv.includes('--json') ? JSON.stringify(rows, null, 2) : rows.map((c) => c.id).join('\n'));
  process.exit(0);
}
const caseName = arg('case');
const profile = arg('profile') ?? 'phone';
const pace = arg('pace') ?? 'normal';
const captions = !process.argv.includes('--no-caption');
const out = resolve(arg('out') ?? 'videos');
if (!cases[caseName]) { console.error(`--case must be one of:\n${Object.keys(cases).join('\n')}`); process.exit(2); }
try { paceFactor(pace); } catch (e) { console.error(e.message); process.exit(2); }
if (!['phone', 'desktop'].includes(profile)) { console.error('--profile must be phone or desktop'); process.exit(2); }

const phone = profile === 'phone';
const viewport = phone ? { width: 390, height: 844 } : { width: 1280, height: 800 };
const scale = phone ? 2 : 1;
const work = mkdtempSync(join(tmpdir(), 'termenbox-video-'));
const shared = await startShared();
let failure = null;
let ready = 0;
const started = Date.now();
let t;
let h;
try {
  if (cases[caseName].real) {
    // The real server binary and the add-server wizard: the case brings its own browser (real-server.mjs).
    t = await launchReal({ viewport, scale, phone, recordDir: work, overlay: overlayScript });
  } else {
    t = await startApp(shared, {
      viewport, deviceScaleFactor: scale, storage: { termenbox_ios_hint: '1', ...cases[caseName].storage },
      contextExtra: { ...(phone ? { isMobile: true, hasTouch: true } : {}), recordVideo: { dir: work, size: viewport } },
    });
    await t.ctx.addInitScript(overlayScript, phone);
    await t.open();
  }
  await sleep(800);
  ready = (Date.now() - started) / 1000;
  h = humanPage(t.page, profile, { pace, caption: captions, seed: caseName });
  await h.say('A rendered walkthrough on scripted data, not a real device');
  const dialog = t.page.locator('dialog[aria-label="New workspace"]');
  await cases[caseName].run({ h, t, page: t.page, dialog, q: (id) => dialog.locator(`[data-testid="${id}"]`), server: t.server, real: t.real });
  await h.clearCaption();
  await h.pause(1200);
  if (t.errors.length) throw new Error(`console errors: ${t.errors.join(' | ')}`);
} catch (e) {
  failure = e;
}
const video = t?.page.video();
await t?.ctx.close().catch(() => {});
const webm = video ? await video.path().catch(() => null) : null;
await t?.stop?.();
await shared.stop();

if (failure || !webm) {
  rmSync(work, { recursive: true, force: true });
  console.error(`FAIL ${caseName} (${profile}): ${process.env.VIDEO_DEBUG ? failure?.stack : failure?.message ?? 'no video was recorded'}`);
  process.exit(1);
}

mkdirSync(out, { recursive: true });
const base = `${caseName}-${profile}`;
const mp4 = join(out, `${base}.mp4`), gif = join(out, `${base}.gif`);
// Start where the app is ready (not on the blank load); even dimensions for H.264; faststart so it plays while it downloads.
const trim = ['-ss', String(Math.max(0, ready - 0.3))];
const comment = `Rendered walkthrough "${cases[caseName].title}" on ${cases[caseName].real ? 'a local test server' : 'scripted data'} (${profile}); not a recording of a real device or agent.`;
const run = (args) => { const r = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', ...args], { stdio: 'inherit' }); if (r.status !== 0) { console.error(`FAIL ffmpeg ${args.slice(0, 4).join(' ')}`); rmSync(work, { recursive: true, force: true }); rmSync(mp4, { force: true }); rmSync(gif, { force: true }); process.exit(1); } };
run([...trim, '-i', webm, '-vf', `scale=${viewport.width * scale}:${viewport.height * scale}:flags=lanczos,fps=30`, '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '23', '-movflags', '+faststart', '-an', '-metadata', `comment=${comment}`, mp4]);
const gifWidth = phone ? 320 : 640;
run([...trim, '-i', webm, '-vf', `fps=10,scale=${gifWidth}:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96[p];[b][p]paletteuse=dither=bayer`, '-loop', '0', gif]);
rmSync(work, { recursive: true, force: true });
const stillFiles = [];
for (const { name, png } of h.stills) { const f = `${base}-${name}.png`; writeFileSync(join(out, f), png); stillFiles.push(f); }
// What the review ledger and the staleness check read (DECISION-0158): the hash says which inputs this video was made from.
const meta = join(out, `${base}.render.json`);
writeFileSync(meta, JSON.stringify({ case: caseName, profile, pace, captions, hash: await hashInputs(caseName), version: appVersion(), run: process.env.GITHUB_RUN_ID ?? null, renderedAt: new Date().toISOString(), files: [`${base}.mp4`, `${base}.gif`, ...stillFiles] }, null, 2) + '\n');
console.log(`ok  ${base}: ${mp4}, ${gif}`);
