#!/usr/bin/env node
// Measures which progress proofs keep arriving while Claude Code is silent (decisions 0181, 0185, plan
// docs/plans/agent-status-model.md A.3): a hook edge, a transcript append, a pane change (raw, and with the
// spinner line stripped) and a live child process of the agent.
//
//   node scripts/status-quiet.mjs --mode silent [--minutes 20] [--out DIR] [--dry-run]
//   node scripts/status-quiet.mjs --mode stall  [--minutes 10] [--cut-after 20] [--out DIR]
//
// silent: one foreground busy-loop tool for --minutes, then Stop.
// stall:  a local TCP proxy carries Claude's HTTPS (HTTPS_PROXY); --cut-after seconds into the inference the
//         proxy blackholes every byte, so the turn stalls the way a dead network does, without touching the host
//         network. The run ends after --minutes or when Claude reports a Stop/StopFailure.
// Prints the longest gap per proof and the longest gap with no proof at all, plus the proof counts per minute.
// Uses a real `claude` (haiku), a scratch tmux socket `tmux -L statusquiet` and a scratch directory.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

const arg = (name, dflt) => {
  const i = process.argv.indexOf(`--${name}`);
  if (i < 0) return dflt;
  const v = process.argv[i + 1];
  return v === undefined || v.startsWith('--') ? true : v;
};
const MODE = String(arg('mode', 'silent'));
const MINUTES = Number(arg('minutes', MODE === 'silent' ? 20 : 10));
const CUT_AFTER = Number(arg('cut-after', 20)) * 1000;
const OUT = path.resolve(String(arg('out', path.join(os.tmpdir(), 'status-quiet'))));
const DRY = arg('dry-run', false) === true;
const SOCK = 'statusquiet';
const SAMPLE_MS = 1000;
const EVENTS = [
  'SessionStart', 'SessionEnd', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PostToolUseFailure',
  'PermissionRequest', 'PermissionDenied', 'Notification', 'Stop', 'StopFailure',
  'SubagentStart', 'SubagentStop', 'PreCompact', 'PostCompact',
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tmux = (...a) => spawnSync('tmux', ['-L', SOCK, ...a], { encoding: 'utf8' });
const pane = () => tmux('capture-pane', '-p', '-t', 'p').stdout ?? '';
// the spinner line: "✽ Manifesting… (18s · ...)" changes every second even when nothing happens
const SPINNER = /^\s*\S\s+[A-Z][A-Za-z]+….*$/gm;
// everything under the last rule is the footer (status line clocks and usage), which ticks with no agent progress
const stripSpinner = (t) => {
  const lines = t.split('\n');
  const rule = lines.map((l) => /^─{20,}$/.test(l.trim())).lastIndexOf(true);
  // elapsed-time and token counters tick on their own ("· 3s", "(1m 2s)", "↓ 1.2k tokens")
  return (rule > 0 ? lines.slice(0, rule) : lines).join('\n').replace(SPINNER, '')
    .replace(/\b\d+h \d+m\b|\b\d+m \d+s\b|\b\d+[smh]\b/g, '#').replace(/[↑↓] [\d.]+k? tokens/g, '#').replace(/\s+$/gm, '');
};

const PROMPTS = {
  silent: `Run the shell command \`node -e "const t=Date.now();while(Date.now()-t<${MINUTES * 60000});console.log(1)"\` with the Bash tool, in the foreground, with timeout 1500000, then reply DONE.`,
  stall: 'Write a numbered list of the first 40 prime numbers with one sentence about each, then reply DONE.',
};

// Child processes of the pane's process tree: a live command under the agent is a proof (decision 0181).
function childCount() {
  const pid = tmux('display-message', '-p', '-t', 'p', '#{pane_pid}').stdout.trim();
  if (!pid) return 0;
  const ps = spawnSync('ps', ['-eo', 'pid=,ppid=,comm='], { encoding: 'utf8' }).stdout.split('\n').map((l) => l.trim().split(/\s+/));
  const kids = new Map();
  for (const [p, pp, comm] of ps) if (p) (kids.get(pp) ?? kids.set(pp, []).get(pp)).push({ p, comm });
  let n = 0;
  const walk = (x, depth) => { for (const k of kids.get(x) ?? []) { if (depth > 0) n++; walk(k.p, depth + 1); } };
  walk(pid, 0);
  return n; // everything below the agent itself (the shell is depth 0, claude depth 1)
}

// A local CONNECT proxy that can start dropping all traffic.
function proxy() {
  let cut = false;
  const socks = new Set();
  const server = net.createServer((c) => {
    socks.add(c);
    c.on('close', () => socks.delete(c));
    c.on('error', () => {});
    let head = '';
    const onData = (b) => {
      head += b.toString('latin1');
      if (!head.includes('\r\n\r\n')) return;
      c.off('data', onData);
      const m = /^CONNECT ([^:\s]+):(\d+)/.exec(head);
      if (!m) { c.end(); return; }
      const up = net.connect(Number(m[2]), m[1], () => {
        c.write('HTTP/1.1 200 Connection established\r\n\r\n');
        c.on('data', (d) => { if (!cut) up.write(d); });
        up.on('data', (d) => { if (!cut) c.write(d); });
      });
      socks.add(up);
      up.on('close', () => { socks.delete(up); c.destroy(); });
      up.on('error', () => c.destroy());
    };
    c.on('data', onData);
  });
  return new Promise((res) => server.listen(0, '127.0.0.1', () => res({
    port: server.address().port,
    cut: () => { cut = true; }, // sockets stay open and silent: a blackhole, not a reset
    close: () => { for (const s of socks) s.destroy(); server.close(); },
  })));
}

if (DRY) {
  console.log(`mode: ${MODE}, minutes: ${MINUTES}, cut-after: ${CUT_AFTER / 1000}s, out: ${OUT}`);
  console.log(`prompt: ${PROMPTS[MODE]}`);
  process.exit(0);
}

fs.mkdirSync(OUT, { recursive: true });
const dir = fs.mkdtempSync(path.join(os.tmpdir(), `statusquiet-${MODE}-`));
const log = path.join(dir, 'hooks.log');
fs.writeFileSync(log, '');
const hookCmd = `sh -c 't=$(date +%s%3N); printf "%s\\t%s\\n" "$t" "$(cat | tr "\\n" " ")" >> ${log}'`;
const settings = path.join(dir, 'settings.json');
fs.writeFileSync(settings, JSON.stringify({
  hooks: Object.fromEntries(EVENTS.map((e) => [e, [{ hooks: [{ type: 'command', command: hookCmd, timeout: 5 }] }]])),
}));
const px = MODE === 'stall' ? await proxy() : null;
const env = [
  'BASH_DEFAULT_TIMEOUT_MS=1500000', 'BASH_MAX_TIMEOUT_MS=1500000',
  ...(px ? [`HTTPS_PROXY=http://127.0.0.1:${px.port}`, `https_proxy=http://127.0.0.1:${px.port}`] : []),
].join(' ');
tmux('kill-server');
tmux('new-session', '-d', '-s', 'p', '-x', '120', '-y', '40', `cd ${dir} && env ${env} claude --model haiku --settings ${settings} --permission-mode default --allowedTools Bash`);

const keys = (...k) => tmux('send-keys', '-t', 'p', ...k);
for (let i = 0; i < 400; i++) {
  const t = pane();
  if (/trust this folder/.test(t)) { keys('Down', 'Enter'); await sleep(500); continue; }
  if (/❯ Try /.test(t) || /^❯ *$/m.test(t)) break;
  await sleep(100);
}
await sleep(1000);

// timeline: one row per second with the proofs seen in that second
const t0 = Date.now();
const rows = []; // {s, hook, transcript, pane, paneQuiet, child}
let hookOff = 0, trFile = null, trSize = 0, lastRaw = '', lastQuiet = '', cutAt = null, ended = null;
const hookEvents = [];
const diffs = []; // the first lines that changed without the spinner, to see what keeps the pane moving
tmux('send-keys', '-t', 'p', '-l', PROMPTS[MODE]);
await sleep(300);
keys('Enter');
const tSubmit = Date.now();
const deadline = tSubmit + (MINUTES + 3) * 60000;
let nextSample = Date.now();
while (Date.now() < deadline) {
  const now = Date.now();
  const row = { s: Math.round((now - tSubmit) / 1000), hook: 0, transcript: 0, pane: 0, paneQuiet: 0, child: 0 };
  const hl = fs.readFileSync(log, 'utf8');
  if (hl.length > hookOff) {
    for (const l of hl.slice(hookOff).split('\n').filter(Boolean)) {
      const json = l.slice(l.indexOf('\t') + 1);
      try {
        const h = JSON.parse(json);
        row.hook++;
        hookEvents.push({ s: Math.round((Number(l.slice(0, l.indexOf('\t'))) - tSubmit) / 1000), event: h.hook_event_name, tool: h.tool_name, error: h.error });
        if (!trFile && h.transcript_path) trFile = h.transcript_path;
        if (h.hook_event_name === 'Stop' || h.hook_event_name === 'StopFailure') ended ??= now;
      } catch { /* partial */ }
    }
    hookOff = hl.length;
  }
  if (trFile) {
    try { const sz = fs.statSync(trFile).size; if (sz > trSize) { row.transcript = 1; trSize = sz; } } catch { /* not yet */ }
  }
  const raw = pane();
  const quiet = stripSpinner(raw);
  if (raw !== lastRaw) { row.pane = 1; lastRaw = raw; }
  if (quiet !== lastQuiet) {
    row.paneQuiet = 1;
    if (diffs.length < 6) { const old = new Set(lastQuiet.split('\n')); diffs.push({ s: row.s, changed: quiet.split('\n').filter((l) => !old.has(l)) }); }
    lastQuiet = quiet;
  }
  row.child = childCount();
  rows.push(row);
  if (px && cutAt === null && now - tSubmit >= CUT_AFTER) { px.cut(); cutAt = Math.round((now - tSubmit) / 1000); }
  if (ended && now - ended > 5000) break;
  nextSample += SAMPLE_MS;
  await sleep(Math.max(0, nextSample - Date.now()));
}
const screenEnd = pane();
tmux('kill-server');
px?.close();

// longest gap (s) with none of `keys` set; the combined one excludes `pane` (the spinner makes it always on)
const longestGap = (ks) => {
  let best = 0, start = null, at = 0;
  for (const r of rows) {
    const any = ks.some((k) => (k === 'child' ? r.child > 0 : r[k] > 0));
    if (any) start = null;
    else { start ??= r.s; if (r.s - start + 1 > best) { best = r.s - start + 1; at = start; } }
  }
  return { gap_s: best, from_s: at };
};
const sums = Object.fromEntries(['hook', 'transcript', 'pane', 'paneQuiet'].map((k) => [k, rows.reduce((a, r) => a + r[k], 0)]));
const summary = {
  mode: MODE, minutes: MINUTES, cutAtSeconds: cutAt, date: new Date().toISOString(),
  version: spawnSync('claude', ['--version'], { encoding: 'utf8' }).stdout.trim(),
  samples: rows.length, counts: sums, secondsWithChild: rows.filter((r) => r.child > 0).length,
  longestGap: {
    hook: longestGap(['hook']), transcript: longestGap(['transcript']), pane_raw: longestGap(['pane']),
    pane_without_spinner: longestGap(['paneQuiet']), child: longestGap(['child']),
    hook_or_transcript: longestGap(['hook', 'transcript']),
    hook_transcript_or_pane_without_spinner: longestGap(['hook', 'transcript', 'paneQuiet']),
    any_proof: longestGap(['hook', 'transcript', 'paneQuiet', 'child']),
  },
  hookEvents, paneDiffs: diffs, endedWithHook: ended !== null, lastScreen: screenEnd.replace(/\s+$/gm, '').trimEnd(),
};
fs.writeFileSync(path.join(OUT, `${MODE}.json`), JSON.stringify({ summary, rows }, null, 1));
console.log(JSON.stringify({ ...summary, hookEvents: hookEvents.length, lastScreen: undefined }, null, 1));
if (trFile) fs.rmSync(path.dirname(trFile), { recursive: true, force: true });
fs.rmSync(dir, { recursive: true, force: true });
