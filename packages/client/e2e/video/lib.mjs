// A page driven the way a person uses it, for the feature videos: a visible pointer (a cursor on desktop, a translucent touch
// spot on a phone), moves that overshoot and correct, typing in bursts with stalls, a reading pause after each state change, and
// a caption bar that stays clear of the control being used. The rhythm is rhythm.mjs, seeded so a render repeats.
// Nothing here asserts the app: the cases do.
import { stepTracker } from './steps.mjs';
import { mulberry32, seedOf, readPause, hesitation, restAfterResult, typingDelays, pointerPath } from './rhythm.mjs';
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let assertions = 0;
/** Cases call this (through their expectThat) each time a state check passes; a still may only follow one. */
export const noteAssertion = () => { assertions++; };

/** The multiplier for every human delay: `normal` leaves them as they are, `slow` stretches them (the /regen pace=slow knob). */
export const paceFactor = (pace = 'normal') => {
  if (pace === 'normal') return 1;
  if (pace === 'slow') return 1.6;
  throw new Error(`pace must be slow or normal, not ${JSON.stringify(pace)}`);
};

/** Runs in every page: the pointer and the caption live in the top layer (a popover), so they paint above a modal <dialog>. */
export const overlayScript = (phone) => {
  const mount = () => {
    if (document.getElementById('__human')) return;
    const root = document.createElement('div');
    root.id = '__human';
    root.setAttribute('popover', 'manual');
    root.style.cssText = 'all:initial;position:fixed;inset:0;margin:0;padding:0;border:0;background:none;pointer-events:none;overflow:visible;';
    const spot = document.createElement('div');
    spot.style.cssText = phone
      ? 'position:fixed;left:0;top:0;width:44px;height:44px;margin:-22px 0 0 -22px;border-radius:50%;background:rgba(120,160,255,.35);border:2px solid rgba(255,255,255,.8);opacity:0;transition:transform .08s,opacity .15s;'
      : 'position:fixed;left:0;top:0;width:18px;height:18px;margin:-2px 0 0 -2px;opacity:0;background:url("data:image/svg+xml,%3Csvg xmlns=%27http://www.w3.org/2000/svg%27 viewBox=%270 0 18 18%27%3E%3Cpath d=%27M1 1l6 15 2.2-6.2L15.5 7.6z%27 fill=%27white%27 stroke=%27black%27 stroke-width=%271.2%27 stroke-linejoin=%27round%27/%3E%3C/svg%3E") no-repeat;transition:opacity .15s;';
    const ring = document.createElement('div');
    ring.style.cssText = 'position:fixed;left:0;top:0;width:36px;height:36px;margin:-18px 0 0 -18px;border-radius:50%;border:3px solid rgba(255,255,255,.9);box-shadow:0 0 0 1px rgba(0,0,0,.4);opacity:0;';
    const cap = document.createElement('div');
    cap.id = '__caption';
    cap.style.cssText = 'position:fixed;left:50%;transform:translateX(-50%);max-width:min(92vw,520px);padding:8px 14px;border-radius:10px;background:rgba(10,10,14,.88);color:#fff;font:600 15px/1.35 system-ui,sans-serif;text-align:center;opacity:0;transition:opacity .2s;bottom:14px;';
    root.append(spot, ring, cap);
    (document.body ?? document.documentElement).append(root);
    const w = window;
    w.__human = { root, spot, ring, cap };
    root.showPopover?.();
  };
  const ready = () => { if (document.body) mount(); else document.addEventListener('DOMContentLoaded', mount, { once: true }); };
  ready();
  // A later modal dialog covers an earlier top-layer element; showing the overlay again moves it back on top. Only when a modal
  // opened since the last time: hiding and showing the popover drops the overlay from the frame recorded in between (a flicker).
  let raisedOver = null;
  window.__humanTop = () => {
    const h = window.__human;
    if (!h) return;
    const modals = [...document.querySelectorAll('dialog')].filter((d) => d.matches(':modal'));
    const top = modals[modals.length - 1] ?? null;
    if (top === raisedOver && h.root.matches(':popover-open')) return;
    raisedOver = top;
    try { h.root.hidePopover(); h.root.showPopover(); } catch { /* not supported: the caption still shows in the page */ }
  };
  window.__humanMove = (x, y) => {
    const h = window.__human;
    if (!h) return;
    // The first move puts the pointer where it starts: without this it flies in from the corner of the screen.
    const first = h.spot.style.opacity !== '1';
    const kept = h.spot.style.transition;
    if (first) { h.spot.style.transition = 'none'; h.spot.style.transform = `translate(${x}px, ${y}px)`; void h.spot.offsetWidth; h.spot.style.transition = kept; }
    h.spot.style.opacity = '1';
    h.spot.style.transform = `translate(${x}px, ${y}px)`;
    h.ring.style.left = `${x}px`; h.ring.style.top = `${y}px`;
  };
  window.__humanPulse = () => {
    const h = window.__human;
    if (!h) return;
    h.ring.animate([{ opacity: 1, transform: 'scale(.4)' }, { opacity: 0, transform: 'scale(1.6)' }], { duration: 380, easing: 'ease-out' });
  };
  window.__humanCaption = (text, atTop) => {
    const h = window.__human;
    if (!h) return;
    h.cap.textContent = text;
    h.cap.style.top = atTop ? '14px' : 'auto';
    h.cap.style.bottom = atTop ? 'auto' : '14px';
    h.cap.style.opacity = text ? '1' : '0';
  };
};

/**
 * Wraps `page` with human-paced actions. `profile` is 'phone' (touch) or 'desktop' (mouse). `pace` is 'normal' or 'slow';
 * `caption: false` keeps the caption placement logic but draws nothing. `seed` (the case id) with the profile seeds the rhythm.
 * `steps` are the case's declared steps: `h.step(id)` shows a step's caption and checks the order.
 */
export function humanPage(page, profile, { pace = 'normal', caption: drawCaptions = true, seed = '', steps = [] } = {}) {
  const k = paceFactor(pace);
  const rng = mulberry32(seedOf(`${seed}:${profile}`));
  const tracker = stepTracker(steps);
  const wait = (ms) => sleep(ms * k);
  const phone = profile === 'phone';
  const vp = page.viewportSize();
  let at = { x: vp.width * 0.5, y: vp.height * 0.85 };
  let bottomBar = true;

  let lastStill = 0;
  const h = {
    page, phone,
    /** Stills taken so far: {name, png}. render.mjs writes them only when the whole case passed. */
    stills: [],
    /** [epoch milliseconds, x, y] of every pointer move (CSS pixels): the pointer is moved on purpose, so the flicker check leaves it out. */
    pointer: [],
    /**
     * A screenshot of the page at the context's pixel ratio, with the pointer and caption as they are on screen. Call it right after an
     * expectThat: it throws when no assertion has passed since the last still (GUARDRAIL ux-scenarios-assert-a-state-before-they-capture-it).
     */
    async still(name) {
      if (assertions === lastStill) throw new Error(`still ${name}: no state was asserted before it`);
      lastStill = assertions;
      await h.pause(400);
      // The overlay stays on screen: hiding it for the screenshot drops the caption and pointer from about five recorded frames.
      const png = await page.screenshot({ type: 'png' });
      h.stills.push({ name, png });
    },
    top: () => page.evaluate(() => window.__humanTop?.()),
    /** A reader's pause after something changed: `ms` when given, else the time to read the caption on screen. */
    pause: (ms = readPause(h.pending ?? '', rng)) => wait(ms),
    /** A rest on a result worth noticing; cases call it after the state they want the viewer to take in. */
    result: async (text) => {
      if (text) await h.caption(text);
      await wait(restAfterResult(rng));
    },
    /** Shows `text`; it sits on the side of the screen away from `avoid`, the control about to be used. */
    async caption(text, avoid) {
      if (avoid) {
        const b = await avoid.boundingBox();
        if (b) bottomBar = b.y + b.height / 2 < vp.height * 0.6;
      }
      if (!drawCaptions) return;
      await h.top();
      await page.evaluate(([t, top]) => window.__humanCaption?.(t, top), [text, !bottomBar]);
    },
    clearCaption: () => page.evaluate(() => window.__humanCaption?.('', false)),
    /** Moves the pointer to `locator` along an eased curve that may overshoot and correct (rhythm.mjs). Returns the point. */
    async move(locator) {
      await locator.scrollIntoViewIfNeeded();
      const b = await locator.boundingBox();
      if (!b) throw new Error('video: nothing to point at');
      const to = { x: b.x + b.width * (0.4 + rng() * 0.2), y: b.y + b.height * (0.4 + rng() * 0.2) };
      await h.top();
      for (const { x, y, ms } of pointerPath(at, to, rng)) {
        if (!phone) await page.mouse.move(x, y);
        h.pointer.push([Date.now(), x, y]);
        await page.evaluate(([px, py]) => window.__humanMove?.(px, py), [x, y]);
        await wait(ms);
      }
      at = to;
      await wait(hesitation(rng));
      return to;
    },
    /** Points at `locator`, then presses it (a touch on a phone, a click on desktop) and waits for the screen to settle. */
    async tap(locator) {
      await locator.waitFor({ state: 'visible', timeout: 5000 });
      await h.caption(h.pending ?? '', locator);
      const p = await h.move(locator);
      await page.evaluate(() => window.__humanPulse?.());
      if (phone) await page.touchscreen.tap(p.x, p.y);
      else await page.mouse.click(p.x, p.y);
      await h.pause();
    },
    /** Types into `locator` in bursts with stalls (rhythm.mjs), after pointing at it. */
    async type(locator, text) {
      await h.tap(locator);
      await locator.fill('');
      await h.typeText(text, (ch) => page.keyboard.type(ch));
      await h.pause();
    },
    /** Types `text` one key at a time at human pace, handing each key to `sink` (a terminal page types into xterm, not an input). */
    async typeText(text, sink) {
      const gaps = typingDelays(text, rng);
      let i = 0;
      for (const ch of text) { await wait(gaps[i++]); await sink(ch); }
    },
    /** Waits `ms`, stretched by the pace knob like every other human delay. */
    wait,
    /** Performs the declared step `id`: shows its caption (away from `locator`, the control about to be used). Throws out of order or unknown. */
    async step(id, locator) {
      await h.say(tracker.reach(id).caption, locator);
    },
    /** Throws when a declared step was never reached; the render calls it after the case ran. */
    assertSteps: () => tracker.assertComplete(),
    /** Sets the caption for the next move, shows it, and holds it long enough to read. */
    async say(text, locator) {
      h.pending = text;
      await h.caption(text, locator);
      await wait(readPause(text, rng));
    },
  };
  return h;
}
