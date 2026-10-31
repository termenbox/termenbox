// The rhythm of a person using the app, for the feature videos (DECISION-0161): reading before acting, a beat of doubt
// before a choice, typing in bursts with stalls, a pointer that overshoots and corrects. Every function is pure given a
// seeded generator, so a render is reproducible and its input hash does not change.
// Durations are milliseconds at pace `normal`; humanPage multiplies them by paceFactor.

/** A small seeded generator returning floats in [0, 1). */
export const mulberry32 = (seed) => {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};

/** A stable 32-bit seed from a string, such as `${caseId}:${profile}`. */
export const seedOf = (text) => {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  return h >>> 0;
};

const between = (rng, lo, hi) => lo + rng() * (hi - lo);

/** Time to read a caption: 400 ms + 55 ms per character, capped at 4500 ms, plus or minus 15 percent. */
export const readPause = (text, rng) => Math.min(4500, 400 + 55 * text.length) * between(rng, 0.85, 1.15);

/** The beat of doubt before a choice. */
export const hesitation = (rng) => between(rng, 500, 1100);

/** The rest on a result worth noticing. */
export const restAfterResult = (rng) => between(rng, 1800, 3000);

/**
 * The gap before each key of `text`: bursts of 3 to 6 keys at 0.6 of the base, a stall of 250 to 600 ms after each burst,
 * non-letters 1.4 times slower. The base is 230 to 340 ms, not the 140 to 230 of the plan: at 0.6 of that the median key would
 * fall under the 120 ms the plan also requires, so the base is lifted until both hold.
 */
export const typingDelays = (text, rng) => {
  const base = between(rng, 230, 340);
  const delays = [];
  let burst = Math.round(between(rng, 3, 6));
  for (const ch of text) {
    let d = base * 0.6 * between(rng, 0.9, 1.1);
    if (!/\p{L}/u.test(ch)) d *= 1.4;
    if (burst === 0) {
      d += between(rng, 250, 600);
      burst = Math.round(between(rng, 3, 6));
    }
    burst--;
    delays.push(d);
  }
  return delays;
};

const ease = (t) => (t < 0.5 ? 2 * t * t : 1 - ((-2 * t + 2) ** 2) / 2);

const bezier = (p0, p1, p2, p3, t) => {
  const u = 1 - t;
  return {
    x: u * u * u * p0.x + 3 * u * u * t * p1.x + 3 * u * t * t * p2.x + t * t * t * p3.x,
    y: u * u * u * p0.y + 3 * u * u * t * p1.y + 3 * u * t * t * p2.y + t * t * t * p3.y,
  };
};

/**
 * A pointer move as samples {x, y, ms} (ms is the wait after the sample): a cubic Bezier of 24 to 40 samples with ease in
 * and out; on 40 percent of moves it overshoots the target by 4 to 10 px and corrects in 80 to 160 ms. It ends exactly on `to`.
 */
export const pointerPath = (from, to, rng) => {
  const dx = to.x - from.x, dy = to.y - from.y;
  const len = Math.hypot(dx, dy) || 1;
  const nx = -dy / len, ny = dx / len;
  const bend = between(rng, -0.15, 0.15) * len;
  const c1 = { x: from.x + dx * 0.3 + nx * bend, y: from.y + dy * 0.3 + ny * bend };
  const c2 = { x: from.x + dx * 0.7 + nx * bend, y: from.y + dy * 0.7 + ny * bend };
  const overshoot = rng() < 0.4;
  const over = between(rng, 4, 10);
  const end = overshoot ? { x: to.x + (dx / len) * over, y: to.y + (dy / len) * over } : to;
  const steps = Math.round(between(rng, 24, 40));
  const path = [];
  for (let i = 1; i <= steps; i++) {
    const p = bezier(from, c1, c2, end, ease(i / steps));
    path.push({ x: p.x, y: p.y, ms: between(rng, 10, 22) });
  }
  if (overshoot) path.push({ x: to.x, y: to.y, ms: between(rng, 80, 160) });
  else path[path.length - 1] = { x: to.x, y: to.y, ms: path[path.length - 1].ms };
  return path;
};
