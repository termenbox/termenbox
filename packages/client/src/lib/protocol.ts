export type ClientMessage =
  /** `protocol`: the termenbox protocol version the client speaks; a server that predates versions ignores it. */
  | { type: 'hello'; protocol?: number }
  | { type: 'auth'; cert: string; signature: string; server_token: string; channels?: boolean }
  /** Decision 0117: sent instead of `auth`; the token authorises a read-only preview and nothing is bound. */
  | { type: 'claim_preview'; server_token: string }
  | { type: 'revoke_device'; device: string }
  /** Owner-only: install the newer release now; the answer is `update_requested`, and the service restarts when it is done. */
  | { type: 'update_server' }
  /** Owner-only (decision 0154): check for a release now without installing it; the answer is `update_check_requested`, then `update_state`. */
  | { type: 'update_check' }
  /** Owner-only: one more grace period before a scheduled install; offered only while `update_state.can_postpone`. */
  | { type: 'update_postpone' }
  /** Owner-only: stop offering this release (the one the owner saw); a newer one is offered again. */
  | { type: 'update_skip'; version: string }
  /** Owner-only (decision 0144): asks for the scrubbed diagnostics bundle; the answer is `diagnostics_bundle` parts. */
  | { type: 'diagnostics_bundle' }
  | { type: 'uninstall'; confirm_name: string; delete_user?: boolean; remove_tmux?: boolean; kill_workspaces?: boolean }
  | { type: 'agent_sync'; after: Record<string, number> }
  | { type: 'peek'; workspace: string; tab?: string; hash?: string }
  | { type: 'scrollback'; workspace: string }
  | { type: 'answer'; workspace: string; tab?: string; prompt_id: string; seq: number; choice: number }
  | { type: 'reply'; workspace: string; text: string }
  | { type: 'windows'; workspace: string }
  | { type: 'select_window'; workspace: string; index: number }
  | { type: 'new_window'; workspace: string }
  | { type: 'close_window'; workspace: string; index: number }
  | { type: 'close_workspace'; workspace: string }
  /** Opens a tab in a workspace; the first tab comes with the workspace. */
  | { type: 'new_tab'; workspace: string; template: string; label?: string; dir?: string }
  /** Renames a tab; its id, process and tmux window stay. */
  | { type: 'rename_tab'; workspace: string; tab: string; label: string }
  /** Kills a tab and removes it; removing the last one leaves a shell tab behind. */
  | { type: 'close_tab'; workspace: string; tab: string }
  | { type: 'move_tab'; workspace: string; tab: string; index: number }
  /** Makes a tab the default a new device opens; it never switches another device's screen. */
  | { type: 'focus_tab'; workspace: string; tab: string }
  | { type: 'move_workspace'; workspace: string; index: number }
  /** Sets the name shown for a workspace; an empty name clears it. The workspace's id does not change. */
  | { type: 'rename_workspace'; workspace: string; name: string }
  | { type: 'list_workspaces' }
  /** Decision 0147: the directories inside `path` (the server user's home when absent), owner only; answered with `dir_listing` or `dir_error`. */
  | { type: 'list_dir'; path?: string; prefix?: string; hidden?: boolean }
  /** U14: the server's in-memory health samples; the answer is `health_history`. Older servers do not answer. */
  | { type: 'health_history' }
  | { type: 'new_workspace'; name: string; backend?: string; dir?: string; template?: string }
  | { type: 'ping' }
  | { type: 'attach'; workspace_id: string; tab?: string }
  | { type: 'detach' }
  | { type: 'input'; data: string }
  | { type: 'resize'; cols: number; rows: number }
  | { type: 'push_info' }
  | { type: 'push_subscribe'; endpoint: string; p256dh: string; auth: string }
  | { type: 'push_unsubscribe'; endpoint: string }
  /** The workspace this device shows in the foreground now, or null (hidden, or on the list): no push for it. */
  | { type: 'foreground'; workspace: string | null; tab?: string | null }
  | { type: 'ack'; attention_id: string }
  | { type: 'dismiss'; attention_id: string }
  /** Decision 0103: asks for the workspace view (cleaned tail, parsed prompt, tier); `hash` is the one already held. */
  | { type: 'workspace_view'; workspace: string; hash?: string }
  /** Taps option `option` (0-based) of prompt `prompt_id`; the server sends the keys its profile lists for it, never more. */
  | { type: 'workspace_answer'; workspace: string; prompt_id: string; option: number }
  /** U10: the hook state of one agent on this server. `preview` answers with the exact settings file `apply` will write. */
  | { type: 'agent_hooks'; agent: string; action: 'status' | 'preview' | 'apply' | 'remove' }
  /** T0 (decision 0111): shows this ACP workspace on this connection; `after` and `epoch` are the revision and stream the client holds. */
  | { type: 'acp_open'; workspace: string; after?: number; epoch?: number }
  /** Resumes the conversation's `session/update` log (step 3.4); the reducer that consumes it is step 5.1. */
  | { type: 'acp_events'; workspace: string; after?: number; epoch?: number }
  | { type: 'acp_close' }
  /** The owner's next prompt (`session/prompt`); several lines allowed. */
  | { type: 'acp_prompt'; workspace: string; text: string }
  | { type: 'acp_interrupt'; workspace: string }
  /** Answers a permission with exactly the option id the agent offered. */
  | { type: 'acp_answer'; workspace: string; prompt_id: string; option_id: string }
  /** Switches to a mode the agent listed. */
  | { type: 'acp_mode'; workspace: string; mode: string }
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

/** The updater's state as a server reports it (`update_state`); a field an older server omits reads as empty. */
export interface UpdateInfo {
  /** `idle`, `available`, `scheduled`, `installing` or `rolled-back`. */
  phase?: string;
  available?: string;
  /** When a scheduled install starts, epoch seconds; 0 when none is scheduled. */
  deadline?: number;
  checked_at?: number;
  checked_ok_at?: number;
  /** Why the last check failed; empty when it did not. */
  error?: string;
  can_postpone?: boolean;
}

export type ServerMessage =
  | { type: 'challenge'; nonce: string; server_id: string }
  | { type: 'device_revoked'; device: string }
  /** Answer to `claim_preview`: `mode` is `container` or `host`; `run_user` is empty in a container. */
  | { type: 'claim_preview'; name: string; mode: string; run_user?: string; version: string }
  | { type: 'update_requested'; available?: string }
  /** The recorded newer release changed while connected; empty when up to date (same as `auth_ok.update_available`). */
  | { type: 'update_status'; available?: string }
  | { type: 'update_check_requested' }
  /** What the updater knows (decision 0154), sent after `auth_ok` and on every change. Times are epoch seconds, 0 when unknown. */
  | ({ type: 'update_state' } & UpdateInfo)
  | { type: 'uninstall_done'; mode: string }
  /** Decision 0123: the server's signed candidate bundle, pushed after `auth_ok`; verify it before use (lib/bundle.ts). */
  | { type: 'candidates'; bundle: string }
  | { type: 'auth_ok'; protocol?: number; control_channel?: boolean; channels?: string[]; tabs?: boolean; user: string; server_name: string; mode?: string; version?: string; backends?: string[]; control_mode?: boolean; tmux_version?: string; prefix?: string; acp_agents?: string[]; update_available?: string; update_capabilities?: string[] }
  | { type: 'workspaces'; workspaces: WorkspaceInfo[] }
  /** The answer to `list_dir`: directory names only, at most 500 (`truncated` says more matched). `parent` is absent at `home`. */
  | { type: 'dir_listing'; path: string; parent?: string | null; entries: DirEntry[]; truncated?: boolean; home: string }
  /** `list_dir` was refused or failed; `reason` is a short sentence to show. */
  | { type: 'dir_error'; reason: string }
  | { type: 'workspace_attached'; workspace_id: string; tab?: string | null }
  | { type: 'output'; data: string }
  /** Never on the wire as JSON: terminal bytes from the `term` lane, as `Connection` hands them to listeners. */
  | { type: 'term'; bytes: Uint8Array }
  /** One part of everything a terminal holds, history included (the answer to `scrollback`); `last` closes it. */
  | { type: 'scrollback'; workspace: string; part: number; last: boolean; text: string }
  | { type: 'snapshot'; workspace: string; text: string; hash: string; unchanged?: boolean }
  | { type: 'window_list'; workspace: string; windows: WindowInfo[]; representative?: number | null }
  | { type: 'agent_snapshot'; states: WorkspaceAgentState[] }
  | { type: 'tab_states'; states: TabAgentState[] }
  | { type: 'agent_events'; events: AgentEvent[] }
  | { type: 'activity'; workspaces: WorkspaceActivity[] }
  | { type: 'attention'; items: Attention[] }
  | SessionViewMessage
  | AcpViewMessage
  | { type: 'push_info'; configured: boolean; public_key: string; devices: number; subscribed: boolean; settings?: boolean }
  | { type: 'push_settings'; settings: PushSettings }
  | { type: 'push_settings_rejected'; reason: string }
  | { type: 'agent_event'; event: AgentEventType; agent: string; workspace_id: string }
  | { type: 'agent_hooks'; agent: string; action: string; state: 'connected' | 'not_connected' | 'unsupported' | 'unavailable' | string; preview?: string; error?: string }
  | ({ type: 'server_health' } & ServerHealth)
  | HealthHistoryMessage
  | HistoryMessage
  | { type: 'history_deleted'; removed: number }
  | { type: 'error'; message: string }
  | { type: 'pong' }
  /** The answer to a control message that carried a `req` (step 3.3). */
  /** `session/update` frames of an ACP conversation, each stamped with the `seq` extension; `reset` on the first message of a compacted replay (step 3.4). */
  | { type: 'acp_events'; workspace: string; epoch: number; last: number; reset?: boolean; frames: unknown[] }
  | { type: 'control_ack'; req: number; ok: boolean; error?: string }
  /** The server's last run ended without a clean exit (decision 0142). `at_ms` is when it was last seen alive: show it once per value. */
  | { type: 'previous_exit'; at_ms: number; streak: number; crashed: boolean }
  /** One part (from 0, in order) of the answer to `diagnostics_bundle`: the direct transport carries 64 KiB a message. */
  | { type: 'diagnostics_bundle'; part: number; parts: number; text: string };

/** One health sample in whole percents. A metric the server could not read is absent, never 0 (the first CPU sample is always absent). */
export interface HealthPoint { ts: number; cpu_percent?: number; ram_percent?: number; disk_percent?: number; event_log_bytes?: number }

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
  workspace: string;
  agent?: string;
  task?: string;
  started_at: number;
  ended_at?: number;
  /** `stopped` is an end nobody reported: interrupted, or the workspace went away. Other values are read as `stopped`. */
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
  type: 'workspace_view';
  workspace: string;
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

/** T0: a change to the client's copy of an ACP workspace. Every text is untrusted agent text, cleaned again on arrival. */
export interface AcpViewMessage {
  type: 'acp_view';
  workspace: string;
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

/** One directory in a `dir_listing`: a name only, never a path. */
export interface DirEntry { name: string; symlink?: boolean; hidden?: boolean }

export interface WorkspaceInfo {
  id: string;
  name: string;
  agent: string | null;
  /** The workspace manager it lives in; set when the server runs more than one (`tmux`). */
  backend?: string;
  /** The workspace's absolute directory; empty when the backend does not know one. */
  dir?: string;
  /** The server's own order of workspaces. */
  order?: number;
  /** The tab a new device opens; absent falls back to the first. */
  focus?: string;
  /** The process tabs in order; absent from a server that predates tabs. */
  tabs?: TabInfo[];
}

/** One process tab of a workspace (decision 0149); `window` is the tmux window it runs in. */
export interface TabInfo {
  id: string;
  label: string;
  /** The template it started from (`shell`, `claude`, ...). */
  kind: string;
  /** A directory inside the workspace, relative to it; empty means the workspace directory. */
  dir: string;
  order: number;
  /** Absent while the workspace is dormant or the backend has no windows. */
  window?: string | null;
}

/** One window of a multiplexer workspace, as the windows dropdown lists it. */
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

export interface WorkspaceAgentState {
  workspace: string;
  state: AgentState;
  seq: number;
  stale: boolean;
  ts: number;
  /** The agent in the workspace (`claude`, `codex`); absent for a plain shell and from older servers. */
  agent?: string;
  /** When the agent last answered (ms since the epoch); absent until it has. */
  last_response_at?: number;
  /** Start of the continuous human wait; absent on older servers. */
  waiting_since?: number;
}

/** The prompt one tab waits on (decision 0149); the choices are labels, the keys stay on the server. */
export interface TabPrompt {
  id: string;
  kind: string;
  choices: string[];
}

/** What one tab's agent does now; the workspace-level state remains the aggregate. */
export interface TabAgentState {
  workspace: string;
  tab: string;
  state: AgentState;
  agent?: string;
  ts: number;
  waiting_since?: number;
  last_response_at?: number;
  prompt?: TabPrompt;
}

/** Decision 0095: what an agent does now. Unknown values are treated as `unknown`. */
export type Activity = 'working' | 'blocked' | 'idle' | 'unknown';

export type AttentionKind = 'needs_input' | 'finished_unseen' | 'failed' | 'unknown';

/** The current piece of work of a workspace; `started_at` is ms since the epoch on the server's clock. */
export interface Task {
  title: string;
  started_at: number;
  source: string;
}

export interface WorkspaceActivity {
  workspace: string;
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
  workspace: string;
  /** The tab that asked, when a hook proved it (decision 0149); older servers send none. */
  tab?: string;
  kind: AttentionKind;
  raised_at: number;
  task?: Task;
  /** The question of a `needs_input`. */
  prompt?: string;
}

/** Plan v6: `seq` and `ts` come from the server; unknown `kind`s and extra fields are ignored. */
export type AgentEvent = { workspace: string; seq: number; ts: number; source: string; confidence: string } & (
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
  /** The owner's signaling Worker (`wss://`), when the server has one (DECISION-0141). */
  signal?: string;
  /** The owner has a TURN key: a relay can be asked for over the signaling channel. A flag, never a credential. */
  relay?: boolean;
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

/** Messages that travel on the control channel (decision 0139); a `req` on any of them asks for a `control_ack`. The
 *  server's copy is `ClientMessage::is_control`; a test checks the two lists agree. */
export const CONTROL_TYPES: ReadonlySet<string> = new Set([
  'acp_interrupt', 'acp_answer', 'acp_prompt', 'acp_mode', 'answer', 'workspace_answer', 'detach', 'close_workspace',
  'rename_workspace', 'new_workspace', 'new_window', 'close_window', 'select_window', 'new_tab', 'rename_tab',
  'close_tab', 'move_tab', 'focus_tab', 'move_workspace', 'resize', 'foreground',
]);
