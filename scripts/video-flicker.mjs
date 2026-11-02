// Finds text that flickers on a feature video: a region that changes for one frame and changes back (A, B, A) while nothing
// else moved. Real motion (a caption fading, a page scrolling) does not return to the earlier frame, so it is not reported.
//   node scripts/video-flicker.mjs [--max N] docs/media/*.mp4     exit 1 when any video has more than N bounce frames (default 0)
//   node scripts/video-flicker.mjs --self-test
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';

const PIXEL = 40;      // a gray level change this large is a real change, not encoder noise
const MIN_AREA = 12;   // fewer changed pixels than this is ignored
const MAX_AREA = 0.04; // a bounce that touches more than this share of the frame is a cut or a transition, not text
const MIRROR = 0.75;   // the change back must be this share of the change out: a glitch undoes itself exactly, a pointer wobble does not

/**
 * Changed pixels between two gray frames of width `w`. A pixel counts only when its left and upper neighbours changed too, so a
 * one-pixel seam (a QR edge, a border the encoder re-draws) is not counted: it is not text, and the eye does not see it.
 */
export const changed = (a, b, w, skipPx = null) => {
  const hit = (i) => !(skipPx && skipPx[i]) && Math.abs(a[i] - b[i]) > PIXEL;
  let n = 0;
  for (let i = w + 1; i < a.length; i++) if (hit(i) && hit(i - 1) && hit(i - w)) n++;
  return n;
};

/** Frame indexes (the middle frame of each A, B, A) where a small region changed and changed straight back. */
export function bounces(counts) {
  // counts.step[i] = pixels changed between frame i-1 and i; counts.skip[i] = between frame i-2 and i. With a pointer mask the
  // three numbers a bounce compares are measured on one mask, so counts.tri[i] = [step[i-1], step[i], skip[i]] stands in for them.
  const out = [];
  const { step, skip, tri, size } = counts;
  for (let i = 2; i < step.length; i++) {
    const [before, after, skipped] = tri?.[i] ?? [step[i - 1], step[i], skip[i]];
    const moved = before >= MIN_AREA && after >= MIN_AREA;
    const back = skipped < MIN_AREA && Math.min(before, after) >= MIRROR * Math.max(before, after);
    if (moved && back && before <= size * MAX_AREA) out.push(i - 1);
  }
  return out;
}

/** Pixels the pointer may move through around time `t`: a box of `r` pixels about each sample [t, x, y, r] within WINDOW seconds. */
const WINDOW = 0.8;
export function pointerMask(samples, t, w, h, into, keep = false) {
  if (!keep) into.fill(0);
  for (const [st, x, y, r] of samples) {
    if (Math.abs(st - t) > WINDOW) continue;
    for (let yy = Math.max(0, Math.floor(y - r)); yy < Math.min(h, Math.ceil(y + r)); yy++) into.fill(1, yy * w + Math.max(0, Math.floor(x - r)), yy * w + Math.min(w, Math.ceil(x + r)));
  }
  return into;
}

/**
 * Streams the video as gray frames and returns the per-frame change counts. `pointer` is the recorder's own pointer path as
 * [seconds, x, y, radius] in the video's pixels: the pointer moves on purpose (it overshoots and corrects), so it is not text
 * that flickers and its box is left out of the comparison.
 */
export async function scan(file, pointer = []) {
  const probe = spawnSync('ffprobe', ['-v', 'error', '-select_streams', 'v:0', '-show_entries', 'stream=width,height,r_frame_rate', '-of', 'csv=p=0', file], { encoding: 'utf8' });
  if (probe.status !== 0) throw new Error(`ffprobe could not read ${file}`);
  const [w, h, rate] = probe.stdout.trim().split(',');
  const size = Number(w) * Number(h);
  const [num, den] = rate.split('/').map(Number);
  const fps = num / (den || 1);
  const ff = spawn('ffmpeg', ['-v', 'error', '-i', file, '-f', 'rawvideo', '-pix_fmt', 'gray', '-'], { stdio: ['ignore', 'pipe', 'inherit'] });
  const step = [0], skip = [0, 0];
  const mask = pointer.length ? new Uint8Array(size) : null;
  const tri = [];
  let index = 0;
  let prev = null, prev2 = null, buf = Buffer.alloc(0);
  for await (const chunk of ff.stdout) {
    buf = buf.length ? Buffer.concat([buf, chunk]) : chunk;
    while (buf.length >= size) {
      const frame = Buffer.from(buf.subarray(0, size));
      buf = buf.subarray(size);
      if (prev) step.push(changed(prev, frame, Number(w)));
      if (prev2) skip.push(changed(prev2, frame, Number(w)));
      if (mask && prev2) {
        // one mask for the three frames a bounce compares: where the pointer was from the first to the last of them
        mask.fill(0);
        for (const k of [index - 2, index - 1, index]) pointerMask(pointer, k / fps, Number(w), Number(h), mask, true);
        tri[index] = [changed(prev2, prev, Number(w), mask), changed(prev, frame, Number(w), mask), changed(prev2, frame, Number(w), mask)];
      }
      prev2 = prev; prev = frame; index++;
    }
  }
  return { step, skip, size, fps, tri: mask ? tri : undefined };
}

async function check(files, max, pointer = []) {
  let bad = 0;
  for (const f of files) {
    const counts = await scan(f, pointer);
    const hits = bounces(counts);
    const verdict = hits.length > max ? 'FLICKER' : 'ok';
    console.log(`${verdict.padEnd(8)}${f}: ${hits.length} bounce frame(s)${hits.length ? ` at ${hits.slice(0, 8).map((i) => (i / counts.fps).toFixed(2) + 's').join(', ')}${hits.length > 8 ? ', …' : ''}` : ''}`);
    if (hits.length > max) bad++;
  }
  return bad;
}

async function selfTest() {
  // pure: a one-frame blip is a bounce, a step change and a fade are not
  const mk = (frames) => { const step = [0], skip = [0, 0]; for (let i = 1; i < frames.length; i++) step.push(Math.abs(frames[i] - frames[i - 1]) > 0 ? 100 : 0); for (let i = 2; i < frames.length; i++) skip.push(frames[i] === frames[i - 2] ? 0 : 100); return { step, skip, size: 1e6 }; };
  assert.deepEqual(bounces(mk([0, 0, 1, 0, 0])), [2]);
  assert.deepEqual(bounces(mk([0, 0, 1, 1, 1])), []);
  assert.deepEqual(bounces(mk([0, 1, 2, 3, 4])), []);
  assert.deepEqual(bounces({ step: [0, 77, 17], skip: [0, 0, 4], size: 1e6 }), [], 'a pointer wobble undoes itself only in part');
  const filters = spawnSync('ffmpeg', ['-hide_banner', '-filters'], { encoding: 'utf8' }).stdout ?? '';
  if (!/\bdrawtext\b/.test(filters)) { console.log('video-flicker self-test ok (no ffmpeg drawtext here: the video part was skipped)'); return; }
  // real: ffmpeg draws a still frame, a frame with one extra word for a single frame, and the still again
  const dir = mkdtempSync(join(tmpdir(), 'flicker-'));
  try {
    const make = (name, flick) => {
      const text = (on) => `drawtext=text='Connected to the server':x=20:y=40:fontsize=24:fontcolor=white${on ? `:enable='eq(n,${flick})'` : ''}`;
      const vf = flick == null ? text(false) : `drawtext=text='Connected to the server':x=20:y=40:fontsize=24:fontcolor=white,drawtext=text='XXXXXXXXXXXX':x=20:y=90:fontsize=24:fontcolor=white:enable='eq(n,${flick})'`;
      const r = spawnSync('ffmpeg', ['-v', 'error', '-y', '-f', 'lavfi', '-i', 'color=c=black:s=320x200:r=30:d=2', '-vf', vf, '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '12', join(dir, name)], { encoding: 'utf8' });
      assert.equal(r.status, 0, r.stderr);
      return join(dir, name);
    };
    assert.equal(bounces(await scan(make('still.mp4', null))).length, 0, 'a still video has no bounce');
    assert.equal(bounces(await scan(make('blip.mp4', 20))).length, 1, 'a one-frame change is a bounce');
    // the same blip inside the pointer's box is the pointer, not text: the word is drawn at x 20..260, y 90..114
    assert.equal(bounces(await scan(join(dir, 'blip.mp4'), [[20 / 30, 140, 100, 160]])).length, 0, 'a change under the pointer is not a bounce');
    assert.equal(bounces(await scan(join(dir, 'blip.mp4'), [[20 / 30, 300, 20, 10]])).length, 1, 'a pointer elsewhere hides nothing');
  } finally { rmSync(dir, { recursive: true, force: true }); }
  console.log('video-flicker self-test ok');
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) { await selfTest(); process.exit(0); }
  const mi = args.indexOf('--max');
  const max = mi >= 0 ? Number(args.splice(mi, 2)[1]) : 0;
  const pi = args.indexOf('--pointer');
  const pointer = pi >= 0 ? JSON.parse(readFileSync(args.splice(pi, 2)[1], 'utf8')) : [];
  if (!args.length) { console.error('usage: node scripts/video-flicker.mjs [--max N] [--pointer path.json] <video.mp4>...'); process.exit(2); }
  process.exit((await check(args, max, pointer)) ? 1 : 0);
}
