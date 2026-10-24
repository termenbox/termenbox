use serde::{Deserialize, Serialize};
use std::fmt::Display;

/// The push escalation settings (decision 0104, U11). Seconds, so a client never has to guess the unit.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PushSettings {
    /// Wait after the first push before re-notifying.
    #[serde(default = "default_initial_wait_secs")]
    pub initial_wait_secs: u64,
    /// The interval never grows past this.
    #[serde(default = "default_max_interval_secs")]
    pub max_interval_secs: u64,
    /// Applied to the interval after each fire.
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,
    /// `HH:MM-HH:MM` in 24-hour local time; absent or empty disables quiet hours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<String>,
    /// Minutes the owner's local time is ahead of UTC (the browser's, saved with the settings): the server keeps UTC.
    #[serde(default)]
    pub utc_offset_minutes: i32,
    /// Agent names whose pushes are silenced.
    #[serde(default)]
    pub muted_agents: Vec<String>,
}

fn default_initial_wait_secs() -> u64 {
    600
}
fn default_max_interval_secs() -> u64 {
    3600
}
fn default_multiplier() -> f64 {
    2.0
}

impl Default for PushSettings {
    fn default() -> Self {
        Self {
            initial_wait_secs: default_initial_wait_secs(),
            max_interval_secs: default_max_interval_secs(),
            multiplier: default_multiplier(),
            quiet_hours: None,
            utc_offset_minutes: 0,
            muted_agents: Vec::new(),
        }
    }
}

impl PushSettings {
    /// A rung can be as short as a minute and as long as a week; below a minute the ladder is a notification storm,
    /// above a week it is not a ladder any more.
    pub const MIN_WAIT_SECS: u64 = 60;
    pub const MAX_WAIT_SECS: u64 = 7 * 24 * 60 * 60;
    pub const MAX_MULTIPLIER: f64 = 10.0;
    pub const MIN_UTC_OFFSET: i32 = -12 * 60;
    pub const MAX_UTC_OFFSET: i32 = 14 * 60;
    pub const MAX_MUTED_AGENTS: usize = 32;
    pub const MAX_AGENT_NAME: usize = 64;

    /// `(start, end)` minutes since midnight, or `None` when quiet hours are off or unreadable.
    pub fn quiet_minutes(&self) -> Option<(u32, u32)> {
        let text = self.quiet_hours.as_deref()?.trim();
        if text.is_empty() {
            return None;
        }
        let (start, end) = text.split_once('-')?;
        Some((parse_hhmm(start)?, parse_hhmm(end)?))
    }

    /// Whether `minutes` (since midnight) falls inside quiet hours; `start == end` means never.
    pub fn quiet_at(&self, minutes: u32) -> bool {
        let Some((start, end)) = self.quiet_minutes() else {
            return false;
        };
        if start == end {
            return false;
        }
        if start < end {
            minutes >= start && minutes < end
        } else {
            minutes >= start || minutes < end
        }
    }

    /// Whether the owner's local time at `now_ms` (ms since the epoch, UTC) falls inside quiet hours.
    pub fn quiet_at_ms(&self, now_ms: u64) -> bool {
        let local = (now_ms / 60_000) as i64 + self.utc_offset_minutes as i64;
        self.quiet_at(local.rem_euclid(24 * 60) as u32)
    }

    /// Checks before the value is persisted or used. The reason is safe to show to the owner.
    pub fn validate(&self) -> Result<(), String> {
        if !(Self::MIN_WAIT_SECS..=Self::MAX_WAIT_SECS).contains(&self.initial_wait_secs) {
            return Err(format!(
                "Re-notify after must be between {} and {} seconds",
                Self::MIN_WAIT_SECS,
                Self::MAX_WAIT_SECS
            ));
        }
        if !(self.initial_wait_secs..=Self::MAX_WAIT_SECS).contains(&self.max_interval_secs) {
            return Err(
                "Max interval must be at least the re-notify wait and at most a week".into(),
            );
        }
        if !self.multiplier.is_finite()
            || self.multiplier <= 1.0
            || self.multiplier > Self::MAX_MULTIPLIER
        {
            return Err(format!(
                "Multiplier must be greater than 1 and at most {}",
                Self::MAX_MULTIPLIER
            ));
        }
        if let Some(text) = self.quiet_hours.as_deref().map(str::trim) {
            if !text.is_empty() && self.quiet_minutes().is_none() {
                return Err("Quiet hours must look like 22:00-07:00".into());
            }
        }
        if !(Self::MIN_UTC_OFFSET..=Self::MAX_UTC_OFFSET).contains(&self.utc_offset_minutes) {
            return Err("The time zone offset is out of range".into());
        }
        if self.muted_agents.len() > Self::MAX_MUTED_AGENTS {
            return Err(format!("At most {} muted agents", Self::MAX_MUTED_AGENTS));
        }
        if self
            .muted_agents
            .iter()
            .any(|a| a.trim().is_empty() || a.len() > Self::MAX_AGENT_NAME)
        {
            return Err("Muted agent names must be 1–64 characters".into());
        }
        Ok(())
    }

    /// Repairs a value read from disk instead of refusing it: a hand-edited file must not stop the ladder.
    pub fn sanitized(mut self) -> Self {
        self.initial_wait_secs = self
            .initial_wait_secs
            .clamp(Self::MIN_WAIT_SECS, Self::MAX_WAIT_SECS);
        self.max_interval_secs = self
            .max_interval_secs
            .clamp(self.initial_wait_secs, Self::MAX_WAIT_SECS);
        if !self.multiplier.is_finite() || self.multiplier <= 1.0 {
            self.multiplier = default_multiplier();
        }
        self.multiplier = self.multiplier.min(Self::MAX_MULTIPLIER);
        if self.quiet_minutes().is_none() {
            self.quiet_hours = None;
        }
        self.utc_offset_minutes = self
            .utc_offset_minutes
            .clamp(Self::MIN_UTC_OFFSET, Self::MAX_UTC_OFFSET);
        self.muted_agents = self
            .muted_agents
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty() && a.len() <= Self::MAX_AGENT_NAME)
            .take(Self::MAX_MUTED_AGENTS)
            .collect();
        self
    }
}

fn parse_hhmm(text: &str) -> Option<u32> {
    let (h, m) = text.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h <= 23 && m <= 59).then_some(h * 60 + m)
}

/// Client → Server (WebSocket)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Opens the handshake; the server answers with a `challenge`.
    Hello,
    /// Device certificate plus a signature over the challenge (DECISION-0062); no GitHub token.
    Auth {
        cert: String,
        signature: String,
        server_token: String,
        /// Asks for terminal bytes on their own lane (decision 0096); absent from older clients, which keep
        /// the single JSON pipe.
        #[serde(default)]
        channels: bool,
    },
    /// Decision 0117: instead of `auth`, asks what a claim link would add. The token authorises it; the answer is
    /// `claim_preview`. It binds nobody, leaves the token valid and needs no device certificate.
    ClaimPreview {
        server_token: String,
    },
    /// Owner-only: refuse this device key from now on.
    RevokeDevice {
        device: String,
    },
    /// Owner-only, recently signed in: remove this server (decision 0072). `confirm_name` must equal the
    /// server's name; the flags choose the optional host cleanup (all off by default).
    Uninstall {
        confirm_name: String,
        #[serde(default)]
        delete_user: bool,
        #[serde(default)]
        remove_tmux: bool,
        #[serde(default)]
        kill_sessions: bool,
    },
    /// Asks for the agent state of every session and for the events after `after[session]` (plan v6, P.6).
    AgentSync {
        #[serde(default)]
        after: std::collections::HashMap<String, u64>,
    },
    /// A one-tap answer to a prompt the user saw: accepted only while that prompt (`prompt_id` at event `seq`) is
    /// still the session's current one, reliable (hook-confirmed) and still on the pane (plan v8, K.4).
    Answer {
        session: String,
        prompt_id: String,
        seq: u64,
        choice: u8,
    },
    /// Types a short reply into a session's pane without attaching to it (plan v8, K.5).
    Reply {
        session: String,
        text: String,
    },
    /// Asks for a snapshot of a session's screen without attaching to it (decision 0084). `hash` is the one the
    /// client already holds: the server answers `unchanged` instead of sending the same text again.
    Peek {
        session: String,
        #[serde(default)]
        hash: Option<String>,
    },
    /// Asks for everything a session's terminal still holds, history included, as text (answered with `scrollback`
    /// parts). It is what select-all selects when the cursor is not in the prompt.
    Scrollback {
        session: String,
    },
    /// Asks for the windows of a session (answered with `window_list`; later changes are pushed).
    Windows {
        session: String,
    },
    /// Makes window `index` the one a client sees when it attaches (plan H.6).
    SelectWindow {
        session: String,
        index: u32,
    },
    /// Opens a new window in the session, in the directory of its current window, and makes it the one
    /// attached terminals show.
    NewWindow {
        session: String,
    },
    /// Closes one window of a session; closing the last one ends the session.
    CloseWindow {
        session: String,
        index: u32,
    },
    /// Ends the session and everything in it (the client has asked the user first). Different from `Detach`,
    /// which only stops streaming and leaves the session running.
    CloseSession {
        session: String,
    },
    /// Gives the session a display name; its id (what hooks, tabs and layouts key on) is unchanged. An empty name
    /// takes the label off again.
    RenameSession {
        session: String,
        name: String,
    },
    ListSessions,
    NewSession {
        name: String,
        /// Which session manager to create it in (`tmux`, `command`); the first one when absent.
        #[serde(default)]
        backend: Option<String>,
        /// Absolute working directory to start in; the manager's default when absent.
        #[serde(default)]
        dir: Option<String>,
        /// A coding agent to start in the new session (`claude`, `codex`, ...); a plain shell when absent.
        #[serde(default)]
        template: Option<String>,
    },
    Ping,
    Attach {
        session_id: String,
    },
    /// Stop streaming the current pane while keeping authentication and agent events alive.
    Detach,
    Input {
        data: String,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Asks whether this server can send Web Push, and its public key.
    PushInfo,
    /// Asks for the server's push escalation settings (U11).
    PushSettingsGet,
    /// Replaces the push escalation settings; the answer is `push_settings` or `push_settings_rejected`.
    PushSettingsSet {
        settings: PushSettings,
    },
    /// This device's push subscription (the browser's `PushSubscription` fields).
    PushSubscribe {
        endpoint: String,
        p256dh: String,
        auth: String,
    },
    PushUnsubscribe {
        endpoint: String,
    },
    /// Which session this device shows in the foreground right now (none when hidden or on the list): the server
    /// sends no push for a session someone is looking at.
    Foreground {
        #[serde(default)]
        session: Option<String>,
    },
    /// A device shows this session in the foreground: clears its `finished_unseen` and `failed` items, never
    /// a `needs_input` (decision 0095).
    Ack {
        attention_id: String,
    },
    /// Explicitly dismiss an attention item.
    Dismiss {
        attention_id: String,
    },
    /// Asks for the session view (decision 0103): the cleaned screen tail, the parsed prompt and the tier in use.
    /// `hash` is the one the client already holds; the server then answers `unchanged`.
    SessionView {
        session: String,
        #[serde(default)]
        hash: Option<String>,
    },
    /// Taps option `option` (0-based, into the options the last `session_view` listed) of prompt `prompt_id`. The
    /// server sends the keys its profile lists for that option and nothing the client supplies.
    SessionAnswer {
        session: String,
        prompt_id: String,
        option: u8,
    },
    /// Owner-only (U10, decision 0106): the hook state of one agent on this server and what changes it. `action` is
    /// `status`, `preview` (the exact settings file as `apply` will write it), `apply` or `remove`; the answer is
    /// `agent_hooks`. Older servers do not answer.
    AgentHooks {
        agent: String,
        action: String,
    },
    /// T0 (decision 0111): shows this ACP session on this connection. The server answers with `acp_view` (everything
    /// after `after` when `epoch` is the one the client holds, else everything it keeps) and then pushes `acp_view`
    /// updates, a few per second at most, until `acp_close` or another `acp_open`.
    AcpOpen {
        session: String,
        #[serde(default)]
        after: Option<u64>,
        #[serde(default)]
        epoch: Option<u64>,
    },
    /// Stops the `acp_view` updates of this connection.
    AcpClose,
    /// The owner's next prompt (`session/prompt`); several lines are allowed, up to 16 KiB.
    AcpPrompt {
        session: String,
        text: String,
    },
    /// Interrupts the turn: open requests are answered `cancelled`, then `session/cancel`.
    AcpInterrupt {
        session: String,
    },
    /// Answers permission `prompt_id` with exactly the option id the agent offered (only one-time options are shown).
    AcpAnswer {
        session: String,
        prompt_id: String,
        option_id: String,
    },
    /// Switches to a mode the agent listed in `acp_view.info.modes`.
    AcpMode {
        session: String,
        mode: String,
    },
    /// U12 (decision 0105): the tasks that ended in `[from, to)` (ms since the epoch) plus any still running, from the
    /// server's history; the answer is `history`. The client picks the range, so day boundaries are its own.
    History {
        from: u64,
        to: u64,
    },
    /// Forgets the server's whole history; the answer is `history_deleted`.
    HistoryDelete,
    /// U14: asks for the server's recent health samples; the answer is `health_history`. Older servers do not answer.
    HealthHistory,
    /// A message from a newer client; ignored.
    #[serde(other)]
    Unknown,
}

/// Server → Client (WebSocket)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Fresh per connection; the device signs it together with the `server_id`.
    Challenge {
        nonce: String,
        server_id: String,
    },
    DeviceRevoked {
        device: String,
    },
    /// Answer to `claim_preview` (decision 0117): what the claim would add. No secret, session or path.
    /// `mode` is `container` or `host`; `run_user` is the OS user sessions run as (empty in a container).
    ClaimPreview {
        name: String,
        mode: String,
        #[serde(default)]
        run_user: String,
        version: String,
    },
    /// The server wiped its configuration and will stop; `mode` is `container` or `host:<user>`.
    UninstallDone {
        mode: String,
    },
    /// The server's signed candidate bundle (decision 0123), pushed after `auth_ok`: `bundle` is the wire string of
    /// `bundle::Bundle`. The client verifies it against the pinned server id before it keeps any of it.
    Candidates {
        bundle: String,
    },
    AuthOk {
        user: String,
        /// The lanes this connection uses besides JSON (`term`); absent means the single pipe.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        channels: Vec<String>,
        server_name: String,
        /// `container` or `host:<user>`; absent from older servers.
        #[serde(default)]
        mode: String,
        /// Server version and build (`0.1.0+abc1234`); absent from older servers.
        #[serde(default)]
        version: String,
        /// Session managers this server offers; more than one means "New session" asks which.
        #[serde(default)]
        backends: Vec<String>,
        /// Live window updates through tmux control mode are on; false from older servers and below tmux 3.2.
        #[serde(default)]
        control_mode: bool,
        /// The tmux version string (`3.6`), empty when there is no tmux; with `control_mode` false the client
        /// can say why live window updates are off.
        #[serde(default)]
        tmux_version: String,
        /// tmux's prefix key in its own notation (`C-b`); empty without tmux.
        #[serde(default)]
        prefix: String,
        /// The agents the owner allows as ACP sessions (decision 0111); empty when the server runs none.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        acp_agents: Vec<String>,
    },
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    SessionAttached {
        session_id: String,
    },
    Output {
        data: String,
    },
    /// A session's visible screen as text with its colour codes. Empty `text` with `unchanged` false means the
    /// session manager cannot give one; with `unchanged` true the client's copy is current.
    /// One part of the answer to `scrollback`, in order from 0; `last` closes it. A part is a few KB so that it fits
    /// one message of the direct transport; the client joins the parts. Text is the terminal's, never logged.
    Scrollback {
        session: String,
        part: u32,
        last: bool,
        text: String,
    },
    Snapshot {
        session: String,
        text: String,
        hash: String,
        #[serde(default)]
        unchanged: bool,
    },
    /// The windows of one session; sent for `windows` and again whenever they change.
    WindowList {
        session: String,
        windows: Vec<WindowInfo>,
        /// The window the session's tab speaks for (plan H.3): the agent window used last, else the active one.
        /// Its command and path name the tab and its agent gives the icon.
        #[serde(default)]
        representative: Option<u32>,
    },
    /// Latest agent state per session; answers `agent_sync`.
    AgentSnapshot {
        states: Vec<crate::events::SessionAgentState>,
    },
    /// New agent events, in `seq` order per session. A gap in `seq` means the client should `agent_sync`.
    AgentEvents {
        events: Vec<crate::events::AgentEvent>,
    },
    /// Answers `push_info`. `configured` is false until the owner has run `termenbox-server --setup-push`.
    PushInfo {
        configured: bool,
        /// The application server key to subscribe with (base64url); empty when not configured.
        public_key: String,
        /// Devices subscribed to this server.
        devices: u32,
        /// Whether the asking device's subscription is among them.
        subscribed: bool,
        /// Whether this server understands `push_settings_get`/`push_settings_set` (U11). Absent from older servers.
        #[serde(default)]
        settings: bool,
    },
    /// Answers `push_settings_get` and `push_settings_set` with the settings in force.
    PushSettings {
        settings: PushSettings,
    },
    /// Refuses `push_settings_set`; `reason` is safe to show.
    PushSettingsRejected {
        reason: String,
    },
    /// The derived Activity (with task) of every session; answers `agent_sync`, then follows each change.
    Activity {
        sessions: Vec<crate::events::SessionActivity>,
    },
    /// The open attention items, and again whenever one is raised or cleared.
    Attention {
        items: Vec<crate::events::Attention>,
    },
    /// Legacy message of the parked termenbox-agent.
    AgentEvent {
        event: AgentEventType,
        agent: String,
        session_id: String,
    },
    /// The session view (decision 0103); answers `session_view`.
    SessionView {
        session: String,
        /// `t1` (structured), `t2` (agent profile) or `t3` (generic). Clients read any other value as `t3`.
        tier: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
        /// The cleaned screen tail: at most 30 lines of 200 characters, control sequences and secrets removed.
        /// Empty when `unchanged`.
        tail: Vec<String>,
        hash: String,
        #[serde(default)]
        unchanged: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<ViewPrompt>,
        /// The structured timeline read from the agent's transcript (tier `t1`, decision 0103). Absent for `t2`
        /// and `t3`, and when `unchanged`; clients that do not know it ignore it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeline: Option<Timeline>,
    },
    /// Answers `agent_hooks`: `state` is `connected`, `not_connected`, `unsupported` or `unavailable` (clients read
    /// any other value as `unavailable`). `preview` is the whole settings file as `apply` will write it.
    AgentHooks {
        agent: String,
        action: String,
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// T0: the stream of an ACP session (see `AcpOpen`).
    AcpView(AcpView),
    /// Answers `history`, oldest first. `truncated` means more tasks ended in the range than one answer carries and the
    /// oldest were left out. `keep_days` and `max_bytes` are the limits this server keeps its history within.
    History {
        from: u64,
        to: u64,
        runs: Vec<HistoryRun>,
        truncated: bool,
        keep_days: u32,
        max_bytes: u64,
    },
    /// Answers `history_delete`: how many tasks were forgotten.
    HistoryDeleted {
        removed: u64,
    },
    Error {
        message: String,
    },
    Pong,
    /// U14 (decision 0105): the server's health, pushed after sign-in and after each 10 s sample. `healthy` is the
    /// server's own verdict (a level held for several samples, with a lower level to recover), so clients show it and
    /// never decide it. `unhealthy` names the metrics over their level (`cpu`, `ram`, `disk`). An older server sends
    /// none, and no dot appears.
    ServerHealth {
        healthy: bool,
        unhealthy: Vec<String>,
        /// `container` when the RAM figure is against a cgroup limit, `host` when it is against the machine.
        scope: String,
        #[serde(flatten)]
        point: HealthPoint,
    },
    /// Answers `health_history`: the in-memory samples, oldest first, at most 360 of them (each the highest of the
    /// `step_s` seconds it stands for, so a spike is not averaged away), with the levels the verdict uses.
    HealthHistory {
        /// Seconds between two samples the server takes.
        interval_s: u32,
        /// Seconds each returned point covers (a multiple of `interval_s`).
        step_s: u32,
        healthy: bool,
        unhealthy: Vec<String>,
        scope: String,
        /// How many samples in a row cross a level before the verdict flips either way.
        confirm: u32,
        levels: Vec<HealthLevel>,
        samples: Vec<HealthPoint>,
    },
    /// A message from a newer server; ignored.
    #[serde(other)]
    Unknown,
}

// ---------------------------------------------------------------------------------------------------
// T0: the ACP session stream (decision 0111). Every text is untrusted agent text; the server has already
// stripped control characters, scrubbed secrets and capped lengths, and clients render it as text only.
// Only what the protocol carries is here: raw tool input and tool output are never copied (decision 0095).
// ---------------------------------------------------------------------------------------------------

/// A change to apply to what the client holds of one ACP session.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AcpView {
    pub session: String,
    /// Names this run of the session's stream. A client that holds another epoch starts over.
    pub epoch: u64,
    /// The revision this message brings the client to; ask for `after = rev` next time.
    pub rev: u64,
    /// True when `entries` is everything the server keeps (the client drops what it held).
    pub reset: bool,
    /// The lowest entry id the server still keeps: older ones are dropped by the client too.
    pub first: u64,
    /// Entries created or changed since `after`, in id order. A changed entry is sent whole.
    pub entries: Vec<AcpEntry>,
    /// The session facts, sent when they changed or on a reset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<AcpInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AcpEntry {
    pub id: u64,
    pub rev: u64,
    #[serde(flatten)]
    pub body: AcpBody,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AcpBody {
    /// What the owner sent (`session/prompt`).
    User { text: String },
    /// The agent's reply, Markdown, grown by `agent_message_chunk`. `truncated`: the reply was longer than the view
    /// keeps and the rest is not shown.
    Message {
        text: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        truncated: bool,
    },
    /// `agent_thought_chunk` (`truncated` as for `Message`).
    Thought {
        text: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        truncated: bool,
    },
    /// A `tool_call` merged with its `tool_call_update`s by `toolCallId`. `tool_kind` is the protocol's `kind`
    /// (`read`, `edit`, `execute`, `switch_mode`, `other`, ...) and `status` is `pending`, `in_progress`,
    /// `completed` or `failed`.
    Tool {
        tool_id: String,
        title: String,
        tool_kind: String,
        status: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        diffs: Vec<AcpDiff>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<AcpLocation>,
    },
    /// The `plan` update, or the agent's todo list.
    Plan { items: Vec<crate::events::PlanItem> },
    /// A `session/request_permission`. `state` is `open`, `answered` (with `chosen`) or `cancelled`. Only
    /// `allow_once` and `reject_once` options are carried (decision 0104).
    Permission {
        prompt_id: String,
        /// The `toolCallId` the request is about: the view shows the diff once, here, while it is open.
        tool_id: String,
        title: String,
        tool_kind: String,
        options: Vec<crate::events::DecisionOption>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        diffs: Vec<AcpDiff>,
        /// The plan of a `switch_mode` request, Markdown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan: Option<String>,
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chosen: Option<String>,
    },
    /// Something the view says itself: `level` is `info` or `error` (a turn that ended `cancelled`, a failed prompt,
    /// a session that ended).
    Notice { level: String, text: String },
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AcpDiff {
    pub path: String,
    /// Absent for a new file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    pub new_text: String,
    /// The texts were cut to the size limit.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AcpLocation {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// A mode the agent offers (`session/new` `modes.availableModes`, or the `mode` config option).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AcpMode {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A slash command from `available_commands_update`; `hint` is its `input.hint`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AcpCommand {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AcpUsage {
    pub used: u64,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct AcpInfo {
    pub agent: String,
    /// The working directory the session was started in (the companion shell opens there).
    pub dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `running`, `ended` or `failed`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    /// A `session/prompt` is open.
    pub working: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<AcpMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<AcpCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AcpUsage>,
}

/// One health sample: whole percents, absent when the server could not read it (never zero in its place). CPU is
/// absent for the first sample after start, as it is a difference between two readings.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthPoint {
    #[serde(default)]
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ram_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_percent: Option<u8>,
}

/// A health level: a metric turns unhealthy at `enter` and healthy again at or below `exit`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HealthLevel {
    pub metric: String,
    pub enter: u8,
    pub exit: u8,
}

/// One task an agent worked on: metadata only, never a prompt or terminal text (decision 0105).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HistoryRun {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The task's title, scrubbed and capped by the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub started_at: u64,
    /// Absent while the task is still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<RunOutcome>,
    /// How long the agent waited on the user during the task.
    pub blocked_ms: u64,
    /// The folder name (not the path) the agent worked in, when the agent reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Counts the agent exposed during the task (U13); absent when it exposed none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<RunUsage>,
}

/// What an agent exposed about its use during one task: counts only. A field is absent when the agent does not report
/// it, never zero in its place. Tokens come from the agent's own reports (a Claude Code transcript, an ACP turn
/// result); the cost is only ever what the agent itself reported, never one worked out here.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct RunUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    /// Cost the agent reported, in millionths of `cost_currency`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_currency: Option<String>,
    /// Times the agent said it was rate limited during the task.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rate_limited: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// How a task ended. `stopped` is an end nobody reported: interrupted, or the session went away.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Finished,
    Failed,
    Stopped,
}

/// What an agent's transcript says about the session so far (decision 0103, tier 1). Every text is untrusted:
/// the server has already stripped, capped and scrubbed it, and clients render it as plain text only.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Timeline {
    /// The most recent steps, oldest first.
    pub entries: Vec<TimelineEntry>,
    /// The agent's own task list, when it keeps one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plan: Vec<crate::events::PlanItem>,
    /// File names the agent changed, most recent last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// The agent's last message to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    /// `prompt`, `tool` or `message`; clients read any other value as `message`.
    pub kind: String,
    /// The tool's name, for `tool`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The prompt, the tool's one-line summary or the message.
    pub text: String,
}

/// A decision the screen is waiting on. The options carry labels only: the keys stay on the server.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ViewPrompt {
    /// Names this prompt as drawn now; a `session_answer` for another id is refused.
    pub id: String,
    pub question: String,
    /// `choice` or `yes_no`; clients read any other value as `choice`.
    pub kind: String,
    pub options: Vec<ViewOption>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ViewOption {
    pub label: String,
}

/// One window of a multiplexer session, as the windows dropdown shows it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub index: u32,
    pub name: String,
    /// The foreground command of the window's active pane (`bash`, `claude`, `lazygit`).
    pub command: String,
    /// The working directory of that pane.
    pub path: String,
    pub active: bool,
    /// Set when `command` is a known coding agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub agent: Option<String>,
    /// The session manager it lives in (`tmux`, `command`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventType {
    Spawned,
    Completed,
    Error,
}

impl Display for AgentEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentEventType::Spawned => write!(f, "spawned"),
            AgentEventType::Completed => write!(f, "completed"),
            AgentEventType::Error => write!(f, "error"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_are_read_in_the_owners_local_time() {
        let night = |offset| PushSettings {
            quiet_hours: Some("22:00-07:00".into()),
            utc_offset_minutes: offset,
            ..PushSettings::default()
        };
        let at = |h: u64, m: u64| (h * 60 + m) * 60_000;
        // 23:30 UTC is 01:30 at +2 (quiet); 12:00 UTC is 14:00 at +2 (awake).
        assert!(night(0).quiet_at_ms(at(23, 30)));
        assert!(night(120).quiet_at_ms(at(23, 30)));
        assert!(!night(120).quiet_at_ms(at(12, 0)));
        // 05:00 UTC is 23:00 the evening before at -6 (quiet); 14:00 UTC is 08:00 there (awake).
        assert!(night(-360).quiet_at_ms(at(5, 0)));
        assert!(!night(-360).quiet_at_ms(at(14, 0)));
        // 06:30 UTC at +1 is 07:30: the end of the window is exclusive.
        assert!(!night(60).quiet_at_ms(at(6, 30)));
        assert!(PushSettings::default().validate().is_ok());
        assert!(night(15 * 60).validate().is_err());
    }

    /// The browser client speaks these exact strings (packages/client/src/lib/protocol.ts).
    #[test]
    fn test_wire_names_match_the_client() {
        let v = serde_json::to_value(ClientMessage::ListSessions).unwrap();
        assert_eq!(v["type"], "list_sessions");
        let v = serde_json::to_value(ServerMessage::AuthOk {
            user: "u".into(),
            channels: vec![],
            server_name: "s".into(),
            mode: "container".into(),
            version: "0.1.0+dev".into(),
            backends: vec!["tmux".into()],
            control_mode: true,
            tmux_version: "3.6".into(),
            prefix: "C-b".into(),
            acp_agents: vec![],
        })
        .unwrap();
        assert_eq!(v["type"], "auth_ok");
        assert!(v.get("acp_agents").is_none());
        let v = serde_json::to_value(ClientMessage::History { from: 1, to: 2 }).unwrap();
        assert_eq!(v["type"], "history");
        assert_eq!(v["from"], 1);
        let v = serde_json::to_value(ClientMessage::HealthHistory).unwrap();
        assert_eq!(v["type"], "health_history");
        let v = serde_json::to_value(ServerMessage::ServerHealth {
            healthy: false,
            unhealthy: vec!["ram".into()],
            scope: "container".into(),
            point: HealthPoint {
                ts: 5,
                cpu_percent: None,
                ram_percent: Some(93),
                disk_percent: Some(40),
            },
        })
        .unwrap();
        assert_eq!(v["type"], "server_health");
        assert_eq!(v["ram_percent"], 93);
        assert!(v.get("cpu_percent").is_none(), "unknown is absent, not 0");
        let v = serde_json::to_value(ClientMessage::HistoryDelete).unwrap();
        assert_eq!(v["type"], "history_delete");
        let v = serde_json::to_value(ServerMessage::HistoryDeleted { removed: 3 }).unwrap();
        assert_eq!(v["type"], "history_deleted");
        let v = serde_json::to_value(ServerMessage::History {
            from: 1,
            to: 2,
            runs: vec![HistoryRun {
                session: "api".into(),
                agent: None,
                task: None,
                started_at: 1,
                ended_at: Some(2),
                outcome: Some(RunOutcome::Stopped),
                blocked_ms: 0,
                project: None,
                usage: None,
            }],
            truncated: false,
            keep_days: 90,
            max_bytes: 20_000_000,
        })
        .unwrap();
        assert_eq!(v["type"], "history");
        assert_eq!(v["runs"][0]["outcome"], "stopped");
        assert!(v["runs"][0].get("task").is_none());
        let v = serde_json::to_value(ClientMessage::ClaimPreview {
            server_token: "t".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "claim_preview");
        assert_eq!(v["server_token"], "t");
        let v = serde_json::to_value(ServerMessage::ClaimPreview {
            name: "vps".into(),
            mode: "host".into(),
            run_user: "alice".into(),
            version: "0.1.0+dev".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "claim_preview");
        assert_eq!(v["run_user"], "alice");
        let v = serde_json::to_value(ClientMessage::AgentSync {
            after: [("a".to_string(), 4u64)].into(),
        })
        .unwrap();
        assert_eq!(v["type"], "agent_sync");
        assert_eq!(v["after"]["a"], 4);
        let v = serde_json::to_value(ClientMessage::Answer {
            session: "s".into(),
            prompt_id: "1-ab".into(),
            seq: 3,
            choice: 1,
        })
        .unwrap();
        assert_eq!(v["type"], "answer");
        assert_eq!(v["choice"], 1);
        let v = serde_json::to_value(ClientMessage::Scrollback { session: "s".into() }).unwrap();
        assert_eq!(v["type"], "scrollback");
        let v = serde_json::to_value(ServerMessage::Scrollback { session: "s".into(), part: 1, last: true, text: "t".into() }).unwrap();
        assert_eq!((v["type"].as_str(), v["part"].as_u64(), v["last"].as_bool()), (Some("scrollback"), Some(1), Some(true)));
        let v = serde_json::to_value(ClientMessage::Peek {
            session: "s".into(),
            hash: None,
        })
        .unwrap();
        assert_eq!(v["type"], "peek");
        let v = serde_json::to_value(ClientMessage::SessionView {
            session: "s".into(),
            hash: None,
        })
        .unwrap();
        assert_eq!(v["type"], "session_view");
        let v = serde_json::to_value(ClientMessage::SessionAnswer {
            session: "s".into(),
            prompt_id: "v-ab".into(),
            option: 2,
        })
        .unwrap();
        assert_eq!(v["type"], "session_answer");
        assert_eq!(v["option"], 2);
        let v = serde_json::to_value(ServerMessage::Snapshot {
            session: "s".into(),
            text: "x".into(),
            hash: "h".into(),
            unchanged: false,
        })
        .unwrap();
        assert_eq!(v["type"], "snapshot");
        let v = serde_json::to_value(ServerMessage::AgentSnapshot { states: vec![] }).unwrap();
        assert_eq!(v["type"], "agent_snapshot");
        let v = serde_json::to_value(ServerMessage::AgentEvents { events: vec![] }).unwrap();
        assert_eq!(v["type"], "agent_events");
        let v = serde_json::to_value(ServerMessage::Activity { sessions: vec![] }).unwrap();
        assert_eq!(v["type"], "activity");
        let v = serde_json::to_value(ServerMessage::Attention { items: vec![] }).unwrap();
        assert_eq!(v["type"], "attention");
        let v = serde_json::to_value(ClientMessage::Ack {
            attention_id: "a".into(),
        })
        .unwrap();
        assert_eq!(
            (v["type"].as_str(), v["attention_id"].as_str()),
            (Some("ack"), Some("a"))
        );
        let v = serde_json::to_value(ClientMessage::Dismiss {
            attention_id: "a".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "dismiss");
        let v = serde_json::to_value(ServerMessage::SessionAttached {
            session_id: "x".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "session_attached");
        let v = serde_json::to_value(ClientMessage::NewSession {
            name: "n".into(),
            backend: None,
            dir: Some("/srv/app".into()),
            template: Some("claude".into()),
        })
        .unwrap();
        assert_eq!(v["type"], "new_session");
        assert_eq!(v["dir"], "/srv/app");
        // The new messages the client speaks (packages/client/src/lib/protocol.ts).
        for (msg, name) in [
            (
                ClientMessage::Windows {
                    session: "s".into(),
                },
                "windows",
            ),
            (
                ClientMessage::SelectWindow {
                    session: "s".into(),
                    index: 1,
                },
                "select_window",
            ),
            (
                ClientMessage::NewWindow {
                    session: "s".into(),
                },
                "new_window",
            ),
            (
                ClientMessage::CloseWindow {
                    session: "s".into(),
                    index: 1,
                },
                "close_window",
            ),
            (
                ClientMessage::CloseSession {
                    session: "s".into(),
                },
                "close_session",
            ),
            (
                ClientMessage::RenameSession {
                    session: "s".into(),
                    name: "n".into(),
                },
                "rename_session",
            ),
        ] {
            assert_eq!(serde_json::to_value(msg).unwrap()["type"], name);
        }
        let v = serde_json::to_value(ServerMessage::WindowList {
            session: "s".into(),
            windows: vec![WindowInfo {
                index: 0,
                name: "w".into(),
                command: "claude".into(),
                path: "/srv".into(),
                active: true,
                agent: Some("claude".into()),
            }],
            representative: Some(0),
        })
        .unwrap();
        assert_eq!(v["type"], "window_list");
        assert_eq!(v["windows"][0]["agent"], "claude");
        // An older client's NewSession has neither field.
        let old: ClientMessage =
            serde_json::from_str(r#"{"type":"new_session","name":"n"}"#).unwrap();
        assert_eq!(
            old,
            ClientMessage::NewSession {
                name: "n".into(),
                backend: None,
                dir: None,
                template: None
            }
        );
        let v = serde_json::to_value(ClientMessage::Hello).unwrap();
        assert_eq!(v["type"], "hello");
        let v = serde_json::to_value(ServerMessage::Challenge {
            nonce: "n".into(),
            server_id: "s".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "challenge");
        let v = serde_json::to_value(ClientMessage::RevokeDevice { device: "d".into() }).unwrap();
        assert_eq!(v["type"], "revoke_device");
        let v: ClientMessage =
            serde_json::from_str(r#"{"type":"uninstall","confirm_name":"vps"}"#).unwrap();
        assert_eq!(
            v,
            ClientMessage::Uninstall {
                confirm_name: "vps".into(),
                delete_user: false,
                remove_tmux: false,
                kill_sessions: false
            }
        );
        let v = serde_json::to_value(ServerMessage::UninstallDone {
            mode: "container".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "uninstall_done");
    }

    #[test]
    fn test_client_message_roundtrip() {
        let msg = ClientMessage::Auth {
            cert: "cert".into(),
            signature: "sig".into(),
            server_token: "server".into(),
            channels: true,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_server_message_roundtrip() {
        let msg = ServerMessage::Sessions {
            sessions: vec![SessionInfo {
                id: "1".into(),
                name: "main".into(),
                agent: Some("tmux".into()),
                backend: None,
            }],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn an_auth_from_an_older_client_asks_for_no_lanes() {
        let m: ClientMessage =
            serde_json::from_str(r#"{"type":"auth","cert":"c","signature":"s","server_token":""}"#)
                .unwrap();
        assert!(matches!(
            m,
            ClientMessage::Auth {
                channels: false,
                ..
            }
        ));
        let ok: ServerMessage =
            serde_json::from_str(r#"{"type":"auth_ok","user":"u","server_name":"s"}"#).unwrap();
        assert!(matches!(ok, ServerMessage::AuthOk { channels, .. } if channels.is_empty()));
    }

    /// An older peer ignores a message or a field it does not know instead of failing (decision 0103).
    #[test]
    fn unknown_messages_parse_as_unknown_and_unknown_fields_are_ignored() {
        let m: ClientMessage = serde_json::from_str(r#"{"type":"from_the_future","x":1}"#).unwrap();
        assert_eq!(m, ClientMessage::Unknown);
        let m: ServerMessage = serde_json::from_str(r#"{"type":"from_the_future"}"#).unwrap();
        assert_eq!(m, ServerMessage::Unknown);
        let m: ClientMessage =
            serde_json::from_str(r#"{"type":"session_view","session":"s","extra":true}"#).unwrap();
        assert_eq!(
            m,
            ClientMessage::SessionView {
                session: "s".into(),
                hash: None
            }
        );
    }

    /// The ACP stream (T0): wire names the client speaks, additive fields, and an unknown entry kind ignored.
    #[test]
    fn acp_messages_use_the_wire_names_the_client_reads() {
        let m: ClientMessage =
            serde_json::from_str(r#"{"type":"acp_open","session":"s"}"#).unwrap();
        assert_eq!(
            m,
            ClientMessage::AcpOpen {
                session: "s".into(),
                after: None,
                epoch: None
            }
        );
        for (msg, name) in [
            (ClientMessage::AcpClose, "acp_close"),
            (
                ClientMessage::AcpPrompt {
                    session: "s".into(),
                    text: "hi".into(),
                },
                "acp_prompt",
            ),
            (
                ClientMessage::AcpInterrupt {
                    session: "s".into(),
                },
                "acp_interrupt",
            ),
            (
                ClientMessage::AcpAnswer {
                    session: "s".into(),
                    prompt_id: "acp-1".into(),
                    option_id: "allow-once".into(),
                },
                "acp_answer",
            ),
            (
                ClientMessage::AcpMode {
                    session: "s".into(),
                    mode: "plan".into(),
                },
                "acp_mode",
            ),
        ] {
            assert_eq!(serde_json::to_value(msg).unwrap()["type"], name);
        }
        let view = ServerMessage::AcpView(AcpView {
            session: "s".into(),
            epoch: 7,
            rev: 3,
            reset: true,
            first: 1,
            entries: vec![
                AcpEntry {
                    id: 1,
                    rev: 1,
                    body: AcpBody::Message {
                        text: "hi".into(),
                        truncated: false,
                    },
                },
                AcpEntry {
                    id: 2,
                    rev: 3,
                    body: AcpBody::Tool {
                        tool_id: "t".into(),
                        title: "Write a.txt".into(),
                        tool_kind: "edit".into(),
                        status: "in_progress".into(),
                        diffs: vec![AcpDiff {
                            path: "a.txt".into(),
                            old_text: None,
                            new_text: "hi\n".into(),
                            truncated: false,
                        }],
                        locations: vec![],
                    },
                },
            ],
            info: None,
        });
        let v = serde_json::to_value(&view).unwrap();
        assert_eq!(v["type"], "acp_view");
        assert_eq!(v["entries"][0]["kind"], "message");
        assert_eq!(v["entries"][1]["tool_kind"], "edit");
        assert!(v["entries"][1]["diffs"][0].get("old_text").is_none());
        assert!(v.get("info").is_none());
        assert_eq!(serde_json::from_value::<ServerMessage>(v).unwrap(), view);
        // An entry kind from a newer server is read as unknown, not fatal.
        let e: AcpEntry =
            serde_json::from_str(r#"{"id":4,"rev":4,"kind":"hologram","x":1}"#).unwrap();
        assert_eq!(e.body, AcpBody::Unknown);
    }

    /// The hook messages are additive: an answer without a preview or an error reads fine, and the reply keeps its
    /// optional fields off the wire when empty (U10).
    #[test]
    fn agent_hooks_messages_round_trip_and_leave_empty_fields_out() {
        let m: ClientMessage =
            serde_json::from_str(r#"{"type":"agent_hooks","agent":"claude","action":"apply"}"#)
                .unwrap();
        assert_eq!(
            m,
            ClientMessage::AgentHooks {
                agent: "claude".into(),
                action: "apply".into()
            }
        );
        let reply = ServerMessage::AgentHooks {
            agent: "claude".into(),
            action: "status".into(),
            state: "connected".into(),
            preview: None,
            error: None,
        };
        let text = serde_json::to_string(&reply).unwrap();
        assert!(!text.contains("preview") && !text.contains("error"));
        assert_eq!(serde_json::from_str::<ServerMessage>(&text).unwrap(), reply);
    }
}
