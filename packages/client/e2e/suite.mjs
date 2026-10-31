// Runs a named list of e2e scripts one after another, keeps going after a failure, and lists what failed.
// Usage: node e2e/suite.mjs <server|desktop|phone|phone-real|realserver> [--shard i/n]    (after: pnpm run e2e:build; the scripts read E2E_ENGINE, E2E_VIEWPORT, E2E_DPR)
// --shard i/n runs the i-th of n parts of the list (1-based), so CI can spread a suite over n machines by changing one number.
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

const suites = {
  // Real server binary (TERMENBOX_BIN) and tmux. Chromium only: WebRTC flags and the resource gates are measured once.
  // Chromium by nature: `run` (resource gates, WebRTC), `touch` (CDP touch events), `wizard` (Chromium's fake camera flags).
  // `appshell` runs everywhere but skips its held-touch and edge-swipe steps (CDP) outside Chromium.
  server: ['run', 'touch', 'cards', 'decision', 'activity', 'windows', 'two-servers', 'appshell', 'desktop', 'newworkspace-real', 'app', 'pane', 'acp', 'acp-views',
    'hookoffer', 'add-server', 'pair', 'assisted', 'failures', 'wizard', 'search', 'push-link', 'banner-resize'],
  // The built client against scripted servers, no binary: the browser matrix runs these per engine (R36).
  desktop: ['desktop', 'newworkspace', 'xterm-dispose'],
  phone: ['appshell', 'cards', 'reorder', 'windows', 'app', 'decision', 'activity', 'failures', 'newworkspace', 'banner-resize'],
  // The phone scripts that need the real server binary (TERMENBOX_BIN) and tmux: WebKit as a phone in CI (R36). A tap is a click there (tap() in browser.mjs).
  'phone-real': ['pane', 'search', 'two-servers', 'hookoffer', 'pair', 'add-server', 'acp'],
  // The built client against the real server binary and real tmux (R36): attach, switch, reconnect, keys, selection, mouse reporting, resize.
  realserver: ['realserver'],
};

// Rough seconds per script on a CI runner; a script not listed counts as DEFAULT_WEIGHT. Only the slow ones need an entry:
// a shard takes the scripts by longest first onto the lightest part, so a new script lands where there is room and
// every script runs in exactly one shard.
const DEFAULT_WEIGHT = 15;
const weights = { desktop: 170, acp: 30, run: 30, assisted: 27, cards: 24, 'add-server': 24 };

function shardOf(list, index, count) {
  const parts = Array.from({ length: count }, () => ({ load: 0, scripts: [] }));
  const byWeight = [...list].sort((a, b) => (weights[b] ?? DEFAULT_WEIGHT) - (weights[a] ?? DEFAULT_WEIGHT));
  for (const script of byWeight) {
    const lightest = parts.reduce((m, p) => (p.load < m.load ? p : m));
    lightest.scripts.push(script);
    lightest.load += weights[script] ?? DEFAULT_WEIGHT;
  }
  return list.filter((script) => parts[index - 1].scripts.includes(script));
}

const args = process.argv.slice(2);
const name = args[0];
if (!suites[name]) { console.error(`usage: node e2e/suite.mjs <${Object.keys(suites).join('|')}> [--shard i/n]`); process.exit(2); }
let scripts = suites[name];
const shardArg = args.indexOf('--shard') >= 0 ? args[args.indexOf('--shard') + 1] : undefined;
if (shardArg !== undefined) {
  const [index, count] = (shardArg ?? '').split('/').map(Number);
  if (!(Number.isInteger(index) && Number.isInteger(count) && count >= 1 && index >= 1 && index <= count)) {
    console.error(`--shard wants i/n with 1 <= i <= n, got "${shardArg}"`); process.exit(2);
  }
  scripts = shardOf(scripts, index, count);
  if (!args.includes('--list')) console.log(`shard ${index}/${count} of ${name}: ${scripts.join(' ') || '(none)'}`);
}
if (args.includes('--list')) { console.log(scripts.join(' ')); process.exit(0); }
const failed = [];
for (const script of scripts) {
  console.log(`\n=== ${script} ===`);
  const started = Date.now();
  const { status } = spawnSync('node', [join(import.meta.dirname, `${script}.mjs`)], { stdio: 'inherit' });
  console.log(`=== ${script}: ${status === 0 ? 'passed' : 'FAILED'} in ${Math.round((Date.now() - started) / 1000)} s ===`);
  if (status !== 0) failed.push(script);
}
if (failed.length) { console.error(`\nFAILED: ${failed.join(', ')}`); process.exit(1); }
console.log(`\nsuite ${name}: all ${scripts.length} passed`);
