#!/usr/bin/env node
// Records which hooks, transcript lines and screens Claude Code produces in the cases that fact 0048 did not
// measure: subagents, background work, wakeups, compaction, resume, API errors, elicitation, denied tools,
// idle notifications and parallel dialogs (plan docs/plans/agent-status-model.md A.2).
//
//   node scripts/status-events.mjs [--cases subagent,errors] [--out DIR] [--fixtures] [--list]
//
// Every case runs a real `claude` (haiku) in a scratch tmux socket and directory with all 17 hook events
// subscribed. Time is milliseconds from the first prompt. API errors come from a local server set as
// ANTHROPIC_BASE_URL that answers 429/401/500/529; it never logs request headers.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
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
const ONLY = arg('cases', '') ? String(arg('cases', '')).split(',') : null;
const OUT = path.resolve(String(arg('out', path.join(os.tmpdir(), 'status-events'))));
const FIXTURES = arg('fixtures', false) === true;
const SOCK = 'statusevents';
const EVENTS = [
  'SessionStart', 'SessionEnd', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PostToolUseFailure',
  'PermissionRequest', 'PermissionDenied', 'Elicitation', 'ElicitationResult', 'Notification', 'Stop',
  'StopFailure', 'SubagentStart', 'SubagentStop', 'PreCompact', 'PostCompact',
];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tmux = (...a) => spawnSync('tmux', ['-L', SOCK, ...a], { encoding: 'utf8' });
const pane = () => tmux('capture-pane', '-p', '-t', 'p').stdout ?? '';
const P = '--permission-mode default';

// A step: {send} types text and Enter, {key} sends keys, {until: hookName | RegExp, ms} waits (the screen for a
// RegExp), {wait} sleeps, {relaunch} restarts claude with extra args in the same directory, {mark} names a screen.
const CASES = {
  subagent: { args: `${P} --allowedTools Agent,Task`, steps: [
    { send: 'Use the Agent tool to start one subagent whose whole job is to reply with the word PING, then reply DONE.' },
    { until: 'Stop', ms: 90000 }, { wait: 4000 },
  ] },
  bg_bash: { args: `${P} --allowedTools Bash`, steps: [
    { send: 'Run `sleep 12; echo bgdone` with the Bash tool using run_in_background true. Do not wait for it. Reply STARTED and stop.' },
    { until: 'Stop', ms: 60000 }, { mark: 'after_stop' }, { wait: 40000 }, { mark: 'after_wait' },
  ] },
  wakeup: { args: `${P} --allowedTools Bash`, steps: [
    { send: 'If you have a ScheduleWakeup tool, use it to wake yourself in 60 seconds with the prompt "WOKE", then stop. If you do not have it, use /loop semantics: reply NOTOOL.' },
    { until: 'Stop', ms: 60000 }, { wait: 150000 }, { mark: 'after_wake' },
  ] },
  compact: { args: P, steps: [
    { send: 'Reply with the single word OK.' }, { until: 'Stop', ms: 60000 }, { wait: 1500 },
    { send: '/compact' }, { until: 'PostCompact', ms: 90000 }, { wait: 5000 }, { mark: 'after_compact' },
  ] },
  resume: { args: P, steps: [
    { send: 'Reply with the single word OK.' }, { until: 'Stop', ms: 60000 }, { wait: 1500 },
    { send: '/exit' }, { until: 'SessionEnd', ms: 20000 }, { wait: 1500 },
    { relaunch: '--continue' }, { wait: 5000 }, { mark: 'after_continue' },
  ] },
  idle_prompt: { args: P, steps: [
    { send: 'Reply with the single word OK.' }, { until: 'Stop', ms: 60000 }, { wait: 90000 }, { mark: 'after_idle' },
  ] },
  denied: { args: `${P} --disallowedTools Bash`, steps: [
    { send: 'Run the shell command `echo hi` with the Bash tool, then reply DONE.' }, { until: 'Stop', ms: 60000 }, { wait: 3000 },
  ] },
  parallel: { args: P, steps: [
    { send: 'In one single message call the Bash tool twice in parallel: `touch pa.txt` and `touch pb.txt`. Then reply DONE.' },
    { until: /Do you want to proceed/, ms: 60000 }, { wait: 2500 }, { mark: 'first_dialog' },
    { key: ['1'] }, { wait: 2500 }, { mark: 'second_dialog' }, { key: ['1'] }, { until: 'Stop', ms: 60000 }, { wait: 3000 },
  ] },
  elicitation: { args: P, mcp: true, steps: [
    { send: 'Call the ask_me tool from the probe MCP server, then reply with what it returned.' },
    { until: /Do you want to proceed/, ms: 60000 }, { key: ['1'] },
    { until: /name|Elicit|provide|Submit|Accept/i, ms: 30000 }, { wait: 2000 }, { mark: 'elicitation' },
    { key: ['Enter'] }, { wait: 2500 }, { mark: 'after_enter' }, { key: ['Escape'] }, { until: 'Stop', ms: 60000 }, { wait: 3000 },
  ] },
  err_429: { args: P, api: 429, steps: [{ send: 'Reply OK.' }, { until: 'StopFailure', ms: 240000 }, { wait: 3000 }] },
  err_401: { args: P, api: 401, steps: [{ send: 'Reply OK.' }, { until: 'StopFailure', ms: 120000 }, { wait: 3000 }] },
  err_500: { args: P, api: 500, steps: [{ send: 'Reply OK.' }, { until: 'StopFailure', ms: 240000 }, { wait: 3000 }] },
  err_529: { args: P, api: 529, steps: [{ send: 'Reply OK.' }, { until: 'StopFailure', ms: 240000 }, { wait: 3000 }] },
};

const MCP = `// minimal MCP server over stdio that elicits from the client when its tool is called
const rl = require('node:readline').createInterface({ input: process.stdin });
const out = (m) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...m }) + '\\n');
const pending = new Map();
let next = 1000;
rl.on('line', (l) => {
  const m = JSON.parse(l);
  if (m.method === 'initialize') return out({ id: m.id, result: { protocolVersion: m.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'probe', version: '1' } } });
  if (m.method === 'tools/list') return out({ id: m.id, result: { tools: [{ name: 'ask_me', description: 'Asks the user for their name through an elicitation', inputSchema: { type: 'object', properties: {} } }] } });
  if (m.method === 'tools/call') {
    const id = next++;
    pending.set(id, m.id);
    return out({ id, method: 'elicitation/create', params: { message: 'What is your name?', requestedSchema: { type: 'object', properties: { name: { type: 'string', description: 'your name' } }, required: ['name'] } } });
  }
  if (m.id !== undefined && pending.has(m.id)) {
    const orig = pending.get(m.id);
    pending.delete(m.id);
    return out({ id: orig, result: { content: [{ type: 'text', text: 'elicitation action: ' + (m.result && m.result.action) }] } });
  }
});
`;

function fakeApi(status, log) {
  const body = {
    429: { type: 'error', error: { type: 'rate_limit_error', message: 'rate limited (probe)' } },
    401: { type: 'error', error: { type: 'authentication_error', message: 'invalid x-api-key (probe)' } },
    500: { type: 'error', error: { type: 'api_error', message: 'internal error (probe)' } },
    529: { type: 'error', error: { type: 'overloaded_error', message: 'overloaded (probe)' } },
  }[status];
  const srv = http.createServer((req, res) => {
    req.resume();
    log.push({ t: Date.now(), method: req.method, url: req.url });
    res.writeHead(status, { 'content-type': 'application/json', ...(status === 429 ? { 'retry-after': '1' } : {}) });
    res.end(JSON.stringify(body));
  });
  return new Promise((r) => srv.listen(0, '127.0.0.1', () => r(srv)));
}

async function runCase(name) {
  const spec = CASES[name];
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `statusevents-${name}-`));
  const hooks = path.join(dir, 'hooks.log');
  fs.writeFileSync(hooks, '');
  const hookCmd = `sh -c 't=$(date +%s%3N); printf "%s\\t%s\\n" "$t" "$(cat | tr "\\n" " ")" >> ${hooks}'`;
  const settings = path.join(dir, 'settings.json');
  fs.writeFileSync(settings, JSON.stringify({ hooks: Object.fromEntries(EVENTS.map((e) => [e, [{ hooks: [{ type: 'command', command: hookCmd, timeout: 5 }] }]])) }));
  let extra = '';
  if (spec.mcp) {
    fs.writeFileSync(path.join(dir, 'probe-mcp.cjs'), MCP);
    fs.writeFileSync(path.join(dir, 'mcp.json'), JSON.stringify({ mcpServers: { probe: { command: 'node', args: [path.join(dir, 'probe-mcp.cjs')] } } }));
    extra = `--mcp-config ${path.join(dir, 'mcp.json')}`;
  }
  const apiLog = [];
  let srv = null;
  let env = '';
  if (spec.api) {
    srv = await fakeApi(spec.api, apiLog);
    env = `ANTHROPIC_BASE_URL=http://127.0.0.1:${srv.address().port} `;
  }
  const launch = (more = '') => `cd ${dir} && ${env}claude --model haiku --settings ${settings} ${spec.args} ${extra} ${more}`;
  tmux('kill-server');
  tmux('new-session', '-d', '-s', 'p', '-x', '120', '-y', '40', launch());
  const startup = async () => {
    for (let i = 0; i < 400; i++) {
      const t = pane();
      if (/trust this folder/.test(t)) { tmux('send-keys', '-t', 'p', 'Down', 'Enter'); await sleep(500); continue; }
      if (/❯ Try /.test(t) || /^❯ *$/m.test(t)) break;
      await sleep(100);
    }
    await sleep(1000);
  };
  await startup();

  const t0 = Date.now();
  const screens = {};
  const tl = []; // {t, kind, text}
  let seenHooks = 0;
  const readHooks = () => {
    const lines = fs.readFileSync(hooks, 'utf8').split('\n').filter(Boolean);
    const fresh = lines.slice(seenHooks);
    seenHooks = lines.length;
    for (const l of fresh) {
      const i = l.indexOf('\t');
      try { tl.push({ t: Number(l.slice(0, i)) - t0, kind: 'hook', data: JSON.parse(l.slice(i + 1)) }); } catch { /* partial */ }
    }
  };
  const hasHook = (n) => tl.some((e) => e.kind === 'hook' && e.data.hook_event_name === n && e.t >= (hasHook.from ?? -1e9));
  for (const s of spec.steps) {
    if (s.send) {
      tmux('send-keys', '-t', 'p', '-l', s.send); await sleep(300); tmux('send-keys', '-t', 'p', 'Enter');
      hasHook.from = Date.now() - t0;
      tl.push({ t: Date.now() - t0, kind: 'sent', text: s.send });
    } else if (s.key) {
      tmux('send-keys', '-t', 'p', ...s.key); tl.push({ t: Date.now() - t0, kind: 'key', text: s.key.join(' ') });
    } else if (s.wait) {
      const end = Date.now() + s.wait;
      while (Date.now() < end) { readHooks(); await sleep(100); }
    } else if (s.until) {
      const end = Date.now() + s.ms;
      while (Date.now() < end) {
        readHooks();
        if (typeof s.until === 'string' ? hasHook(s.until) : s.until.test(pane())) break;
        await sleep(50);
      }
      readHooks();
      tl.push({ t: Date.now() - t0, kind: 'waited', text: String(s.until), ok: typeof s.until === 'string' ? hasHook(s.until) : s.until.test(pane()) });
    } else if (s.mark) {
      screens[s.mark] = pane();
    } else if (s.relaunch) {
      tmux('kill-server');
      tmux('new-session', '-d', '-s', 'p', '-x', '120', '-y', '40', launch(s.relaunch));
      await startup();
      tl.push({ t: Date.now() - t0, kind: 'relaunch', text: s.relaunch });
    }
  }
  readHooks();
  screens.final = pane();
  tmux('kill-server');
  if (srv) srv.close();
  const tr = tl.find((e) => e.data?.transcript_path)?.data.transcript_path;
  const res = { name, version: spawnSync('claude', ['--version'], { encoding: 'utf8' }).stdout.trim(), timeline: tl, screens, api: apiLog.map((a) => ({ t: a.t - t0, method: a.method, url: a.url })) };
  fs.mkdirSync(path.join(OUT, name), { recursive: true });
  fs.writeFileSync(path.join(OUT, name, 'result.json'), JSON.stringify(res, null, 1));
  if (tr && fs.existsSync(tr)) fs.copyFileSync(tr, path.join(OUT, name, 'transcript.jsonl'));
  fs.copyFileSync(hooks, path.join(OUT, name, 'hooks.log'));
  if (tr) fs.rmSync(path.dirname(tr), { recursive: true, force: true });
  fs.rmSync(dir, { recursive: true, force: true });
  return res;
}

function show(r) {
  console.log(`\n## ${r.name} (${r.version})`);
  for (const e of r.timeline) {
    if (e.kind === 'hook') {
      const d = e.data;
      const keep = Object.entries(d).filter(([k]) => !['hook_event_name', 'session_id', 'transcript_path', 'cwd', 'permission_mode', 'scratchpad_dir', 'prompt_id', 'effort'].includes(k)).map(([k, v]) => `${k}=${JSON.stringify(v).slice(0, 60)}`);
      console.log(`${String(e.t).padStart(7)} ${d.hook_event_name} ${keep.join(' ')}`);
    } else console.log(`${String(e.t).padStart(7)} ${e.kind} ${e.text ?? ''}${e.ok === false ? ' (TIMED OUT)' : ''}`);
  }
  if (r.api.length) console.log(`api requests: ${r.api.length} at ${r.api.slice(0, 12).map((a) => a.t).join(', ')} ms`);
}

if (arg('list', false) === true) { for (const c of Object.keys(CASES)) console.log(c); process.exit(0); }
fs.mkdirSync(OUT, { recursive: true });
for (const c of Object.keys(CASES).filter((c) => !ONLY || ONLY.includes(c))) {
  const r = await runCase(c);
  show(r);
  if (FIXTURES) {
    const td = path.join(root, 'packages/server/testdata/events/a2');
    fs.mkdirSync(td, { recursive: true });
    fs.copyFileSync(path.join(OUT, c, 'hooks.log'), path.join(td, `claude-${c}.jsonl`));
  }
}
