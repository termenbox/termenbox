// A fake ACP agent for the browser e2e (U07b): JSON-RPC 2.0 over stdio, built from the recorded captures in
// packages/server/testdata/acp/. It starts exactly as the server starts a real agent (a command from the owner's
// allowlist), answers `initialize` and `session/new` with what claude really returned, and plays the recorded
// `session/update` and `session/request_permission` messages for a prompt keyword. It owns no model: what a turn does is
// decided by the prompt's first word.
//   edit      claude's Write with a diff and a permission request (recorded); allowed: the write completes; rejected: the
//             recorded failure and reply
//   shell     opencode's `ls /etc` permission request (recorded)
//   plan      kilo's todowrite calls (recorded): the plan becomes the agent's todo list
//   markdown  a reply with a heading, a list, code and a link, in chunks like the recorded ones
//   stream    tokens every 60 ms until `session/cancel`, then `stopReason: cancelled` (as every measured agent does)
//   burst     5000 tiny chunks as fast as they can be written
//   crash     the process dies
//   anything else is answered with one line.
// FAKE_ACP_LOG (optional) is a file: every message received is appended as a JSON line, for the test to read.
import { readFileSync, appendFileSync } from 'node:fs';
import { createInterface } from 'node:readline';

const SID = 'fake-session-1';
const fixture = (rel) => readFileSync(new URL(`../../server/testdata/acp/${rel}`, import.meta.url), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
const fix = (v) => JSON.parse(JSON.stringify(v).replaceAll('<uuid>', SID));
const claude = fixture('claude/permission-plan-mode.jsonl');
const opencode = fixture('opencode/permission-reject.jsonl');
const kilo = fixture('kilo/todowrite.jsonl');
const ins = (recs) => recs.filter((r) => r.dir === 'in').map((r) => fix(r.msg));
const updatesOf = (recs) => ins(recs).filter((m) => m.method === 'session/update').map((m) => m.params.update);

const log = process.env.FAKE_ACP_LOG;
const out = (m) => process.stdout.write(`${JSON.stringify(m)}\n`);
const update = (u) => out({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: SID, update: u } });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const chunk = (text, messageId = 'msg_fake_1') => update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text }, messageId });

// ---- what claude really answered
const initResult = ins(claude).find((m) => m.id === 1).result;
const newResult = { ...ins(claude).find((m) => m.id === 2 && m.result).result, sessionId: SID };
const commandsUpdate = updatesOf(claude).find((u) => u.sessionUpdate === 'available_commands_update');

// ---- waiting for the client's answer to a request of ours
let nextRpc = 9000;
const waiting = new Map();
const request = (method, params) => new Promise((resolve) => { const id = nextRpc++; waiting.set(id, resolve); out({ jsonrpc: '2.0', id, method, params }); });

const ownCall = (p) => ({ ...p, toolCall: own(p.toolCall) });
let cancelled = null; // resolves the running turn when `session/cancel` arrives
const cancelPromise = () => new Promise((r) => { cancelled = r; });

// A real agent never reuses a tool call id: each turn gets its own, so the recorded ones are suffixed.
let turns = 0;
const own = (u) => (u.toolCallId ? { ...u, toolCallId: `${u.toolCallId}-t${turns}` } : u);
async function play(updates, gap = 25) {
  for (const u of updates) { update(own(u)); await sleep(gap); }
}

async function edit() {
  const recs = ins(claude);
  const recorded = recs.filter((m) => m.method === 'session/update').map((m) => m.params.update);
  const start = recorded.findIndex((u) => u.sessionUpdate === 'tool_call' && u.name === 'Write');
  const toolId = `${recorded[start].toolCallId}-t${turns}`;
  // usage, the Write call and its two updates (title, then the diff), up to the permission request
  await play(recorded.slice(Math.max(0, start - 1), start + 3).filter((u) => u.sessionUpdate !== 'available_commands_update'));
  const asked = recs.find((m) => m.method === 'session/request_permission');
  const reply = await request('session/request_permission', ownCall(asked.params));
  const chosen = reply?.outcome?.optionId ?? '';
  if (String(chosen).startsWith('allow')) {
    await play([
      { sessionUpdate: 'tool_call_update', toolCallId: toolId.replace(/-t\d+$/, ''), status: 'completed' },
      { sessionUpdate: 'usage_update', used: 19000, size: 1000000 },
    ]);
    for (const t of ['Created ', '`a.txt`', ' with the text:\n\n```\nhi\n```\n']) { chunk(t); await sleep(20); }
    return { stopReason: 'end_turn' };
  }
  // the recorded refusal: the tool failed, the agent said so
  const after = recorded.slice(recorded.findIndex((u) => u.sessionUpdate === 'tool_call_update' && u.status === 'failed'), recorded.findIndex((u) => u.sessionUpdate === 'agent_message_chunk') + 3);
  await play(after.filter((u) => u.sessionUpdate !== 'available_commands_update'));
  return { stopReason: 'end_turn' };
}

async function shell() {
  const recs = ins(opencode);
  const ups = recs.filter((m) => m.method === 'session/update').map((m) => m.params.update);
  const call = ups.findIndex((u) => u.sessionUpdate === 'tool_call');
  await play(ups.slice(call, call + 2));
  const asked = recs.find((m) => m.method === 'session/request_permission');
  const reply = await request('session/request_permission', ownCall(asked.params));
  const ok = String(reply?.outcome?.optionId ?? '') === 'once';
  await play([{ sessionUpdate: 'tool_call_update', toolCallId: ups[call].toolCallId, status: ok ? 'completed' : 'failed' }]);
  chunk(ok ? 'Listed /etc.' : 'Understood, I will not run it.');
  return { stopReason: ok ? 'end_turn' : 'cancelled' };
}

async function plan() {
  const ups = updatesOf(kilo).filter((u) => u.sessionUpdate === 'tool_call' || u.sessionUpdate === 'tool_call_update');
  await play(ups.slice(0, 5), 40);
  chunk('The todo list is written.');
  return { stopReason: 'end_turn' };
}

async function markdown() {
  const text = [
    '# Findings\n\n', 'The parser has **two** bugs. See `lex.rs` and the ', '[spec](https://example.com/spec).\n\n',
    '- first: tokens are dropped\n- second: lines are miscounted\n  - nested note\n\n', '```rust\nfn lex(src: &str) -> Vec<Token> {\n    todo!()\n}\n```\n\n',
    '> Fixing the lexer fixes both.\n',
  ];
  for (const t of text) { chunk(t, 'msg_fake_md'); await sleep(30); }
  return { stopReason: 'end_turn' };
}

async function stream() {
  const cancel = cancelPromise();
  let n = 0;
  const done = new Promise((resolve) => {
    const t = setInterval(() => { chunk(`token-${n++} `, 'msg_fake_stream'); if (n > 6000) { clearInterval(t); resolve('end_turn'); } }, 60);
    cancel.then(() => { clearInterval(t); resolve('cancelled'); });
  });
  return { stopReason: await done };
}

async function burst() {
  for (let i = 0; i < 5000; i++) chunk(`w${i} `, 'msg_fake_burst');
  return { stopReason: 'end_turn' };
}

const TURNS = { edit, shell, plan, markdown, stream, burst };

async function onPrompt(msg) {
  turns++;
  const text = String(msg.params.prompt?.[0]?.text ?? '');
  const word = text.trim().split(/\s+/)[0].toLowerCase();
  if (word === 'crash') process.exit(3);
  const turn = TURNS[word];
  const result = turn ? await turn() : (chunk(`You said: ${text.slice(0, 200)}`), { stopReason: 'end_turn' });
  out({ jsonrpc: '2.0', id: msg.id, result });
}

createInterface({ input: process.stdin }).on('line', (line) => {
  let m;
  try { m = JSON.parse(line); } catch { return; }
  if (log) appendFileSync(log, `${JSON.stringify(m)}\n`);
  if (m.id !== undefined && !m.method) { waiting.get(m.id)?.(m.result); waiting.delete(m.id); return; }
  const reply = (result) => out({ jsonrpc: '2.0', id: m.id, result });
  switch (m.method) {
    case 'initialize': return reply(initResult);
    case 'session/new':
      reply(newResult);
      return void setTimeout(() => update(commandsUpdate), 20);
    case 'session/resume': case 'session/load': return reply({});
    case 'session/set_mode':
      reply({});
      return update({ sessionUpdate: 'current_mode_update', currentModeId: m.params.modeId });
    case 'session/cancel': cancelled?.(); cancelled = null; return;
    case 'session/prompt': return void onPrompt(m);
    default: if (m.id !== undefined) out({ jsonrpc: '2.0', id: m.id, error: { code: -32601, message: 'Method not found' } });
  }
});
