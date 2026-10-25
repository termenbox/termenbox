#!/usr/bin/env node
// Probe an ACP agent over stdio and record the full wire traffic.
//
//   node scripts/acp-probe.mjs --agent opencode --cwd /tmp/work \
//     --steps 'init|auth:openai|new|prompt:create a.txt then plan|list' \
//     --perm reject --out packages/server/testdata/acp/opencode
//
// Steps (comma separated, run in order; a ':' argument may not contain ','):
//   init                      initialize (we advertise fs and terminal only with --fs / --terminal)
//   auth:<methodId>           authenticate
//   new | load | resume | close | list | fork   workspace/* against the current workspace
//   prompt:<text>             session/prompt, waits for the response
//   promptbg:<text>           session/prompt without waiting (pair with cancel / wait)
//   cancel                    session/cancel notification
//   cancel-open               reply outcome=cancelled to permission requests left open by --perm none
//   wait:<ms>                 sleep, keeping the log open
//   mode:<id>                 session/set_mode
//   config:<id>=<value>       session/set_config_option
//   model:<id>                session/set_model (unstable)
//   raw:<method>=<json>       any request
//   restart                   kill the agent and spawn a fresh one (re-init needed)
//   kill                      SIGKILL the agent and record what the client sees
// --session <id>                   start with this workspace id (for load/resume in a fresh process)
//   use:<id> switches the current workspace id; uselatest picks workspaces[0] of the last list
// --perm reject|allow|always|none  how to answer session/request_permission (none = never answer)
// --perm-extra <json>              extra top-level fields in the permission reply (e.g. qwen '{"answers":{...}}')
// --perm-pick <optionId>           choose this option id when offered
// --perm-delay <ms>                delay before answering
// --caps <json>                    extra clientCapabilities, e.g. '{"elicitation":{"form":{}}}'
// --ask <json>                     reply payload for elicitation/* and other unknown agent requests
// Output: <out>/wire.jsonl (every message, direction-tagged), <out>/samples.json (first
// full JSON per distinct update kind and per method), <out>/summary.txt.
// Secrets: env values whose name matches KEY|TOKEN|SECRET|PASSWORD, and emails, are redacted.

import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, appendFileSync } from 'node:fs';
import { readFileSync, existsSync } from 'node:fs';
import { resolve, join } from 'node:path';

const NM = process.env.ACP_NPM || '/home/ubuntu/data/acp-scratch/npm/node_modules/.bin';
const NM2 = process.env.ACP_NPM2 || '/home/ubuntu/data/acp-scratch/npm2/node_modules/.bin';
// Command per agent; adapters live in $ACP_NPM.
const AGENTS = {
  opencode: ['opencode', 'acp'],
  kilo: ['kilo', 'acp'],
  gemini: [`${NM}/gemini`, '--acp'],
  qwen: [`${NM}/qwen`, '--acp'],
  copilot: [`${NM}/copilot`, '--acp'],
  'cursor-agent': ['cursor-agent', 'acp'],
  goose: ['goose', 'acp'],
  // @zed-industries/{claude-code,codex}-acp are deprecated; the maintained packages are
  // @agentclientprotocol/claude-agent-acp and @agentclientprotocol/codex-acp.
  claude: [`${NM}/claude-agent-acp`],
  codex: [`${NM2}/codex-acp`],
};

const args = process.argv.slice(2);
const opt = (n, d) => { const i = args.indexOf(`--${n}`); return i < 0 ? d : args[i + 1]; };
const flag = (n) => args.includes(`--${n}`);
const agent = opt('agent');
const cwd = resolve(opt('cwd', process.cwd()));
const out = resolve(opt('out', `acp-out/${agent}`));
const perm = opt('perm', 'reject');
const permDelay = Number(opt('perm-delay', 0));
const steps = (opt('steps', 'init|new') || '').split('|').filter(Boolean);
const cmd = opt('cmd') ? opt('cmd').split(' ') : AGENTS[agent];
if (!cmd) { console.error(`unknown agent ${agent}; known: ${Object.keys(AGENTS).join(', ')}`); process.exit(2); }
mkdirSync(out, { recursive: true });

const secrets = Object.entries(process.env)
  .filter(([k, v]) => /KEY|TOKEN|SECRET|PASSWORD/i.test(k) && v && v.length > 6).map(([, v]) => v);
const redact = (s) => {
  for (const v of secrets) s = s.split(v).join('<redacted>');
  return s.replace(/[\w.+-]+@[\w-]+\.[\w.-]+/g, '<email>')
    .replace(/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/g, '<uuid>');
};

const wire = join(out, 'wire.jsonl');
writeFileSync(wire, '');
const samples = { updates: {}, methods: {} };
const kinds = {}; // update kind -> count
const t0 = Date.now();
const log = (dir, msg) => appendFileSync(wire, redact(JSON.stringify({ t: Date.now() - t0, dir, msg })) + '\n');

const unanswered = []; // permission requests left open by --perm none
let child, buf = '', nextId = 1;
const pending = new Map();
let sessionId = opt('workspace', null), caps = null;
const exits = [];

function start() {
  buf = '';
  const env = { ...process.env }; delete env.CLAUDECODE; // adapters refuse to start inside a Claude Code workspace
  child = spawn(cmd[0], cmd.slice(1), { cwd, env, stdio: ['pipe', 'pipe', 'pipe'] });
  child.stdout.on('data', (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) { const l = buf.slice(0, i); buf = buf.slice(i + 1); if (l.trim()) onLine(l); }
  });
  child.stderr.on('data', (d) => log('stderr', String(d).slice(0, 2000)));
  child.on('exit', (code, sig) => {
    exits.push({ code, sig, t: Date.now() - t0 });
    log('exit', { code, sig });
    for (const [, p] of pending) p.resolve({ error: { code: 'agent-exit', message: `exit ${code} ${sig}` } });
    pending.clear();
  });
}

function send(msg) { log('out', msg); child.stdin.write(JSON.stringify(msg) + '\n'); }
function request(method, params, timeout = 120000) {
  const id = nextId++;
  samples.methods[method] ??= { request: params };
  return new Promise((res) => {
    const timer = setTimeout(() => { pending.delete(id); res({ error: { code: 'timeout', message: `${method} ${timeout}ms` } }); }, timeout);
    pending.set(id, { method, resolve: (r) => { clearTimeout(timer); res(r); } });
    send({ jsonrpc: '2.0', id, method, params });
  });
}

function onLine(l) {
  let m; try { m = JSON.parse(l); } catch { log('in-nonjson', l.slice(0, 500)); return; }
  log('in', m);
  if (m.id !== undefined && (m.result !== undefined || m.error) && !m.method) {
    const p = pending.get(m.id);
    if (p) { pending.delete(m.id); if (m.result) samples.methods[p.method].response ??= m.result; p.resolve(m); }
    return;
  }
  if (m.method === 'session/update') {
    const u = m.params.update; const k = u.sessionUpdate;
    kinds[k] = (kinds[k] || 0) + 1;
    (samples.updates[k] ??= []).length < 2 && samples.updates[k].push(u);
    return;
  }
  if (m.id !== undefined && m.method) return void onAgentRequest(m);
}

async function onAgentRequest(m) {
  samples.methods[`agent->${m.method}`] ??= { request: m.params };
  const reply = (result) => send({ jsonrpc: '2.0', id: m.id, result });
  const fail = (code, message) => send({ jsonrpc: '2.0', id: m.id, error: { code, message } });
  if (m.method === 'session/request_permission') {
    if (perm === 'none') { unanswered.push(m.id); return; }
    if (permDelay) await new Promise((r) => setTimeout(r, permDelay));
    const opts = m.params.options || [];
    if (opt('perm-pick')) { const o = opts.find((x) => x.optionId === opt('perm-pick')); if (o) return reply({ outcome: { outcome: 'selected', optionId: o.optionId }, ...JSON.parse(opt('perm-extra', '{}')) }); }
    const want = perm === 'allow' ? ['allow_once', 'allow_always'] : perm === 'always' ? ['allow_always'] : ['reject_once', 'reject_always'];
    const o = opts.find((x) => want.includes(x.kind)) || opts[0];
    return reply({ outcome: { outcome: 'selected', optionId: o.optionId }, ...JSON.parse(opt('perm-extra', '{}')) });
  }
  if (m.method === 'fs/read_text_file') {
    try { return reply({ content: readFileSync(m.params.path, 'utf8') }); } catch (e) { return fail(-32002, String(e.message)); }
  }
  if (m.method === 'fs/write_text_file') { writeFileSync(m.params.path, m.params.content); return reply(null); }
  if (m.method.startsWith('terminal/')) return fail(-32601, 'terminal not implemented by probe');
  if (opt('ask') === 'none') return;
  if (opt('ask')) return reply(JSON.parse(opt('ask')));
  fail(-32601, `probe does not handle ${m.method}`);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
let lastList = [];
async function step(s) {
  const i = s.indexOf(':'); const name = i < 0 ? s : s.slice(0, i); const a = i < 0 ? '' : s.slice(i + 1);
  let r;
  switch (name) {
    case 'init':
      r = await request('initialize', {
        protocolVersion: 1,
        clientCapabilities: { fs: { readTextFile: flag('fs'), writeTextFile: flag('fs') }, terminal: flag('terminal'), ...JSON.parse(opt('caps', '{}')) },
        clientInfo: { name: 'termenbox-probe', version: '0' },
      });
      caps = r.result; break;
    case 'use': sessionId = a; r = { result: 'using ' + a }; break;
    case 'auth': r = await request('authenticate', { methodId: a }); break;
    case 'new': r = await request('session/new', { cwd, mcpServers: [] }); if (r.result) sessionId = r.result.sessionId; break;
    case 'load': r = await request('session/load', { sessionId, cwd, mcpServers: [] }); break;
    case 'resume': r = await request('session/resume', { sessionId, cwd, mcpServers: [] }); break;
    case 'fork': r = await request('session/fork', { sessionId, cwd, mcpServers: [] }); break;
    case 'close': r = await request('session/close', { sessionId }); break;
    case 'list': r = await request('session/list', { cwd }); lastList = r.result?.workspaces || []; break;
    case 'uselatest': sessionId = lastList[0]?.sessionId; r = { result: 'using ' + sessionId }; break;
    case 'prompt': r = await request('session/prompt', { sessionId, prompt: [{ type: 'text', text: a }] }, Number(opt('timeout', 180000))); break;
    case 'promptbg': request('session/prompt', { sessionId, prompt: [{ type: 'text', text: a }] }, 600000).then((x) => { log('bg-result', x); results.push({ step: 'promptbg-done', ok: !x.error, error: x.error, result: x.result }); }); r = { result: 'started' }; break;
    case 'cancel': send({ jsonrpc: '2.0', method: 'session/cancel', params: { sessionId } }); r = { result: 'sent' }; break;
    case 'cancel-open': // answer open permission requests with outcome cancelled, as the spec asks after session/cancel
      while (unanswered.length) send({ jsonrpc: '2.0', id: unanswered.shift(), result: { outcome: { outcome: 'cancelled' } } });
      r = { result: 'answered cancelled' }; break;
    case 'wait': await sleep(Number(a)); r = { result: 'waited' }; break;
    case 'mode': r = await request('session/set_mode', { sessionId, modeId: a }); break;
    case 'config': { const [id, v] = a.split('='); r = await request('session/set_config_option', { sessionId, configId: id, value: v }); break; }
    case 'model': r = await request('session/set_model', { sessionId, modelId: a }); break;
    case 'raw': { const j = a.indexOf('='); r = await request(a.slice(0, j), JSON.parse(a.slice(j + 1).replace('$SID', sessionId))); break; }
    case 'kill': child.kill('SIGKILL'); await sleep(1500); r = { result: 'killed' }; break;
    case 'restart': child.kill(); await sleep(1000); start(); r = { result: 'restarted' }; break;
    default: r = { error: { message: `unknown step ${s}` } };
  }
  results.push({ step: s, ok: !r.error, error: r.error, result: r.result });
}

start();
for (const s of steps) await step(s);
await sleep(500);
child.kill();
writeFileSync(join(out, 'samples.json'), redact(JSON.stringify({ caps, sessionId: sessionId && '<sid>', ...samples }, null, 2)));
const sum = [
  `agent ${agent} cmd ${cmd.join(' ')}`,
  `update kinds: ${JSON.stringify(kinds)}`,
  `exits: ${JSON.stringify(exits)}`,
  ...results.map((r) => `${r.step}: ${r.ok ? 'ok' : 'ERR ' + JSON.stringify(r.error)} ${r.ok ? JSON.stringify(r.result)?.slice(0, 300) : ''}`),
].join('\n');
writeFileSync(join(out, 'summary.txt'), redact(sum));
console.log(redact(sum));
process.exit(0);
