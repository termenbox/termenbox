// THE LEGACY SHIM, client side (decision 0138, step 3.2; removed by step 8.1). A server whose `auth_ok` names no
// protocol version predates the rename of "session" to "workspace" on the wire: its messages are translated up to the
// current vocabulary on the way in, ours down on the way out. This file and `packages/server/src/legacy.rs` are the
// only places that know the old names; both are tested against `packages/core/testdata/legacy-vectors.json`.
// The translation is by key and message type only, so no value is touched, and the one map keyed by workspace ids
// (`agent_sync`'s `after`) is not entered.

const TYPES: [string, string][] = [
  ['list_sessions', 'list_workspaces'],
  ['new_session', 'new_workspace'],
  ['close_session', 'close_workspace'],
  ['rename_session', 'rename_workspace'],
  ['sessions', 'workspaces'],
  ['session_attached', 'workspace_attached'],
  ['session_view', 'workspace_view'],
  ['session_answer', 'workspace_answer'],
];
const KEYS: [string, string][] = [
  ['session', 'workspace'],
  ['session_id', 'workspace_id'],
  ['sessions', 'workspaces'],
  ['kill_sessions', 'kill_workspaces'],
];
const DATA_KEYED = new Set(['after']);

function renameKeys(v: unknown, fromOld: boolean): unknown {
  if (Array.isArray(v)) return v.map((x) => renameKeys(x, fromOld));
  if (v === null || typeof v !== 'object') return v;
  const out: Record<string, unknown> = {};
  for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
    const hit = KEYS.find(([o, n]) => k === (fromOld ? o : n));
    out[hit ? (fromOld ? hit[1] : hit[0]) : k] = DATA_KEYED.has(k) ? val : renameKeys(val, fromOld);
  }
  return out;
}

function translate<T>(msg: T, fromOld: boolean): T {
  if (msg === null || typeof msg !== 'object' || Array.isArray(msg)) return msg;
  const out = renameKeys(msg, fromOld) as Record<string, unknown>;
  const ty = (msg as Record<string, unknown>).type;
  const hit = TYPES.find(([o, n]) => ty === (fromOld ? o : n));
  if (hit) out.type = fromOld ? hit[1] : hit[0];
  return out as T;
}

/** A message from an old server, in the current vocabulary. */
export const fromLegacy = <T>(msg: T): T => translate(msg, true);
/** A message of ours, in the vocabulary an old server reads. */
export const toLegacy = <T>(msg: T): T => translate(msg, false);
