#!/usr/bin/env node
// Measures the lag between the three sources of agent status for Claude Code: a hook, the session
// transcript append and the first pane redraw (decision 0182, plan docs/plans/agent-status-model.md A.1).
//
//   node scripts/status-lag.mjs [--runs 5] [--cases plain,permission] [--out DIR] [--fixtures] [--dry-run]
//
// Drives a real `claude` (haiku, small prompts) in a scratch tmux socket (`tmux -L statuslag`) inside a scratch
// directory, never the owner's sessions. One clock (Date.now() in this process; the hook script reads `date`
// on the same machine). Prints, per case and mark, the median and p95 lag of each source pair, and with
// --fixtures keeps the first run's hook payloads, transcript lines and screens under packages/server/testdata.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const arg = (name, dflt) => {
  const i = process.argv.indexOf(`--${name}`);
  if (i < 0) return dflt;
  const v = process.argv[i + 1];
  return v === undefined || v.startsWith('--') ? true : v;
};
const RUNS = Number(arg('runs', 5));
const ONLY = arg('cases', '') ? String(arg('cases', '')).split(',') : null;
const OUT = path.resolve(String(arg('out', path.join(os.tmpdir(), 'status-lag'))));
const FIXTURES = arg('fixtures', false) === true;
const DRY = arg('dry-run', false) === true;
const SOCK = 'statuslag';
const EVENTS = [
  'SessionStart', 'SessionEnd', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PostToolUseFailure',
  'PermissionRequest', 'PermissionDenied', 'Elicitation', 'ElicitationResult', 'Notification', 'Stop',
  'StopFailure', 'SubagentStart', 'SubagentStop', 'PreCompact', 'PostCompact',
];
const POLL_MS = 15;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tmux = (...a) => spawnSync('tmux', ['-L', SOCK, ...a], { encoding: 'utf8' });
const pane = () => tmux('capture-pane', '-p', '-t', 'p').stdout ?? '';
// the spinner line: "✽ Manifesting… (18s · ...)"; the echoed prompt (lines starting with ❯) is not content
const working = (t) => /^\s*\S\s+[A-Z][A-Za-z]+…\s*\(/m.test(t);

// A mark is one thing that happens in a case, seen by up to three sources. `hook`/`line` take the parsed
// object, `screen` takes the pane text. A mark with `base: 'sent'` has no hook and is timed from the key we sent.
const BASH_OK = '--permission-mode default --allowedTools Bash';
const CASES = {
  plain: {
    args: '--permission-mode default',
    prompt: 'Reply with the single word OK and nothing else.',
    marks: {
      turn_start: { hook: (h) => h.hook_event_name === 'UserPromptSubmit', line: (l) => l.type === 'user', screen: working },
      turn_end: { hook: (h) => h.hook_event_name === 'Stop', line: (l) => l.type === 'assistant' && l.message?.stop_reason === 'end_turn', screen: (t, c) => c.seen.turn_start && !working(t) && /^● OK\b/m.test(t) },
    },
    done: (c) => c.seen.turn_end,
  },
  tool: {
    args: BASH_OK,
    prompt: 'Run the shell command `echo lagprobe` with the Bash tool, then reply DONE.',
    marks: {
      tool_start: { hook: (h) => h.hook_event_name === 'PreToolUse' && h.tool_name === 'Bash', line: (l) => has(l, 'tool_use', 'Bash'), screen: (t) => /⎿\s+\$ echo lagprobe/.test(t) },
      tool_end: { hook: (h) => h.hook_event_name === 'PostToolUse' && h.tool_name === 'Bash', line: (l, c) => has(l, 'tool_result') && c.at.tool_start?.transcript !== undefined, screen: (t) => /Ran 1 shell command/.test(t) },
      turn_end: { hook: (h) => h.hook_event_name === 'Stop', line: (l) => l.type === 'assistant' && l.message?.stop_reason === 'end_turn', screen: (t, c) => c.seen.tool_end && !working(t) },
    },
    done: (c) => c.seen.turn_end,
  },
  permission: {
    args: '--permission-mode default',
    prompt: 'Run the shell command `touch lagprobe.txt` with the Bash tool, then reply DONE.',
    marks: {
      ask_open: { hook: (h) => h.hook_event_name === 'PermissionRequest', line: (l) => has(l, 'tool_use', 'Bash'), screen: (t) => /Do you want to proceed/.test(t) },
      notification: { hook: (h) => h.hook_event_name === 'Notification' },
      answered: { base: 'sent', hook: (h) => h.hook_event_name === 'PostToolUse' && h.tool_name === 'Bash', line: (l, c) => has(l, 'tool_result') && c.at.ask_open?.transcript !== undefined, screen: (t, c) => c.seen.ask_open && !/Do you want to proceed/.test(t) },
      turn_end: { hook: (h) => h.hook_event_name === 'Stop', line: (l) => l.type === 'assistant' && l.message?.stop_reason === 'end_turn' },
    },
    on: { ask_open: async (c) => { await sleep(1500); c.mark('sent'); c.keys('1'); } },
    done: (c) => c.seen.turn_end,
  },
  permission_esc: {
    args: '--permission-mode default',
    prompt: 'Run the shell command `touch lagprobe.txt` with the Bash tool, then reply DONE.',
    marks: {
      ask_open: { hook: (h) => h.hook_event_name === 'PermissionRequest', line: (l) => has(l, 'tool_use', 'Bash'), screen: (t) => /Do you want to proceed/.test(t) },
      cancelled: { base: 'sent', line: (l) => /interrupted/i.test(JSON.stringify(l)), screen: (t, c) => c.seen.ask_open && !/Do you want to proceed/.test(t) },
    },
    on: { ask_open: async (c) => { await sleep(1500); c.mark('sent'); c.keys('Escape'); } },
    done: (c) => c.seen.cancelled && c.sinceSent() > 6000,
  },
  question: {
    args: '--permission-mode default',
    prompt: 'Use the AskUserQuestion tool to ask me whether I prefer red or blue, with exactly those two options. Then reply DONE.',
    marks: {
      ask_open: { hook: (h) => h.hook_event_name === 'PermissionRequest' && h.tool_name === 'AskUserQuestion', line: (l) => has(l, 'tool_use', 'AskUserQuestion'), screen: (t) => /Enter to select/.test(t) },
      answered: { base: 'sent', hook: (h) => h.hook_event_name === 'PostToolUse' && h.tool_name === 'AskUserQuestion', line: (l) => has(l, 'tool_result'), screen: (t, c) => c.seen.ask_open && !/Enter to select/.test(t) },
      turn_end: { hook: (h) => h.hook_event_name === 'Stop', line: (l) => l.type === 'assistant' && l.message?.stop_reason === 'end_turn' },
    },
    on: { ask_open: async (c) => { await sleep(1500); c.mark('sent'); c.keys('Enter'); } },
    done: (c) => c.seen.turn_end,
  },
  question_esc: {
    args: '--permission-mode default',
    prompt: 'Use the AskUserQuestion tool to ask me whether I prefer red or blue, with exactly those two options. Then reply DONE.',
    marks: {
      ask_open: { hook: (h) => h.hook_event_name === 'PermissionRequest' && h.tool_name === 'AskUserQuestion', line: (l) => has(l, 'tool_use', 'AskUserQuestion'), screen: (t) => /Enter to select/.test(t) },
      cancelled: { base: 'sent', line: (l, c) => (has(l, 'tool_result') && c.at.ask_open?.transcript !== undefined) || /declined|interrupted/i.test(JSON.stringify(l)), screen: (t, c) => c.seen.ask_open && !/Enter to select/.test(t) },
    },
    on: { ask_open: async (c) => { await sleep(1500); c.mark('sent'); c.keys('Escape'); } },
    done: (c) => c.seen.cancelled && c.sinceSent() > 6000,
  },
  plan: {
    args: '--permission-mode plan',
    prompt: 'Plan: create a file plan.txt containing the word hi. Present the plan with the ExitPlanMode tool.',
    marks: {
      ask_open: { hook: (h) => h.hook_event_name === 'PermissionRequest' && h.tool_name === 'ExitPlanMode', line: (l) => has(l, 'tool_use', 'ExitPlanMode'), screen: (t) => /Would you like to proceed|Ready to code/i.test(t) },
      cancelled: { base: 'sent', line: (l, c) => (has(l, 'tool_result') && c.at.ask_open?.transcript !== undefined) || /interrupted|rejected|declined/i.test(JSON.stringify(l)), screen: (t, c) => c.seen.ask_open && !/Would you like to proceed|Ready to code/i.test(t) },
    },
    on: { ask_open: async (c) => { await sleep(1500); c.mark('sent'); c.keys('Escape'); } },
    done: (c) => c.seen.cancelled && c.sinceSent() > 6000,
  },
  esc_turn: {
    args: BASH_OK,
    prompt: 'Run the shell command `node -e "const t=Date.now();while(Date.now()-t<40000);console.log(1)"` with the Bash tool, in the foreground, then reply DONE.',
    marks: {
      tool_start: { hook: (h) => h.hook_event_name === 'PreToolUse' && h.tool_name === 'Bash', line: (l) => has(l, 'tool_use', 'Bash'), screen: (t) => /⎿\s+\$ node -e/.test(t) },
      interrupted: { base: 'sent', line: (l) => /interrupted/i.test(JSON.stringify(l)), screen: (t, c) => c.seen.tool_start && c.sent && !working(t) },
    },
    on: { tool_start: async (c) => { await sleep(4000); c.mark('sent'); c.keys('Escape'); } },
    done: (c) => c.seen.interrupted && c.sinceSent() > 6000,
  },
  silent: {
    args: BASH_OK,
    prompt: 'Run the shell command `node -e "const t=Date.now();while(Date.now()-t<25000);console.log(1)"` with the Bash tool, in the foreground, then reply DONE.',
    marks: {
      tool_start: { hook: (h) => h.hook_event_name === 'PreToolUse' && h.tool_name === 'Bash', line: (l) => has(l, 'tool_use', 'Bash'), screen: (t) => /⎿\s+\$ node -e/.test(t) },
      tool_end: { hook: (h) => h.hook_event_name === 'PostToolUse' && h.tool_name === 'Bash', line: (l, c) => has(l, 'tool_result') && c.at.tool_start?.transcript !== undefined, screen: (t) => /Ran 1 shell command/.test(t) },
      turn_end: { hook: (h) => h.hook_event_name === 'Stop', line: (l) => l.type === 'assistant' && l.message?.stop_reason === 'end_turn' },
    },
    done: (c) => c.seen.turn_end,
    timeout: 120000,
  },
};

function has(line, kind, name) {
  const parts = Array.isArray(line.message?.content) ? line.message.content : [];
  return parts.some((p) => p.type === kind && (!name || p.name === name));
}

// Reads the bytes appended to a file since the last call and returns the complete lines.
function tailer(file) {
  let off = 0;
  let rest = '';
  return () => {
    let st;
    try { st = fs.statSync(file); } catch { return []; }
    if (st.size <= off) return [];
    const fd = fs.openSync(file, 'r');
    const buf = Buffer.alloc(st.size - off);
    fs.readSync(fd, buf, 0, buf.length, off);
    fs.closeSync(fd);
    off = st.size;
    const parts = (rest + buf.toString('utf8')).split('\n');
    rest = parts.pop();
    return parts.filter(Boolean);
  };
}

async function runCase(name, n) {
  const spec = CASES[name];
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `statuslag-${name}-`));
  const log = path.join(dir, 'hooks.log');
  fs.writeFileSync(log, '');
  const hookCmd = `sh -c 't=$(date +%s%3N); printf "%s\\t%s\\n" "$t" "$(cat | tr "\\n" " ")" >> ${log}'`;
  const settings = path.join(dir, 'settings.json');
  fs.writeFileSync(settings, JSON.stringify({
    hooks: Object.fromEntries(EVENTS.map((e) => [e, [{ hooks: [{ type: 'command', command: hookCmd, timeout: 5 }] }]])),
  }));
  tmux('kill-server');
  const cmd = `cd ${dir} && claude --model haiku --settings ${settings} ${spec.args}`;
  tmux('new-session', '-d', '-s', 'p', '-x', '120', '-y', '40', cmd);

  const ctx = {
    seen: {}, at: {}, sent: false,
    keys: (...k) => tmux('send-keys', '-t', 'p', ...k),
    mark: (m) => { ctx.sentAt = Date.now(); ctx.sent = true; ctx.at.sent = ctx.sentAt; },
    sinceSent: () => (ctx.sentAt ? Date.now() - ctx.sentAt : 0),
  };
  // startup: the trust dialog defaults to "No, exit", so move down before Enter
  for (let i = 0; i < 400; i++) {
    const t = pane();
    if (/trust this folder/.test(t)) { ctx.keys('Down', 'Enter'); await sleep(500); continue; }
    if (/❯ Try /.test(t) || /^❯ *$/m.test(t)) break;
    await sleep(100);
  }
  await sleep(1000);

  const hookTail = tailer(log);
  let trTail = null;
  const events = []; // {src, t, data}
  const screens = []; // {t, text}
  let lastScreen = '';
  const t0 = Date.now();
  const consume = (src, t, data) => {
    events.push({ src, t, data });
    for (const [m, def] of Object.entries(spec.marks)) {
      const match = def[src === 'hook' ? 'hook' : src === 'transcript' ? 'line' : 'screen'];
      if (!match) continue;
      ctx.at[m] ??= {};
      if (ctx.at[m][src] !== undefined) continue;
      let ok = false;
      try { ok = match(data, ctx); } catch { ok = false; }
      if (!ok) continue;
      // a mark timed from a sent key only counts after the key
      if (def.base === 'sent' && !ctx.sent) continue;
      ctx.at[m][src] = t;
      if (src === 'screen' || src === 'hook' || src === 'transcript') {
        if (!ctx.seen[m]) {
          ctx.seen[m] = true;
          if (spec.on?.[m]) spec.on[m](ctx).catch(() => {});
        }
      }
    }
  };

  // submit the prompt
  tmux('send-keys', '-t', 'p', '-l', spec.prompt);
  await sleep(300);
  const tSubmit = Date.now();
  ctx.keys('Enter');
  const deadline = Date.now() + (spec.timeout ?? 90000);
  let tfile = null;
  while (Date.now() < deadline) {
    const now = Date.now();
    for (const l of hookTail()) {
      const [t, json] = [l.slice(0, l.indexOf('\t')), l.slice(l.indexOf('\t') + 1)];
      try {
        const h = JSON.parse(json);
        if (!tfile && h.transcript_path) { tfile = h.transcript_path; trTail = tailer(tfile); }
        consume('hook', Number(t), h);
      } catch { /* partial payload */ }
    }
    if (trTail) {
      for (const l of trTail()) {
        try { consume('transcript', Date.now(), JSON.parse(l)); events[events.length - 1].raw = l; } catch { /* partial line */ }
      }
    }
    const text = pane();
    if (text !== lastScreen) {
      lastScreen = text;
      screens.push({ t: now, text });
      consume('screen', Date.now(), text);
    }
    if (spec.done(ctx)) {
      // keep polling until every source has reported every mark, at most 5 s, so a late hook is not lost
      ctx.doneAt ??= now;
      const complete = Object.entries(spec.marks).every(([m, d]) => (d.hook ? ctx.at[m]?.hook !== undefined : true) && (d.line ? ctx.at[m]?.transcript !== undefined : true) && (d.screen ? ctx.at[m]?.screen !== undefined : true));
      if (complete || now - ctx.doneAt > 5000) break;
    }
    await sleep(POLL_MS);
  }
  const finished = spec.done(ctx);
  const rawHooks = fs.readFileSync(log, 'utf8');
  tmux('kill-server');
  const result = { name, n, finished, tSubmit, t0, at: ctx.at, events: events.map(({ src, t, data, raw }) => ({ src, t, data: src === 'screen' ? undefined : data, raw })), rawHooks, version: spawnSync('claude', ['--version'], { encoding: 'utf8' }).stdout.trim() };
  fs.mkdirSync(path.join(OUT, name), { recursive: true });
  fs.writeFileSync(path.join(OUT, name, `${n}.json`), JSON.stringify(result, null, 1));
  fs.writeFileSync(path.join(OUT, name, `${n}.screens.json`), JSON.stringify(screens));
  if (tfile) fs.copyFileSync(tfile, path.join(OUT, name, `${n}.transcript.jsonl`));
  fs.rmSync(dir, { recursive: true, force: true });
  if (tfile) fs.rmSync(path.dirname(tfile), { recursive: true, force: true });
  return result;
}

const pct = (xs, p) => {
  const s = [...xs].sort((a, b) => a - b);
  if (!s.length) return null;
  return s[Math.min(s.length - 1, Math.ceil((p / 100) * s.length) - 1)];
};
const med = (xs) => pct(xs, 50);

function summarize(results) {
  const rows = [];
  const byCase = {};
  for (const r of results) (byCase[r.name] ??= []).push(r);
  for (const [name, rs] of Object.entries(byCase)) {
    for (const m of Object.keys(CASES[name].marks)) {
      const pairs = { 'transcript-hook': [], 'screen-hook': [], 'screen-transcript': [], 'transcript-sent': [], 'screen-sent': [] };
      let n = 0;
      for (const r of rs) {
        const a = r.at[m] ?? {};
        const sent = r.at.sent;
        if (a.hook !== undefined) {
          if (a.transcript !== undefined) pairs['transcript-hook'].push(a.transcript - a.hook);
          if (a.screen !== undefined) pairs['screen-hook'].push(a.screen - a.hook);
        }
        if (a.transcript !== undefined && a.screen !== undefined) pairs['screen-transcript'].push(a.screen - a.transcript);
        if (CASES[name].marks[m].base === 'sent' && sent) {
          if (a.transcript !== undefined) pairs['transcript-sent'].push(a.transcript - sent);
          if (a.screen !== undefined) pairs['screen-sent'].push(a.screen - sent);
        }
        n++;
      }
      for (const [pair, xs] of Object.entries(pairs)) {
        if (xs.length) rows.push({ case: name, mark: m, pair, n: xs.length, runs: n, median_ms: med(xs), p95_ms: pct(xs, 95) });
      }
      const missing = ['hook', 'transcript', 'screen'].filter((s) => CASES[name].marks[m][s === 'transcript' ? 'line' : s] && rs.every((r) => (r.at[m] ?? {})[s] === undefined));
      if (missing.length) rows.push({ case: name, mark: m, pair: `never seen by ${missing.join(',')}`, n: 0, runs: n });
    }
  }
  return rows;
}

function keepFixtures(r) {
  const td = path.join(root, 'packages/server/testdata');
  const dir = path.join(OUT, r.name);
  fs.mkdirSync(path.join(td, 'events', 'lag'), { recursive: true });
  fs.writeFileSync(path.join(td, 'events', 'lag', `claude-${r.name}.jsonl`), r.rawHooks);
  fs.mkdirSync(path.join(td, 'transcripts', 'lag'), { recursive: true });
  const tr = path.join(dir, `${r.n}.transcript.jsonl`);
  if (fs.existsSync(tr)) fs.copyFileSync(tr, path.join(td, 'transcripts', 'lag', `claude-${r.name}.jsonl`));
  const screens = JSON.parse(fs.readFileSync(path.join(dir, `${r.n}.screens.json`), 'utf8'));
  // the screen at each mark's first redraw
  fs.mkdirSync(path.join(td, 'screens', 'lag'), { recursive: true });
  for (const [m, a] of Object.entries(r.at)) {
    if (a?.screen === undefined) continue;
    const s = screens.find((x) => x.t <= a.screen) ? [...screens].reverse().find((x) => x.t <= a.screen) : screens[0];
    if (s) fs.writeFileSync(path.join(td, 'screens', 'lag', `claude-${r.name}-${m}.txt`), s.text.replace(/\s+$/gm, '').trimEnd() + '\n');
  }
}

const names = Object.keys(CASES).filter((c) => !ONLY || ONLY.includes(c));
if (DRY) {
  for (const c of names) console.log(`${c}: ${Object.keys(CASES[c].marks).join(', ')}`);
  console.log(`runs per case: ${RUNS}, out: ${OUT}, fixtures: ${FIXTURES}`);
  process.exit(0);
}
fs.mkdirSync(OUT, { recursive: true });
const results = [];
for (const c of names) {
  for (let i = 1; i <= RUNS; i++) {
    const r = await runCase(c, i);
    results.push(r);
    console.error(`${c} #${i}: ${r.finished ? 'ok' : 'TIMEOUT'} ${JSON.stringify(r.at)}`);
    if (i === 1 && FIXTURES) keepFixtures(r);
  }
}
const rows = summarize(results);
fs.writeFileSync(path.join(OUT, 'summary.json'), JSON.stringify({ version: results[0]?.version, date: new Date().toISOString(), rows }, null, 1));
console.log('| case | mark | pair | n | median ms | p95 ms |\n|---|---|---|---|---|---|');
for (const r of rows) console.log(`| ${r.case} | ${r.mark} | ${r.pair} | ${r.n}/${r.runs} | ${r.median_ms ?? ''} | ${r.p95_ms ?? ''} |`);
