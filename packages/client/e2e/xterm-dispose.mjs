// A terminal that is closed while xterm still has a frame of work queued must not throw. xterm 5.3's Viewport read
// renderService.dimensions from a requestAnimationFrame callback that dispose did not cancel, so closing a workspace right
// after output reached it raised "Cannot read properties of undefined (reading 'dimensions')" in the page (#406).
// Fixed in @xterm/xterm 5.5. Runs the installed build in the selected engine; XTERM_DIR points at another build to check it.
// Usage: node e2e/xterm-dispose.mjs
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { launch, contextOptions } from './browser.mjs';

const root = process.env.XTERM_DIR ?? dirname(createRequire(import.meta.url).resolve('@xterm/xterm/package.json'));
const js = readFileSync(join(root, 'lib/xterm.js'), 'utf8');
const css = readFileSync(join(root, 'css/xterm.css'), 'utf8');

// Each body runs in the page after a terminal is open in #t; it resolves once its dispose has been called.
const frame = 'new Promise((r) => requestAnimationFrame(r))';
const scenarios = {
  'disposed in the frame it was opened': 'term.dispose();',
  'disposed right after a write that changes the buffer': `term.write('a\\r\\nb\\r\\n'); term.dispose();`,
  'disposed after a later write, with the frame still queued': `await ${frame}; await ${frame}; term.write('x\\r\\n'.repeat(60)); term.dispose();`,
  'disposed once settled': 'await new Promise((r) => setTimeout(r, 400)); term.dispose();',
};

const browser = await launch();
let failed = false;
try {
  for (const [name, body] of Object.entries(scenarios)) {
    const ctx = await browser.newContext(contextOptions());
    const page = await ctx.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.setContent(`<style>${css}</style><div id="t" style="width:600px;height:300px"></div>`);
    await page.addScriptTag({ content: js });
    await page.evaluate(`(async () => { const term = new Terminal(); term.open(document.getElementById('t')); ${body} })()`);
    await page.waitForTimeout(400); // the queued frames and timers run now, against a disposed terminal
    if (errors.length) { failed = true; console.error(`FAIL: ${name}: ${[...new Set(errors)].join(' | ')}`); } else console.log(`ok  ${name}`);
    await ctx.close();
  }
} finally {
  await browser.close();
}
if (failed) process.exit(1);
console.log('PASS');
