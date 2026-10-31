// The logic of the docs-media review (DECISION-0158), kept free of GitHub so it runs under node --test. The workflows pass
// the comment and the issue body in through the environment and call the entry points here; nothing is ever evaluated.
export const PACES = ['normal', 'slow'];
export const PROFILES = ['phone', 'desktop'];
const NOTE_MAX = 300;

/**
 * Parses `/regen <case> [pace=slow|normal] [profile=phone|desktop] [caption=off] <note>` or `/approve <case|all>`.
 * Returns { cmd, ... } or { error }. The note is data: it is stored and posted back, never run.
 */
export function parseCommand(text, catalogIds) {
  const lines = String(text ?? '').replace(/\r/g, '').split('\n').map((l) => l.trim()).filter(Boolean);
  if (lines.length !== 1) return { error: 'one command on one line' };
  const [verb, ...rest] = lines[0].split(/\s+/);
  if (verb === '/approve') {
    if (rest.length !== 1) return { error: 'usage: /approve <case|all>' };
    if (rest[0] !== 'all' && !catalogIds.includes(rest[0])) return { error: `unknown case ${JSON.stringify(rest[0].slice(0, 40))}` };
    return { cmd: 'approve', case: rest[0] };
  }
  if (verb !== '/regen') return { error: 'not a media command' };
  const id = rest.shift();
  if (!id || !catalogIds.includes(id)) return { error: `unknown case ${JSON.stringify((id ?? '').slice(0, 40))}` };
  const out = { cmd: 'regen', case: id, pace: 'normal', profile: 'both', caption: 'on', note: '' };
  while (rest.length && /^[a-z]+=/.test(rest[0])) {
    const [key, value] = rest.shift().split('=');
    if (key === 'pace' && PACES.includes(value)) out.pace = value;
    else if (key === 'profile' && PROFILES.includes(value)) out.profile = value;
    else if (key === 'caption' && value === 'off') out.caption = 'off';
    else return { error: `unknown knob ${JSON.stringify(`${key}=${value}`.slice(0, 40))}` };
  }
  out.note = rest.join(' ').slice(0, NOTE_MAX);
  return out;
}

const LINE = /^- \[( |x|X)\] ([a-z0-9-]+) (phone|desktop) ([0-9a-f]{8,64})\s*$/;

/** The checklist lines of an issue body as { case, profile, hash, ticked }; other lines are ignored. */
export function parseChecklist(body) {
  return String(body ?? '').split('\n').flatMap((l) => { const m = LINE.exec(l.replace(/\r$/, '')); return m ? [{ case: m[2], profile: m[3], hash: m[4], ticked: m[1] !== ' ' }] : []; });
}

/** Lines newly ticked and newly unticked between two issue bodies, keyed by case, profile and hash. */
export function diffChecklist(before, after) {
  const key = (l) => `${l.case} ${l.profile} ${l.hash}`;
  const was = new Map(parseChecklist(before).map((l) => [key(l), l.ticked]));
  const ticked = [], unticked = [];
  for (const l of parseChecklist(after)) {
    if (!was.has(key(l))) continue; // a line that was not in the ledger is not an approval
    if (l.ticked && !was.get(key(l))) ticked.push(l);
    if (!l.ticked && was.get(key(l))) unticked.push(l);
  }
  return { ticked, unticked };
}

/** Only a person with write access to the repository may render, approve or regenerate (guardrail on media commands). */
export const mayAct = (permission) => ['write', 'maintain', 'admin'].includes(permission);
