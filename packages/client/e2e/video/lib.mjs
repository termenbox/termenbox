// A page driven the way a person uses it, for the feature videos: a visible pointer (a cursor on desktop, a translucent touch
// spot on a phone), eased moves with a settle before the press, typing at a human pace, a pause after each state change so a
// reader can read it, and a caption bar that stays clear of the control being used. Nothing here asserts the app: the cases do.
const rand = (lo, hi) => lo + Math.random() * (hi - lo);
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ease = (t) => (t < 0.5 ? 2 * t * t : 1 - ((-2 * t + 2) ** 2) / 2);

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
  // A later modal dialog covers an earlier top-layer element; showing the overlay again moves it back on top.
  window.__humanTop = () => {
    const h = window.__human;
    if (!h) return;
    try { h.root.hidePopover(); h.root.showPopover(); } catch { /* not supported: the caption still shows in the page */ }
  };
  window.__humanMove = (x, y) => {
    const h = window.__human;
    if (!h) return;
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

/** Wraps `page` with human-paced actions. `profile` is 'phone' (touch) or 'desktop' (mouse). */
export function humanPage(page, profile) {
  const phone = profile === 'phone';
  const vp = page.viewportSize();
  let at = { x: vp.width * 0.5, y: vp.height * 0.85 };
  let bottomBar = true;

  const h = {
    page, phone,
    top: () => page.evaluate(() => window.__humanTop?.()),
    /** A reader's pause after something changed. */
    pause: (ms = rand(600, 1200)) => sleep(ms),
    /** Shows `text`; it sits on the side of the screen away from `avoid`, the control about to be used. */
    async caption(text, avoid) {
      if (avoid) {
        const b = await avoid.boundingBox();
        if (b) bottomBar = b.y + b.height / 2 < vp.height * 0.6;
      }
      await h.top();
      await page.evaluate(([t, top]) => window.__humanCaption?.(t, top), [text, !bottomBar]);
    },
    clearCaption: () => page.evaluate(() => window.__humanCaption?.('', false)),
    /** Moves the pointer to the middle of `locator` in 18 to 30 eased steps, then settles 120 to 350 ms. Returns the point. */
    async move(locator) {
      await locator.scrollIntoViewIfNeeded();
      const b = await locator.boundingBox();
      if (!b) throw new Error('video: nothing to point at');
      const to = { x: b.x + b.width * rand(0.4, 0.6), y: b.y + b.height * rand(0.4, 0.6) };
      const steps = Math.round(rand(18, 30));
      const from = at;
      await h.top();
      for (let i = 1; i <= steps; i++) {
        const k = ease(i / steps);
        const x = from.x + (to.x - from.x) * k, y = from.y + (to.y - from.y) * k;
        if (!phone) await page.mouse.move(x, y);
        await page.evaluate(([px, py]) => window.__humanMove?.(px, py), [x, y]);
        await sleep(rand(10, 22));
      }
      at = to;
      await sleep(rand(120, 350));
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
    /** Types into `locator` at 55 to 140 ms per key, after pointing at it. */
    async type(locator, text) {
      await h.tap(locator);
      await locator.fill('');
      for (const ch of text) { await page.keyboard.type(ch); await sleep(rand(55, 140)); }
      await h.pause();
    },
    /** Sets the caption for the next move, shows it, and holds it long enough to read. */
    async say(text, locator) {
      h.pending = text;
      await h.caption(text, locator);
      await sleep(Math.min(1800, 500 + text.length * 35));
    },
  };
  return h;
}
