export type ClientMessage =
  | { type: 'hello' }
  | { type: 'auth'; cert: string; signature: string; server_token: string; channels?: boolean }
  /** Decision 0117: sent instead of `auth`; the token authorises a read-only preview and nothing is bound. */
  | { type: 'claim_preview'; server_token: string }
  | { type: 'revoke_device'; device: string }
  | { type: 'uninstall'; confirm_name: string; delete_user?: boolean; remove_tmux?: boolean; kill_sessions?: boolean }
  | { type: 'agent_sync'; after: Record<string, number> }
  | { type: 'peek'; session: string; hash?: string }
  | { type: 'answer'; session: string; prompt_id: string; seq: number; choice: number }
  | { type: 'reply'; session: string; text: string }
  | { type: 'windows'; session: string }
  | { type: 'select_window'; session: string; index: number }
  | { type: 'new_window'; session: string }
  | { type: 'close_window'; session: string; index: number }
  | { type: 'close_session'; session: string }
  /** Sets the name shown for a session; an empty name clears it. The session's id does not change. */
  | { type: 'rename_session'; session: string; name: string }
  | { type: 'list_sessions' }
  /** U14: the server's in-memory health samples; the answer is `health_history`. Older servers do not answer. */
  | { type: 'health_history' }
  | { type: 'new_session'; name: string; backend?: string; dir?: string; template?: string }
  | { type: 'ping' }
  | { type: 'attach'; session_id: string }
  | { type: 'detach' }
  | { type: 'input'; data: string }
  | { type: 'resize'; cols: number; rows: number }
  | { type: 'push_info' }
  | { type: 'push_subscribe'; endpoint: string; p256dh: string; auth: string }
  | { type: 'push_unsubscribe'; endpoint: string }
  /** The session this device shows in the foreground now, or null (hidden, or on the list): no push for it. */
  | { type: 'foreground'; session: string | null }
  | { type: 'ack'; attention_id: string }
  | { type: 'dismiss'; attention_id: string }
  /** Decision 0103: asks for the session view (cleaned tail, parsed prompt, tier); `hash` is the one already held. */
  | { type: 'session_view'; session: string; hash?: string }
  /** Taps option `option` (0-based) of prompt `prompt_id`; the server sends the keys its profile lists for it, never more. */
  | { type: 'session_answer'; session: string; prompt_id: string; option: number }
  /** U10: the hook state of one agent on this server. `preview` answers with the exact settings file `apply` will write. */
  | { type: 'agent_hooks'; agent: string; action: 'status' | 'preview' | 'apply' | 'remove' }
  /** T0 (decision 0111): shows this ACP session on this connection; `after` and `epoch` are the revision and stream the client holds. */
  | { type: 'acp_open'; session: string; after?: number; epoch?: number }
  | { type: 'acp_close' }
  /** The owner's next prompt (`session/prompt`); several lines allowed. */
  | { type: 'acp_prompt'; session: string; text: string }
  | { type: 'acp_interrupt'; session: string }
  /** Answers a permission with exactly the option id the agent offered. */
  | { type: 'acp_answer'; session: string; prompt_id: string; option_id: string }
  /** Switches to a mode the agent listed. */
  | { type: 'acp_mode'; session: string; mode: string }
  /** U12: the tasks that ended in [from, to) (ms since the epoch) and those still running. Older servers do not answer. */
  | { type: 'history'; from: number; to: number }
  /** U11: ask for the server's push escalation settings. */
  | { type: 'push_settings_get' }
  /** U11: replace the push escalation settings. */
  | { type: 'push_settings_set'; settings: PushSettings }
  /** Forgets the server's whole history. */
  | { type: 'history_delete' };

/** U11: push escalation settings (seconds, not minutes). */
export interface PushSettings {
  initial_wait_secs: number;
  max_interval_secs: number;
  multiplier: number;
  quiet_hours?: string;
  /** Minutes the owner's local time is ahead of UTC (the browser's), so quiet hours are local. */
  utc_offset_minutes: number;
  muted_agents: string[];
}

export type ServerMessage =
  | { type: 'challenge'; nonce: string; server_id: string }
  | { type: 'device_revoked'; device: string }
  /** Answer to `claim_preview`: `mode` is `container` or `host`; `run_user` is empty in a container. */
  | { type: 'claim_preview'; name: string; mode: string; run_user?: string; version: string }
  | { type: 'uninstall_done'; mode: string }
  /** Decision 0123: the server's signed candidate bundle, pushed after `auth_ok`; verify it before use (lib/bundle.ts). */
  | { type: 'candidates'; bundle: string }
  | { type: 'auth_ok'; channels?: string[]; user: string; server_name: string; mode?: string; version?: string; backends?: string[]; control_mode?: boolean; tmux_version?: string; prefix?: string; acp_agents?: string[] }
  | { type: 'sessions'; sessions: SessionInfo[] }
  | { type: 'session_attached'; session_id: string }
  | { type: 'output'; data: string }
  /** Never on the wire as JSON: terminal bytes from the `term` lane, as `Connection` hands them to listeners. */
  | { type: 'term'; bytes: Uint8Array }
  | { type: 'snapshot'; session: string; text: string; hash: string; unchanged?: boolean }
  | { type: 'window_list'; session: string; windows: WindowInfo[]; representative?: number | null }
  | { type: 'agent_snapshot'; states: SessionAgentState[] }
  | { type: 'agent_events'; events: AgentEvent[] }
  | { type: 'activity'; sessions: SessionActivity[] }
  | { type: 'attention'; items: Attention[] }
  | SessionViewMessage
  | AcpViewMessage
  | { type: 'push_info'; configured: boolean; public_key: string; devices: number; subscribed: boolean; settings?: boolean }
  | { type: 'push_settings'; settings: PushSettings }
  | { type: 'push_settings_rejected'; reason: string }
  | { type: 'agent_event'; event: AgentEventType; agent: string; session_id: string }
  | { type: 'agent_hooks'; agent: string; action: string; state: 'connected' | 'not_connected' | 'unsupported' | 'unavailable' | string; preview?: string; error?: string }
  | ({ type: 'server_health' } & ServerHealth)
  | HealthHistoryMessage
  | HistoryMessage
  | { type: 'history_deleted'; removed: number }
  | { type: 'error'; message: string }
  | { type: 'pong' };

/** One health sample in whole percents. A metric the server could not read is absent, never 0 (the first CPU sample is always absent). */
export interface HealthPoint { ts: number; cpu_percent?: number; ram_percent?: number; disk_percent?: number }

/** U14: pushed after sign-in and every 10 s. `healthy` is the server's own verdict (hysteresis included): clients show it and never recompute it. */
export interface ServerHealth extends HealthPoint {
  healthy: boolean;
  /** The metrics over their level: `cpu`, `ram`, `disk`. */
  unhealthy: string[];
  /** `container`: RAM is against the container's limit; `host`: against the machine. */
  scope: string;
}

/** Answers `health_history`: oldest first, at most 360 points, each the highest of the `step_s` seconds it stands for. */
export interface HealthHistoryMessage {
  type: 'health_history';
  interval_s: number;
  step_s: number;
  healthy: boolean;
  unhealthy: string[];
  scope: string;
  /** Samples in a row across a level before the verdict flips either way. */
  confirm: number;
  levels: { metric: string; enter: number; exit: number }[];
  samples: HealthPoint[];
}

/** One task an agent worked on: metadata only. `task` is scrubbed text, shown as text; no `ended_at` while it still runs. */
export interface HistoryRun {
  session: string;
  agent?: string;
  task?: string;
  started_at: number;
  ended_at?: number;
  /** `stopped` is an end nobody reported: interrupted, or the session went away. Other values are read as `stopped`. */
  outcome?: 'finished' | 'failed' | 'stopped' | string;
  /** How long the agent waited on the user during the task. */
  blocked_ms: number;
  /** The name of the folder the agent worked in (not the path). */
  project?: string;
  /** Counts the agent exposed (U13). A field is absent when the agent does not report it; never zero in its place. */
  usage?: RunUsage;
}

/** Counts only. `cost_micros` is millionths of `cost_currency` and is only ever what the agent itself reported. */
export interface RunUsage {
  input_tokens?: number;
  output_tokens?: number;
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  cost_micros?: number;
  cost_currency?: string;
  /** Times the agent said it was rate limited during the task. */
  rate_limited?: number;
}

/** Oldest first. `truncated`: more tasks ended in the range than one answer carries and the oldest were left out. */
export interface HistoryMessage {
  type: 'history';
  from: number;
  to: number;
  runs: HistoryRun[];
  truncated: boolean;
  keep_days: number;
  max_bytes: number;
}

/** Decision 0103. Unknown tiers and prompt kinds are read as `t3` and `choice`; extra fields are ignored. Options carry no keys. */
export interface SessionViewMessage {
  type: 'session_view';
  session: string;
  tier: 't1' | 't2' | 't3' | string;
  agent?: string;
  /** The cleaned screen tail, at most 30 lines. Empty with `unchanged` when the client's copy is current. */
  tail: string[];
  hash: string;
  unchanged?: boolean;
  task?: string;
  prompt?: { id: string; question: string; kind: 'choice' | 'yes_no' | string; options: { label: string }[] };
  /** Tier 1 only (decision 0103): the steps read from the agent's transcript. Older servers never send it. */
  timeline?: TimelineMessage;
}

/** T0: a change to the client's copy of an ACP session. Every text is untrusted agent text, cleaned again on arrival. */
export interface AcpViewMessage {
  type: 'acp_view';
  session: string;
  epoch: number;
  rev: number;
  /** `entries` is everything the server keeps: drop what you hold. */
  reset: boolean;
  /** The lowest entry id the server still keeps. */
  first: number;
  entries: AcpEntryWire[];
  /** Present on a reset and when the facts changed. */
  info?: AcpInfo;
}

export interface AcpDiff { path: string; old_text?: string; new_text: string; truncated?: boolean }
export interface AcpLocation { path: string; line?: number }
export interface AcpOptionWire { id: string; kind: string; name: string }
export type AcpEntryWire = { id: number; rev: number } & (
  | { kind: 'user'; text: string }
  | { kind: 'message' | 'thought'; text: string; truncated?: boolean }
  | { kind: 'tool'; tool_id: string; title: string; tool_kind: string; status: string; diffs?: AcpDiff[]; locations?: AcpLocation[] }
  | { kind: 'plan'; items: { text: string; status: string }[] }
  | { kind: 'permission'; prompt_id: string; tool_id: string; title: string; tool_kind: string; options: AcpOptionWire[]; diffs?: AcpDiff[]; plan?: string; state: 'open' | 'answered' | 'cancelled' | string; chosen?: string }
  | { kind: 'notice'; level: 'info' | 'error' | string; text: string }
  | { kind: string }
);

export interface AcpInfo {
  agent: string;
  dir: string;
  title?: string;
  status: 'running' | 'ended' | 'failed' | string;
  status_text?: string;
  working: boolean;
  mode?: string;
  modes?: { id: string; name: string; description?: string }[];
  commands?: { name: string; description?: string; hint?: string }[];
  usage?: { used: number; size: number; cost?: string };
}

export interface TimelineMessage {
  entries: { kind: 'prompt' | 'tool' | 'message' | string; name?: string; text: string }[];
  plan?: { text: string; status: string }[];
  files?: string[];
  last_message?: string;
}

export interface SessionInfo {
  id: string;
  name: string;
  agent: 'tmux' | 'codex' | null;
  /** The session manager it lives in; set when the server runs more than one (`tmux`). */
  backend?: string;
}

/** One window of a multiplexer session, as the windows dropdown lists it. */
export interface WindowInfo {
  index: number;
  name: string;
  /** Foreground command of the window's active pane (`bash`, `claude`, `lazygit`). */
  command: string;
  path: string;
  active: boolean;
  /** Set when `command` is a known coding agent. */
  agent?: string;
}

export type AgentState = 'idle' | 'working' | 'needs_input' | 'done' | 'error' | 'unknown';

export interface SessionAgentState {
  session: string;
  state: AgentState;
  seq: number;
  stale: boolean;
  ts: number;
  /** The agent in the session (`claude`, `codex`); absent for a plain shell and from older servers. */
  agent?: string;
  /** When the agent last answered (ms since the epoch); absent until it has. */
  last_response_at?: number;
  /** Start of the continuous human wait; absent on older servers. */
  waiting_since?: number;
}

/** Decision 0095: what an agent does now. Unknown values are treated as `unknown`. */
export type Activity = 'working' | 'blocked' | 'idle' | 'unknown';

export type AttentionKind = 'needs_input' | 'finished_unseen' | 'failed' | 'unknown';

/** The current piece of work of a session; `started_at` is ms since the epoch on the server's clock. */
export interface Task {
  title: string;
  started_at: number;
  source: string;
}

export interface SessionActivity {
  session: string;
  activity: Activity;
  ts: number;
  task?: Task;
  agent?: string;
  /** What scan or screen suspect without a hook: low confidence, never pushed. */
  hint?: AttentionKind;
}

/** A durable, server-owned item that asks for the owner. */
export interface Attention {
  id: string;
  session: string;
  kind: AttentionKind;
  raised_at: number;
  task?: Task;
  /** The question of a `needs_input`. */
  prompt?: string;
}

/** Plan v6: `seq` and `ts` come from the server; unknown `kind`s and extra fields are ignored. */
export type AgentEvent = { session: string; seq: number; ts: number; source: string; confidence: string } & (
  | { kind: 'state'; state: AgentState }
  | { kind: 'tool'; name: string; summary: string; phase: 'started' | 'finished' }
  | { kind: 'plan'; items: { text: string; status: string }[] }
  | { kind: 'prompt'; id: string; prompt_kind: string; choices: string[] }
  | { kind: 'message'; summary: string }
  | { kind: string }
);

/** Legacy message of the parked termenbox-agent. */
export type AgentEventType = 'spawned' | 'completed' | 'error';

/** Decision 0123: the ways a server can be reached, each an address around one pinned identity. */
export type CandidateKind = 'lan' | 'wan4' | 'wan6' | 'overlay' | 'tunnel';
export interface Candidate { kind: CandidateKind; addr: string; port: number }
/** A verified candidate bundle (wire form and layout: packages/core/src/bundle.rs, lib/bundle.ts). */
export interface Bundle {
  /** The server's Ed25519 public key, base64url. */
  server_id: string;
  seq: number;
  /** Unix seconds. */
  expires: number;
  /** The server publishes to the directory: only then may a claimed pairing be linked to the account. */
  published: boolean;
  fingerprint: string;
  name: string;
  candidates: Candidate[];
}

export interface ServerListItem {
  /** Tunnel (WebSocket) fallback or manually entered address; optional once a directory record exists. */
  server_url: string;
  /** Pinned Ed25519 public key of the server (base64url); the directory is trusted only through it. */
  server_id?: string;
  /** Last verified directory record, tried first before asking the directory again. */
  record?: import('./directory').DirRecord;
  server_name: string;
  registered_at: number;
  /** One-time claim token from the registration link; dropped after the first successful connect. */
  claim?: string;
  /** The newest verified candidate bundle (wire string) and its `seq`; a hint to race, never the server's identity. */
  bundle?: string;
  bundle_seq?: number;
  /** What that bundle said: this server publishes to the directory. */
  published?: boolean;
  /** The path that connected last (`lan 192.168.1.20:4443`), tried first next time. */
  winner?: string;
}
