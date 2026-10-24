// Runs a named list of e2e scripts one after another, keeps going after a failure, and lists what failed.
// Usage: node e2e/suite.mjs <server|desktop|phone|realserver>    (after: pnpm run e2e:build; the scripts read E2E_ENGINE, E2E_VIEWPORT, E2E_DPR)
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

const suites = {
  // Real server binary (TERMENBOX_BIN) and tmux. Chromium only: WebRTC flags and the resource gates are measured once.
  server: ['run', 'touch', 'cards', 'decision', 'activity', 'windows', 'two-servers', 'appshell', 'desktop', 'app', 'pane', 'acp',
    'hookoffer', 'add-server', 'pair', 'failures', 'wizard', 'search', 'push-link'],
  // The built client against scripted servers, no binary: the browser matrix runs these per engine (R36).
  desktop: ['desktop'],
  phone: ['appshell', 'cards', 'windows', 'app'],
  // The built client against the real server binary and real tmux (R36): attach, switch, reconnect, keys, selection, mouse reporting, resize.
  realserver: ['realserver'],
};

const name = process.argv[2];
if (!suites[name]) { console.error(`usage: node e2e/suite.mjs <${Object.keys(suites).join('|')}>`); process.exit(2); }
const failed = [];
for (const script of suites[name]) {
  console.log(`\n=== ${script} ===`);
  const started = Date.now();
  const { status } = spawnSync('node', [join(import.meta.dirname, `${script}.mjs`)], { stdio: 'inherit' });
  console.log(`=== ${script}: ${status === 0 ? 'passed' : 'FAILED'} in ${Math.round((Date.now() - started) / 1000)} s ===`);
  if (status !== 0) failed.push(script);
}
if (failed.length) { console.error(`\nFAILED: ${failed.join(', ')}`); process.exit(1); }
console.log(`\nsuite ${name}: all ${suites[name].length} passed`);
