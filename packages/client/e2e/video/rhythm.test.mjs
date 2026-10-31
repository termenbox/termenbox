import { test, expect } from 'vitest';
import { mulberry32, seedOf, readPause, hesitation, restAfterResult, typingDelays, pointerPath } from './rhythm.mjs';

const seeds = Array.from({ length: 200 }, (_, i) => i + 1);
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

test('the same seed gives the same output', () => {
  const run = (s) => {
    const r = mulberry32(s);
    return [readPause('Tap the plus', r), hesitation(r), restAfterResult(r), typingDelays('hello world', r), pointerPath({ x: 0, y: 0 }, { x: 90, y: 40 }, r)];
  };
  expect(run(7)).toEqual(run(7));
  expect(run(7)).not.toEqual(run(8));
  expect(seedOf('recent-folder:phone')).toBe(seedOf('recent-folder:phone'));
  expect(seedOf('recent-folder:phone')).not.toBe(seedOf('recent-folder:desktop'));
});

test('a longer caption never gives a shorter reading pause', () => {
  for (const s of seeds) {
    // The same draw for both, so only the length differs; the jitter is at most 15 percent, so compare the unjittered bound.
    const short = readPause('abc', mulberry32(s));
    const long = readPause('abc'.repeat(20), mulberry32(s));
    expect(long).toBeGreaterThanOrEqual(short);
  }
});

test('reading pause bounds', () => {
  for (const s of seeds) {
    const p = readPause('x'.repeat(10), mulberry32(s));
    expect(p).toBeGreaterThanOrEqual((400 + 550) * 0.85);
    expect(p).toBeLessThanOrEqual((400 + 550) * 1.15);
    expect(readPause('x'.repeat(500), mulberry32(s))).toBeLessThanOrEqual(4500 * 1.15);
  }
});

test('hesitation and rest bounds', () => {
  for (const s of seeds) {
    const r = mulberry32(s);
    const h = hesitation(r), x = restAfterResult(r);
    expect(h).toBeGreaterThanOrEqual(500); expect(h).toBeLessThanOrEqual(1100);
    expect(x).toBeGreaterThanOrEqual(1800); expect(x).toBeLessThanOrEqual(3000);
  }
});

test('typing stalls and has a human median', () => {
  const text = 'ship the fix to the staging server now';
  const sixty = text + ' and then write the release notes now';
  expect(sixty.length).toBeGreaterThanOrEqual(60);
  for (const s of seeds) {
    const d = typingDelays(sixty.slice(0, 60), mulberry32(s));
    expect(d).toHaveLength(60);
    expect(Math.max(...d)).toBeGreaterThan(400);
    const m = median(d);
    expect(m).toBeGreaterThanOrEqual(120);
    expect(m).toBeLessThanOrEqual(230);
  }
});

test('keys that are not letters are slower', () => {
  const r = (s) => typingDelays('aaaa    ', mulberry32(s));
  const letters = seeds.map((s) => median(r(s).slice(0, 4)));
  const spaces = seeds.map((s) => median(r(s).slice(4)));
  expect(median(spaces)).toBeGreaterThan(median(letters));
});

test('every pointer path ends exactly on its target and stays within bounds', () => {
  const to = { x: 210.5, y: 480.25 };
  let overshoots = 0;
  for (const s of seeds) {
    const path = pointerPath({ x: 30, y: 700 }, to, mulberry32(s));
    const last = path[path.length - 1];
    expect(last.x).toBe(to.x); expect(last.y).toBe(to.y);
    const overshot = last.ms >= 80;
    if (overshot) {
      overshoots++;
      expect(last.ms).toBeGreaterThanOrEqual(80); expect(last.ms).toBeLessThanOrEqual(160);
      expect(path.length).toBeGreaterThanOrEqual(25); expect(path.length).toBeLessThanOrEqual(41);
      const peak = path[path.length - 2];
      const off = Math.hypot(peak.x - to.x, peak.y - to.y);
      expect(off).toBeGreaterThanOrEqual(3.9); expect(off).toBeLessThanOrEqual(10.1);
    } else {
      expect(path.length).toBeGreaterThanOrEqual(24); expect(path.length).toBeLessThanOrEqual(40);
    }
    for (const p of path.slice(0, -1)) { expect(p.ms).toBeGreaterThanOrEqual(10); expect(p.ms).toBeLessThanOrEqual(22); }
  }
  expect(overshoots / seeds.length).toBeGreaterThan(0.25);
  expect(overshoots / seeds.length).toBeLessThan(0.55);
});
