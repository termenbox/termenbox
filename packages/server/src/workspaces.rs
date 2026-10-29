use std::time::Duration;

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use tracing::{debug, info};

use termenbox_core::protocol::{ClientMessage, ServerMessage, WindowInfo};

use std::sync::Arc;

use crate::backend::{RunMode, WorkspaceBackend};
use crate::control::{WindowHub, WindowState, WindowUpdate};
use crate::events::EventHub;
use crate::health::HealthCollector;
use crate::link::{Inbound, Link, LinkTx};
use crate::pty::Attachment;
use crate::registry::Registry;

/// Clients ping every 5 s, so a minute of silence means the peer is gone.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// A streaming reply changes many times a second; a client that shows it gets one update per this long at most.
/// How often a connection looks at `update-status.json` to tell its client when the recorded release changed.
const UPDATE_POLL: Duration = Duration::from_secs(20);
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(600);
const UPDATE_DEADLINE: Duration = Duration::from_secs(600);

#[derive(Default)]
struct UpdateGate {
    checked: Option<std::time::Instant>,
    pending: Option<(String, std::time::Instant)>,
}
const ACP_BATCH: Duration = Duration::from_millis(120);
/// The longest prompt a client may send (the backend caps it again).
const ACP_PROMPT_BYTES: usize = 16 * 1024;

/// The ACP workspace one connection shows (T0): what it holds, and whether an update is waiting to go out.
struct AcpSub {
    workspace: String,
    rx: tokio::sync::watch::Receiver<u64>,
    rev: u64,
    epoch: u64,
    info_rev: u64,
    due: bool,
    /// The stream's sender is gone: no more updates will come.
    dead: bool,
}

/// The conversation log one connection follows (step 3.4): where it is in the log, and whether news is waiting.
struct EventSub {
    workspace: String,
    rx: tokio::sync::watch::Receiver<u64>,
    after: u64,
    epoch: u64,
    due: bool,
    dead: bool,
}

/// Frames of one `acp_events` message stay under this many bytes of JSON (the direct transport carries 64 KiB).
const EVENTS_MESSAGE_BYTES: usize = 40 * 1024;

/// A replay as messages of bounded size; `reset` is set on the first only. A replay with no frames still makes one
/// message when it is a reset or `always` (the client learns `last`).
fn events_messages(
    workspace: &str,
    r: crate::event_log::Replay,
    always: bool,
) -> Vec<ServerMessage> {
    let mut groups: Vec<Vec<serde_json::Value>> = vec![Vec::new()];
    let mut size = 0;
    for f in r.frames {
        let n = f.to_string().len();
        if size + n > EVENTS_MESSAGE_BYTES && !groups.last().is_some_and(Vec::is_empty) {
            groups.push(Vec::new());
            size = 0;
        }
        size += n;
        if let Some(g) = groups.last_mut() {
            g.push(f);
        }
    }
    if groups.iter().all(Vec::is_empty) && !r.reset && !always {
        return Vec::new();
    }
    groups
        .into_iter()
        .enumerate()
        .map(|(i, frames)| ServerMessage::AcpEvents {
            workspace: workspace.to_string(),
            epoch: r.epoch,
            last: r.last,
            reset: r.reset && i == 0,
            frames,
        })
        .collect()
}

/// Serves one authenticated WebSocket: lists/creates tmux workspaces and bridges one attached workspace.
/// Persistence lives in the workspace backend (tmux by default); a dropped connection only detaches (see pty.rs).
#[derive(Clone)]
pub struct WorkspaceManager {
    backend: Arc<dyn WorkspaceBackend>,
    registry: Arc<Registry>,
    mode: RunMode,
    server_name: String,
    /// Called after an uninstall has wiped the configuration and told the client; stops the process.
    on_uninstalled: Arc<dyn Fn() + Send + Sync>,
    /// Where requests for the root helper go and where the updater's status sits beside them (decision 0077); None
    /// in tests and builds without an updater.
    requests_dir: Option<std::path::PathBuf>,
    update_gate: Arc<std::sync::Mutex<UpdateGate>>,
    events: Option<EventHub>,
    push: Option<Arc<crate::push::Push>>,
    windows: Option<WindowHub>,
    /// tmux version text and whether control mode runs (plan H.3); reported in `auth_ok`.
    tmux_version: String,
    control_mode: bool,
    prefix: String,
    /// Bounded readers of the transcripts hooks named (tier 1, decision 0103).
    transcripts: Arc<crate::transcript::Transcripts>,
    /// Where the owner can connect each agent's hook surface from the app (U10).
    hooks: Option<crate::agent_hooks::Targets>,
    /// Samples of the server's CPU, RAM and disk (U14).
    health: Option<Arc<HealthCollector>>,
    /// Set when the last run of this server ended without a clean exit; every sign-in is told (decision 0142).
    previous_exit: Option<termenbox_core::protocol::ServerMessage>,
    /// Pinged (with the connection's number) when a connection creates, renames or closes a workspace, so every other
    /// connection sends its client the new list (a device that did not ask must not keep showing the old names). The
    /// connection that asked has its own reply and skips the ping.
    list_changed: tokio::sync::broadcast::Sender<u64>,
}

impl WorkspaceManager {
    pub fn new(backend: Arc<dyn WorkspaceBackend>, mode: RunMode, registry: Arc<Registry>) -> Self {
        Self {
            backend,
            registry,
            mode,
            server_name: String::new(),
            on_uninstalled: Arc::new(|| {}),
            requests_dir: None,
            update_gate: Arc::default(),
            events: None,
            push: None,
            windows: None,
            tmux_version: String::new(),
            control_mode: false,
            prefix: String::new(),
            transcripts: Arc::default(),
            hooks: None,
            health: None,
            previous_exit: None,
            list_changed: tokio::sync::broadcast::channel(16).0,
        }
    }

    /// The transcript timeline for a workspace whose hooks named one (tier 1). The path is the hook's, so it is
    /// opened only if it resolves under the user's Claude config directory; the read is bounded and runs off the
    /// async threads. Nothing from it is logged.
    async fn timeline(&self, workspace: &str) -> Option<termenbox_core::protocol::Timeline> {
        let path = self.events.as_ref()?.transcript(workspace)?;
        let root = crate::transcript::claude_root()?;
        let readers = self.transcripts.clone();
        let workspace = workspace.to_string();
        tokio::task::spawn_blocking(move || readers.timeline(&root, &workspace, &path))
            .await
            .ok()
            .flatten()
    }

    /// Lets the owner enable and remove Claude Code's hooks from the app (U10); without it the state is `unavailable`.
    pub fn with_agent_hooks(mut self, targets: crate::agent_hooks::Targets) -> Self {
        self.hooks = Some(targets);
        self
    }

    /// The workspace managers a client may pick from when it creates a workspace.
    pub fn backends(&self) -> Vec<String> {
        self.backend.kinds()
    }

    /// The agents the owner allows as ACP workspaces (decision 0111); empty when none are configured.
    pub fn acp_agents(&self) -> Vec<String> {
        self.backend.acp_agents()
    }

    /// Lets the owner update the server from the app: `dir` is the requests directory the root helper watches.
    pub fn with_requests(mut self, dir: std::path::PathBuf) -> Self {
        if let Some((id, age)) = crate::requests::pending_update(&dir) {
            let now = std::time::Instant::now();
            self.update_gate.lock().unwrap().pending = Some((id, now.checked_sub(age).unwrap_or(now)));
        }
        self.requests_dir = Some(dir);
        self
    }

    /// The newer release the updater recorded, empty when there is none (reported in `auth_ok`).
    pub fn update_available(&self) -> String {
        self.requests_dir
            .as_deref()
            .and_then(|dir| crate::requests::available_update(dir, &crate::version()))
            .unwrap_or_default()
    }

    fn check_update(&self) {
        let Some(dir) = &self.requests_dir else {
            return;
        };
        let mut gate = self.update_gate.lock().unwrap();
        if gate
            .pending
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < UPDATE_DEADLINE)
            || gate
                .checked
                .is_some_and(|at| at.elapsed() < UPDATE_CHECK_EVERY)
        {
            return;
        }
        // The shared gate is held through publishing the tiny request, never through a network call.
        if crate::requests::leave_update_check(dir).is_ok() {
            gate.checked = Some(std::time::Instant::now());
        }
    }

    fn request_update(&self, dir: &std::path::Path) -> Result<String> {
        let mut gate = self.update_gate.lock().unwrap();
        if let Some((id, at)) = &gate.pending {
            if at.elapsed() < UPDATE_DEADLINE && crate::requests::update_result(dir, id).is_none() {
                return Ok(id.clone());
            }
        }
        let id = crate::requests::leave_update_now(dir)?;
        gate.pending = Some((id.clone(), std::time::Instant::now()));
        Ok(id)
    }

    /// Serves the agent event stream (plan v6) to every connection.
    pub fn with_events(mut self, hub: EventHub) -> Self {
        self.events = Some(hub);
        self
    }

    /// Serves the server's health samples to every connection (U14).
    pub fn with_health(mut self, health: Arc<HealthCollector>) -> Self {
        self.health = Some(health);
        self
    }

    /// Tells every signed-in client that the last run ended without a clean exit.
    pub fn with_previous_exit(
        mut self,
        message: Option<termenbox_core::protocol::ServerMessage>,
    ) -> Self {
        self.previous_exit = message;
        self
    }

    /// Lets devices subscribe to Web Push and tell which workspace they show (plan A.6).
    pub fn with_push(mut self, push: Arc<crate::push::Push>) -> Self {
        self.push = Some(push);
        self
    }

    /// Serves the tmux window lists kept current by control mode (plan H.3).
    pub fn with_windows(mut self, hub: WindowHub, tmux_version: String) -> Self {
        self.windows = Some(hub);
        self.control_mode = true;
        self.tmux_version = tmux_version;
        self
    }

    /// The tmux version seen at startup and why control mode may be off (an old tmux still gets the scan).
    pub fn with_tmux_version(mut self, tmux_version: String) -> Self {
        self.tmux_version = tmux_version;
        self
    }

    /// tmux's prefix key as tmux writes it (`C-b`), for the key row.
    pub fn with_prefix(mut self, prefix: String) -> Self {
        self.prefix = prefix;
        self
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn control_mode(&self) -> bool {
        self.control_mode
    }

    pub fn tmux_version(&self) -> &str {
        &self.tmux_version
    }

    /// Returns the push instance if configured.
    pub fn push(&self) -> Option<&Arc<crate::push::Push>> {
        self.push.as_ref()
    }

    /// Enables remote uninstall: `server_name` is what the owner must type, `on_uninstalled` stops the server.
    pub fn with_uninstall(
        mut self,
        server_name: String,
        on_uninstalled: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        self.server_name = server_name;
        self.on_uninstalled = on_uninstalled;
        self
    }

    /// Serves one authenticated connection until it ends or goes silent.
    pub async fn serve(
        &self,
        link: Link,
        identity: crate::auth::Identity,
        auth: &crate::auth::AuthState,
    ) -> Result<()> {
        static NEXT_CONN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let conn = NEXT_CONN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let res = self.serve_conn(conn, link, identity, auth).await;
        if let Some(p) = &self.push {
            p.set_foreground(conn, None);
        }
        res
    }

    async fn serve_conn(
        &self,
        conn: u64,
        mut link: Link,
        identity: crate::auth::Identity,
        auth: &crate::auth::AuthState,
    ) -> Result<()> {
        let mut attachment: Option<Attachment> = None;
        let (mut cols, mut rows) = (80u16, 24u16);
        let idle = tokio::time::sleep(IDLE_TIMEOUT);
        tokio::pin!(idle);
        let mut agent_rx = self.events.as_ref().map(|h| h.subscribe());
        let tab_status = self.events.as_ref().and_then(|h| h.tab_status());
        let mut tab_rev = 0;
        let mut tab_tick = tokio::time::interval(std::time::Duration::from_millis(500));
        tab_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        if let Some(status) = &tab_status {
            tab_rev = status.rev();
            if !send(
                &link.tx,
                &ServerMessage::TabStates {
                    states: status.snapshot(),
                },
            )
            .await
            {
                return Ok(());
            }
        }
        let mut window_rx = self.windows.as_ref().map(|h| h.subscribe());
        let mut status_rx = self.events.as_ref().map(|h| h.subscribe_status());
        let mut list_rx = self.list_changed.subscribe();
        let mut acp: Option<AcpSub> = None;
        let mut events: Option<EventSub> = None;
        let mut acp_tick = tokio::time::interval(ACP_BATCH);
        acp_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut event_tick = tokio::time::interval(ACP_BATCH);
        event_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut update_tick = tokio::time::interval(UPDATE_POLL);
        update_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut update_sent = self.update_available();
        let mut update_pending = if auth
            .owner()
            .is_some_and(|o| o.provider == identity.provider && o.id == identity.id)
        {
            self.update_gate.lock().unwrap().pending.clone()
        } else {
            None
        };
        // None once the collector is gone: the arm below then waits for ever instead of spinning on a closed channel.
        let mut health_rx = self.health.as_ref().map(|h| h.subscribe());
        if let Some(u) = self.health.as_ref().and_then(|h| h.latest()) {
            if !send(&link.tx, &u.message()).await {
                return Ok(());
            }
        }
        if let Some(previous) = &self.previous_exit {
            if !send(&link.tx, previous).await {
                return Ok(());
            }
        }

        loop {
            tokio::select! {
                msg = link.rx.recv() => {
                    let Some(msg) = msg.map(|m| link.tx.inbound(m)) else { break };
                    idle.as_mut().reset(tokio::time::Instant::now() + IDLE_TIMEOUT);
                    let text = match msg {
                        Inbound::Json(text) => text,
                        Inbound::Term(bytes) => {
                            if let Some(att) = attachment.as_mut() {
                                if let Err(e) = att.write(&bytes) {
                                    if !send(&link.tx, &ServerMessage::Error { message: e.to_string() }).await { break; }
                                }
                            }
                            continue;
                        }
                    };
                    // A control message may carry `req`: the answer is a `control_ack` on the control channel.
                    let req = serde_json::from_str::<ReqId>(&text).ok().and_then(|r| r.req);
                    let parsed = serde_json::from_str::<ClientMessage>(&text).ok().filter(|m| *m != ClientMessage::Unknown);
                    let Some(client_msg) = parsed else {
                        if let Some(req) = req {
                            if !send_ack(&link.tx, req, Err("Unknown message".into())).await { break; }
                        }
                        continue;
                    };
                    if let (true, ClientMessage::Attach { workspace_id, .. }) = (self.mode.is_host(), &client_msg) {
                        // Who got a shell on the host and as whom; never what they typed.
                        info!(target: "audit", event = "host_session", provider = %identity.provider, account_id = %identity.id, login = %identity.login, run_as = %self.mode.label(), session = %workspace_id);
                    }
                    let updating = matches!(&client_msg, ClientMessage::UpdateServer {});
                    let result = self.handle(conn, client_msg, auth, &identity, &mut attachment, &mut cols, &mut rows, &mut acp, &mut events, &link.tx).await;
                    if updating && result.is_ok() { update_pending = self.update_gate.lock().unwrap().pending.clone(); }
                    match (req, result) {
                        (Some(req), result) => {
                            if !send_ack(&link.tx, req, result.map_err(|e| e.to_string())).await { break; }
                        }
                        (None, Err(e)) => {
                            if !send(&link.tx, &ServerMessage::Error { message: e.to_string() }).await { break; }
                        }
                        (None, Ok(())) => {}
                    }
                }
                // Room first, then the bytes: a client that reads slowly stops the terminal, not the status messages.
                out = async {
                    let permit = link.tx.reserve_output().await;
                    (permit, next_output(&mut attachment).await)
                }, if attachment.is_some() => {
                    let (permit, out) = out;
                    match out {
                        Some(bytes) => {
                            let Some(permit) = permit else { break };
                            permit.send(bytes);
                        }
                        None => {
                            // The attach client exited: workspace killed, or detached from inside.
                            attachment = None;
                            if !send(&link.tx, &ServerMessage::Error { message: "Workspace ended".into() }).await { break; }
                            let workspaces = self.registry.list(self.backend.as_ref()).await.unwrap_or_default();
                            if !send(&link.tx, &ServerMessage::Workspaces { workspaces }).await { break; }
                        }
                    }
                }
                batch = next_events(&mut agent_rx) => {
                    let msg = match batch {
                        Some(events) => ServerMessage::AgentEvents { events },
                        // Too slow to keep up: the client resyncs from a snapshot.
                        None => ServerMessage::AgentSnapshot { states: self.events.as_ref().map(|h| h.snapshot()).unwrap_or_default() },
                    };
                    if !send(&link.tx, &msg).await { break; }
                }
                change = next_status(&mut status_rx) => {
                    let msgs = match change {
                        Some(c) => vec![status_message(c)],
                        // Fell behind: send the whole picture again.
                        None => self.status_messages(),
                    };
                    for m in msgs {
                        if !send(&link.tx, &m).await { return Ok(()); }
                    }
                }
                _ = tab_tick.tick(), if tab_status.is_some() => {
                    if let Some(status) = &tab_status {
                        let rev = status.rev();
                        if rev != tab_rev {
                            tab_rev = rev;
                            if !send(&link.tx, &ServerMessage::TabStates { states: status.snapshot() }).await { break; }
                        }
                    }
                }
                from = next_list_change(&mut list_rx) => {
                    // Another connection changed the list of workspaces: this one's client gets the new one.
                    if from == Some(conn) { continue; }
                    let workspaces = self.registry.list(self.backend.as_ref()).await.unwrap_or_default();
                    if !send(&link.tx, &ServerMessage::Workspaces { workspaces }).await { break; }
                }
                update = next_window(&mut window_rx) => {
                    let msg = match update {
                        Some(u) => self.window_update_message(&u),
                        None => {
                            // Fell behind: send everything again.
                            for u in self.windows.as_ref().map(|h| h.all()).unwrap_or_default() {
                                if !send(&link.tx, &self.window_update_message(&u)).await { return Ok(()); }
                            }
                            continue;
                        }
                    };
                    if !send(&link.tx, &msg).await { break; }
                }
                woke = next_event_change(&mut events) => {
                    if let Some(sub) = events.as_mut() {
                        if woke { sub.due = true } else { sub.dead = true }
                    }
                }
                _ = event_tick.tick(), if events.as_ref().is_some_and(|e| e.due) => {
                    let mut gone = false;
                    for msg in self.event_update(&mut events) {
                        if !send(&link.tx, &msg).await { gone = true; break; }
                    }
                    if gone { break; }
                }
                woke = next_acp_change(&mut acp) => {
                    if let Some(sub) = acp.as_mut() {
                        if woke { sub.due = true } else { sub.dead = true }
                    }
                }
                _ = acp_tick.tick(), if acp.as_ref().is_some_and(|a| a.due) => {
                    let mut gone = false;
                    for msg in self.acp_update(&mut acp) {
                        if !send(&link.tx, &msg).await { gone = true; break; }
                    }
                    if gone { break; }
                }
                update = async {
                    match &mut health_rx {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match update {
                        Ok(u) => { if !send(&link.tx, &u.message()).await { break; } }
                        // Behind by a few samples: the next one carries the same story.
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => health_rx = None,
                    }
                }
                _ = update_tick.tick() => {
                    if auth.owner().is_some_and(|o| o.provider == identity.provider && o.id == identity.id) { self.check_update(); }
                    if let (Some(dir), Some((id, at))) = (&self.requests_dir, &update_pending) {
                        let result = crate::requests::update_result(dir, id);
                        let done = result.is_some() || at.elapsed() >= UPDATE_DEADLINE;
                        let error = result.and_then(|r| r.error).or_else(|| {
                            (at.elapsed() >= UPDATE_DEADLINE).then(|| "The helper did not report an update result in time. Check its service logs.".to_string())
                        });
                        if let Some(error) = error {
                            if !send(&link.tx, &ServerMessage::Error { message: format!("Server update: {error}") }).await { break; }
                        }
                        if done { update_pending = None; }
                    }
                    if let Some(msg) = update_change(&mut update_sent, self.update_available()) {
                        if !send(&link.tx, &msg).await { break; }
                    }
                }
                _ = &mut idle => {
                    info!("Closing idle connection of {}", identity.login);
                    break;
                }
            }
        }

        info!("Connection closed for {}", identity.login);
        Ok(())
    }

    /// The `acp_events` messages the connection owes its client: the frames since the last it sent, or a compacted
    /// replay if the log no longer holds them.
    fn event_update(&self, events: &mut Option<EventSub>) -> Vec<ServerMessage> {
        let Some(sub) = events.as_mut() else {
            return Vec::new();
        };
        sub.due = false;
        match self
            .backend
            .acp_events(&sub.workspace, Some(sub.after), Some(sub.epoch))
        {
            Ok(replay) => {
                sub.after = replay.last;
                sub.epoch = replay.epoch;
                events_messages(&sub.workspace, replay, false)
            }
            // The workspace was closed: the client learns it from the workspace list.
            Err(_) => {
                *events = None;
                Vec::new()
            }
        }
    }

    /// The `acp_view` messages the connection owes its client, if anything changed since the last update. A view is cut
    /// into messages the direct transport can carry (`acp_stream::split`).
    fn acp_update(&self, acp: &mut Option<AcpSub>) -> Vec<ServerMessage> {
        let Some(sub) = acp.as_mut() else {
            return Vec::new();
        };
        sub.due = false;
        match self
            .backend
            .acp_view(&sub.workspace, Some(sub.rev), Some(sub.epoch), sub.info_rev)
        {
            Ok((mut view, info_rev)) => {
                if view.reset
                    || !view.entries.is_empty()
                    || view.info.is_some()
                    || view.rev != sub.rev
                {
                    view.workspace = sub.workspace.clone();
                    (sub.rev, sub.epoch, sub.info_rev) = (view.rev, view.epoch, info_rev);
                    crate::acp_stream::split(view)
                        .into_iter()
                        .map(ServerMessage::AcpView)
                        .collect()
                } else {
                    Vec::new()
                }
            }
            // The workspace was closed: the client learns it from the workspace list.
            Err(_) => {
                *acp = None;
                Vec::new()
            }
        }
    }

    /// Every workspace's Activity and the open Attention items, for a client that is new or fell behind.
    fn status_messages(&self) -> Vec<ServerMessage> {
        let Some(hub) = &self.events else {
            return Vec::new();
        };
        vec![
            ServerMessage::Activity {
                workspaces: hub.activities(),
            },
            ServerMessage::Attention {
                items: hub.attention_items(),
            },
        ]
    }

    /// One tab by its position in the server's order, for an old client's window message.
    fn tab_at(&self, workspace: &str, index: u32) -> Option<String> {
        self.registry
            .tabs(workspace)
            .get(index as usize)
            .map(|t| t.id.clone())
    }

    /// The `window_list` an old client reads: the registry's tabs in order, with the live command and path of the
    /// tmux window each one carries. A workspace with no record falls back to the live list.
    fn window_list_for(&self, workspace: &str, live: &[WindowState]) -> ServerMessage {
        let Some(record) = self.registry.record(workspace) else {
            return ServerMessage::WindowList {
                workspace: workspace.to_string(),
                representative: crate::control::representative(live),
                windows: live.iter().map(|w| w.info.clone()).collect(),
            };
        };
        let mut tabs = record.tabs.clone();
        tabs.sort_by_key(|t| t.order);
        let focus = record.focus.clone();
        let windows: Vec<WindowInfo> = tabs
            .iter()
            .map(|t| {
                let observed = t
                    .window
                    .as_deref()
                    .and_then(|id| live.iter().find(|w| w.window_id == id));
                WindowInfo {
                    index: t.order,
                    name: t.label.as_str().to_string(),
                    command: observed
                        .map(|w| w.info.command.clone())
                        .unwrap_or_else(|| t.kind.clone()),
                    path: observed
                        .map(|w| w.info.path.clone())
                        .unwrap_or_else(|| tab_path(&record.dir, &t.dir)),
                    active: focus.as_deref() == Some(t.id.as_str()),
                    agent: (t.kind != "shell").then(|| t.kind.clone()),
                }
            })
            .collect();
        ServerMessage::WindowList {
            workspace: workspace.to_string(),
            representative: tabs
                .iter()
                .position(|t| focus.as_deref() == Some(t.id.as_str()))
                .map(|i| i as u32)
                .or_else(|| windows.first().map(|w| w.index)),
            windows,
        }
    }

    /// A control-mode update mapped through the registry, so the old window menu keeps the server's order.
    fn window_update_message(&self, update: &WindowUpdate) -> ServerMessage {
        let Some(record) = self.registry.record(&update.workspace) else {
            return window_message(update.clone());
        };
        let mut tabs = record.tabs.clone();
        tabs.sort_by_key(|t| t.order);
        let focus = record.focus.clone();
        let windows: Vec<WindowInfo> = tabs
            .iter()
            .map(|t| {
                let at = t
                    .window
                    .as_deref()
                    .and_then(|id| update.window_ids.iter().position(|w| w == id));
                let observed = at.and_then(|i| update.windows.get(i));
                WindowInfo {
                    index: t.order,
                    name: t.label.as_str().to_string(),
                    command: observed
                        .map(|w| w.command.clone())
                        .unwrap_or_else(|| t.kind.clone()),
                    path: observed
                        .map(|w| w.path.clone())
                        .unwrap_or_else(|| tab_path(&record.dir, &t.dir)),
                    active: focus.as_deref() == Some(t.id.as_str()),
                    agent: (t.kind != "shell").then(|| t.kind.clone()),
                }
            })
            .collect();
        ServerMessage::WindowList {
            workspace: update.workspace.clone(),
            representative: tabs
                .iter()
                .position(|t| focus.as_deref() == Some(t.id.as_str()))
                .map(|i| i as u32)
                .or_else(|| windows.first().map(|w| w.index)),
            windows,
        }
    }

    #[allow(clippy::too_many_arguments)] // one connection's state, passed explicitly
    async fn handle(
        &self,
        conn: u64,
        msg: ClientMessage,
        auth: &crate::auth::AuthState,
        identity: &crate::auth::Identity,
        attachment: &mut Option<Attachment>,
        cols: &mut u16,
        rows: &mut u16,
        acp: &mut Option<AcpSub>,
        events: &mut Option<EventSub>,
        tx: &LinkTx,
    ) -> Result<()> {
        match msg {
            // The handshake happens once, before this loop (connection.rs).
            ClientMessage::Hello { .. }
            | ClientMessage::Auth { .. }
            | ClientMessage::ClaimPreview { .. } => {}
            ClientMessage::Uninstall { confirm_name, .. } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if let Err(reason) = crate::uninstall::authorize(
                    &self.server_name,
                    &confirm_name,
                    identity.issued_at,
                    now,
                ) {
                    info!(target: "audit", event = "uninstall_refused", provider = %identity.provider, account_id = %identity.id, reason);
                    return Err(anyhow::anyhow!(reason));
                }
                info!(target: "audit", event = "uninstall_started", provider = %identity.provider, account_id = %identity.id, login = %identity.login, mode = %self.mode.label());
                *attachment = None;
                crate::uninstall::wipe(auth.dir())?;
                info!(target: "audit", event = "uninstall_config_wiped");
                send_r(
                    tx,
                    &ServerMessage::UninstallDone {
                        mode: self.mode.label(),
                    },
                )
                .await?;
                (self.on_uninstalled)();
            }
            ClientMessage::UpdateServer {} => {
                let is_owner = auth
                    .owner()
                    .is_some_and(|o| o.provider == identity.provider && o.id == identity.id);
                let Some(dir) = self.requests_dir.as_deref().filter(|_| is_owner) else {
                    info!(target: "audit", event = "update_refused", provider = %identity.provider, account_id = %identity.id);
                    return Err(anyhow::anyhow!("Server update: Only the owner can update this server"));
                };
                self.request_update(dir).map_err(|_| {
                    anyhow::anyhow!("Server update: Could not contact the update helper.")
                })?;
                info!(target: "audit", event = "update_requested", provider = %identity.provider, account_id = %identity.id, login = %identity.login);
                send_r(
                    tx,
                    &ServerMessage::UpdateRequested {
                        available: self.update_available(),
                    },
                )
                .await?;
            }
            ClientMessage::DiagnosticsBundle {} => {
                let is_owner = auth
                    .owner()
                    .is_some_and(|o| o.provider == identity.provider && o.id == identity.id);
                if !is_owner {
                    info!(target: "audit", event = "diagnostics_refused", provider = %identity.provider, account_id = %identity.id);
                    return Err(anyhow::anyhow!(
                        "Only the owner can read this server's diagnostics"
                    ));
                }
                info!(target: "audit", event = "diagnostics_requested", provider = %identity.provider, account_id = %identity.id);
                let config_dir = auth.dir().to_path_buf();
                let mode = self.mode.label();
                let previous = self.previous_exit.clone();
                let health = self.health.as_ref().map(|h| h.history());
                // Names a user chose are in no field name or token shape: the bundle scrubs the live ones from free text.
                let names: Vec<String> = self
                    .backend
                    .list()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .flat_map(|w| [w.id, w.name.as_str().to_string()])
                    // A minted id is random; only an id an older server took from the typed name is text.
                    .filter(|n| !crate::backend::is_minted_id(n))
                    .collect();
                // Reads up to 2 MB of log and scrubs it: off the async threads.
                let text = tokio::task::spawn_blocking(move || {
                    crate::bundle::build(&crate::bundle::Inputs {
                        config_dir: &config_dir,
                        version: env!("CARGO_PKG_VERSION"),
                        mode: &mode,
                        previous_exit: previous.as_ref(),
                        health,
                        names: &names,
                    })
                })
                .await?;
                let parts = crate::bundle::parts(&text);
                let total = parts.len() as u32;
                for (part, text) in parts.into_iter().enumerate() {
                    send_r(
                        tx,
                        &ServerMessage::DiagnosticsBundle {
                            part: part as u32,
                            parts: total,
                            text,
                        },
                    )
                    .await?;
                }
            }
            ClientMessage::RevokeDevice { device } => {
                auth.revoke_device(&device)?;
                send_r(tx, &ServerMessage::DeviceRevoked { device }).await?;
            }
            ClientMessage::Ping => send_control_r(tx, &ServerMessage::Pong).await?,
            ClientMessage::PushInfo => {
                let (configured, public_key, devices) = match &self.push {
                    Some(p) => (
                        p.configured(),
                        p.public_key().unwrap_or_default(),
                        p.subscription_count() as u32,
                    ),
                    None => (false, String::new(), 0),
                };
                send_r(
                    tx,
                    &ServerMessage::PushInfo {
                        configured,
                        public_key,
                        devices,
                        subscribed: false,
                        settings: configured,
                    },
                )
                .await?;
            }
            ClientMessage::PushSubscribe {
                endpoint,
                p256dh,
                auth,
            } => {
                let Some(p) = &self.push else {
                    return Err(anyhow::anyhow!("Push is not set up on this server"));
                };
                p.subscribe(crate::push::Subscription {
                    endpoint: endpoint.clone(),
                    p256dh,
                    auth,
                })?;
                send_r(
                    tx,
                    &ServerMessage::PushInfo {
                        configured: true,
                        public_key: p.public_key().unwrap_or_default(),
                        devices: p.subscription_count() as u32,
                        subscribed: p.has(&endpoint),
                        settings: true,
                    },
                )
                .await?;
            }
            ClientMessage::PushUnsubscribe { endpoint } => {
                if let Some(p) = &self.push {
                    p.unsubscribe(&endpoint);
                    send_r(
                        tx,
                        &ServerMessage::PushInfo {
                            configured: p.configured(),
                            public_key: p.public_key().unwrap_or_default(),
                            devices: p.subscription_count() as u32,
                            subscribed: false,
                            settings: p.configured(),
                        },
                    )
                    .await?;
                }
            }
            ClientMessage::PushSettingsGet => {
                if let Some(p) = &self.push {
                    let settings = p.get_config();
                    send_r(tx, &ServerMessage::PushSettings { settings }).await?;
                } else {
                    send_r(
                        tx,
                        &ServerMessage::PushSettingsRejected {
                            reason: "Push is not configured on this server".into(),
                        },
                    )
                    .await?;
                }
            }
            ClientMessage::PushSettingsSet { settings } => {
                if let Some(p) = &self.push {
                    if let Err(e) = settings.validate() {
                        send_r(tx, &ServerMessage::PushSettingsRejected { reason: e }).await?;
                    } else {
                        p.set_config(settings.clone());
                        send_r(tx, &ServerMessage::PushSettings { settings }).await?;
                    }
                } else {
                    send_r(
                        tx,
                        &ServerMessage::PushSettingsRejected {
                            reason: "Push is not configured on this server".into(),
                        },
                    )
                    .await?;
                }
            }
            ClientMessage::Foreground { workspace, tab } => {
                if let Some(p) = &self.push {
                    p.set_foreground_tab(conn, workspace, tab);
                }
            }
            ClientMessage::Ack { attention_id } | ClientMessage::Dismiss { attention_id } => {
                if let Some(hub) = &self.events {
                    hub.ack(&attention_id);
                }
            }
            ClientMessage::Answer {
                workspace,
                tab,
                prompt_id,
                seq,
                choice,
            } => {
                let hub = self
                    .events
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Answers are not available"))?;
                let window = if self.registry.has(&workspace) {
                    self.registry
                        .tab_window(self.backend.as_ref(), &workspace, tab.as_deref())
                        .await?
                } else {
                    None
                };
                answer_tab(
                    self.backend.as_ref(),
                    hub,
                    &workspace,
                    window.as_deref(),
                    tab.as_deref(),
                    &prompt_id,
                    seq,
                    choice,
                )
                .await?;
                info!(target: "audit", event = "answer_sent", provider = %identity.provider, account_id = %identity.id, session = %workspace, choice);
            }
            ClientMessage::Reply { workspace, text } => {
                // One line, no control characters: it is typed, then Enter.
                let line: String = text.chars().filter(|c| !c.is_control()).take(500).collect();
                if line.trim().is_empty() {
                    return Ok(());
                }
                self.backend
                    .send_text(&workspace, &format!("{line}\r"))
                    .await?;
                info!(target: "audit", event = "reply_sent", provider = %identity.provider, account_id = %identity.id, session = %workspace);
            }
            ClientMessage::AgentSync { after } => {
                let Some(hub) = &self.events else {
                    return Ok(());
                };
                let states = hub.snapshot();
                let mut events = Vec::new();
                for st in &states {
                    if let Some(seq) = after.get(&st.workspace) {
                        events.extend(hub.since(&st.workspace, *seq));
                    }
                }
                send_r(tx, &ServerMessage::AgentSnapshot { states }).await?;
                for m in self.status_messages() {
                    send_r(tx, &m).await?;
                }
                if !events.is_empty() {
                    send_r(tx, &ServerMessage::AgentEvents { events }).await?;
                }
                // A tab speaks for one window of its workspace: send them all along with the agent states.
                for u in self.windows.as_ref().map(|h| h.all()).unwrap_or_default() {
                    send_r(tx, &self.window_update_message(&u)).await?;
                }
            }
            ClientMessage::Windows { workspace } => {
                // Adopt a window the owner made in tmux directly before answering, so it is not invisible.
                let _ = self.registry.list(self.backend.as_ref()).await;
                let live = self.backend.windows(&workspace).await.unwrap_or_default();
                send_r(tx, &self.window_list_for(&workspace, &live)).await?;
            }
            ClientMessage::SelectWindow { workspace, index } => {
                if let Some(tab) = self.tab_at(&workspace, index) {
                    self.registry
                        .focus_tab(self.backend.as_ref(), &workspace, &tab)
                        .await?;
                } else {
                    self.backend.select_window(&workspace, index).await?;
                }
            }
            ClientMessage::NewWindow { workspace } => {
                info!(target: "audit", event = "new_window", provider = %identity.provider, account_id = %identity.id, session = %workspace);
                if self.registry.has(&workspace) {
                    self.registry
                        .new_tab(self.backend.as_ref(), &workspace, "shell", "", "")
                        .await?;
                    let workspaces = self.registry.list(self.backend.as_ref()).await?;
                    send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                    let _ = self.list_changed.send(conn);
                } else {
                    self.backend.new_window(&workspace).await?;
                }
            }
            ClientMessage::CloseWindow { workspace, index } => {
                info!(target: "audit", event = "close_window", provider = %identity.provider, account_id = %identity.id, session = %workspace, window = index);
                if let Some(tab) = self.tab_at(&workspace, index) {
                    self.registry
                        .close_tab(self.backend.as_ref(), &workspace, &tab)
                        .await?;
                    if let Some(events) = &self.events {
                        events.forget_tab(&workspace, &tab);
                    }
                } else {
                    self.backend.kill_window(&workspace, index).await?;
                }
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::CloseWorkspace { workspace } => {
                info!(target: "audit", event = "close_workspace", provider = %identity.provider, account_id = %identity.id, login = %identity.login, session = %workspace);
                // Drop this connection's tmux client first: a live client can keep kill-session waiting.
                *attachment = None;
                self.registry
                    .close_workspace(self.backend.as_ref(), &workspace)
                    .await?;
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::RenameWorkspace { workspace, name } => {
                info!(target: "audit", event = "rename_workspace", provider = %identity.provider, account_id = %identity.id, session = %workspace);
                self.registry
                    .rename_workspace(self.backend.as_ref(), &workspace, &name)
                    .await?;
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::NewTab {
                workspace,
                template,
                label,
                dir,
            } => {
                info!(target: "audit", event = "new_tab", provider = %identity.provider, account_id = %identity.id, session = %workspace);
                self.registry
                    .new_tab(self.backend.as_ref(), &workspace, &template, &label, &dir)
                    .await?;
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::RenameTab {
                workspace,
                tab,
                label,
            } => {
                info!(target: "audit", event = "rename_tab", provider = %identity.provider, account_id = %identity.id, session = %workspace, tab = %tab);
                self.registry
                    .rename_tab(self.backend.as_ref(), &workspace, &tab, &label)
                    .await?;
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::CloseTab { workspace, tab } => {
                info!(target: "audit", event = "close_tab", provider = %identity.provider, account_id = %identity.id, session = %workspace, tab = %tab);
                self.registry
                    .close_tab(self.backend.as_ref(), &workspace, &tab)
                    .await?;
                if let Some(events) = &self.events {
                    events.forget_tab(&workspace, &tab);
                }
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::MoveTab {
                workspace,
                tab,
                index,
            } => {
                self.registry.move_tab(&workspace, &tab, index).await?;
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::FocusTab { workspace, tab } => {
                self.registry
                    .focus_tab(self.backend.as_ref(), &workspace, &tab)
                    .await?;
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::MoveWorkspace { workspace, index } => {
                self.registry.move_workspace(&workspace, index).await?;
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::ListDir {
                path,
                prefix,
                hidden,
            } => {
                let is_owner = auth
                    .owner()
                    .is_some_and(|o| o.provider == identity.provider && o.id == identity.id);
                info!(target: "audit", event = "list_dir", provider = %identity.provider, account_id = %identity.id, owner = is_owner, path_hash = %crate::listdir::path_hash(path.as_deref()));
                let reply = crate::listdir::answer(
                    is_owner,
                    crate::listdir::server_home(),
                    path,
                    prefix,
                    hidden,
                )
                .await;
                send_r(tx, &reply).await?;
            }
            ClientMessage::Peek {
                workspace,
                tab,
                hash,
            } => {
                // Only a counter in the audit log: what is on the screen is never logged.
                audit_peek(&identity.provider, &identity.id, &workspace);
                let reply = if self.registry.has(&workspace) {
                    let window = self
                        .registry
                        .tab_window(self.backend.as_ref(), &workspace, tab.as_deref())
                        .await?;
                    peek_tab(
                        self.backend.as_ref(),
                        &workspace,
                        hash.as_deref(),
                        window.as_deref(),
                    )
                    .await
                } else {
                    let window = self
                        .windows
                        .as_ref()
                        .and_then(|h| h.representative(&workspace));
                    peek_window(self.backend.as_ref(), &workspace, hash.as_deref(), window).await
                };
                send_r(tx, &reply).await?;
            }
            ClientMessage::Scrollback { workspace } => {
                // Only a counter in the audit log, as for a peek: the terminal's text is never logged.
                audit_peek(&identity.provider, &identity.id, &workspace);
                let text = self.backend.capture_history(&workspace).await?;
                for part in scrollback_parts(&workspace, &text) {
                    send_r(tx, &part).await?;
                }
            }
            ClientMessage::SessionView { workspace, hash } => {
                // Only a counter in the audit log, as for a peek: the screen is never logged.
                audit_peek(&identity.provider, &identity.id, &workspace);
                let agent = self
                    .events
                    .as_ref()
                    .and_then(|h| workspace_agent(h, &workspace));
                let timeline = self.timeline(&workspace).await;
                let reply = session_view(
                    self.backend.as_ref(),
                    &workspace,
                    agent.as_deref(),
                    hash.as_deref(),
                    timeline,
                )
                .await;
                send_r(tx, &reply).await?;
            }
            ClientMessage::SessionAnswer {
                workspace,
                prompt_id,
                option,
            } => {
                let agent = self
                    .events
                    .as_ref()
                    .and_then(|h| workspace_agent(h, &workspace));
                session_answer(
                    self.backend.as_ref(),
                    &workspace,
                    agent.as_deref(),
                    &prompt_id,
                    option,
                )
                .await?;
                info!(target: "audit", event = "session_answer_sent", provider = %identity.provider, account_id = %identity.id, session = %workspace, option);
            }
            ClientMessage::AgentHooks { agent, action } => {
                let target = self.hooks.clone();
                let (a, act) = (agent.clone(), action.clone());
                let reply = tokio::task::spawn_blocking(move || {
                    crate::agent_hooks::respond(target.as_ref(), &a, &act)
                })
                .await?;
                // Only the event is logged (who, applied or removed), never the settings file.
                if reply.error.is_none() && (action == "apply" || action == "remove") {
                    let event = if action == "apply" {
                        "agent_hooks_applied"
                    } else {
                        "agent_hooks_removed"
                    };
                    info!(target: "audit", event, provider = %identity.provider, account_id = %identity.id, login = %identity.login, agent = %agent);
                }
                send_r(
                    tx,
                    &ServerMessage::AgentHooks {
                        agent,
                        action,
                        state: reply.state.into(),
                        preview: reply.preview,
                        error: reply.error,
                    },
                )
                .await?;
            }
            ClientMessage::AcpOpen {
                workspace,
                after,
                epoch,
            } => {
                // Who looked at which workspace's stream, never what it held (as for a peek).
                audit_acp_open(&identity.provider, &identity.id, &workspace);
                // Subscribe first: a change between the two calls then still wakes the connection.
                let rx = self.backend.acp_watch(&workspace)?;
                let (mut view, info_rev) = self.backend.acp_view(&workspace, after, epoch, 0)?;
                view.workspace = workspace.clone();
                let (rev, epoch) = (view.rev, view.epoch);
                for part in crate::acp_stream::split(view) {
                    send_r(tx, &ServerMessage::AcpView(part)).await?;
                }
                *acp = Some(AcpSub {
                    workspace,
                    rx,
                    rev,
                    epoch,
                    info_rev,
                    due: false,
                    dead: false,
                });
            }
            ClientMessage::AcpClose => {
                *acp = None;
                *events = None;
            }
            ClientMessage::AcpEvents {
                workspace,
                after,
                epoch,
            } => {
                // Who followed which conversation, never what it held.
                audit_acp_open(&identity.provider, &identity.id, &workspace);
                // Subscribe first: a change between the two calls then still wakes the connection.
                let rx = self.backend.acp_watch(&workspace)?;
                let replay = self.backend.acp_events(&workspace, after, epoch)?;
                let (last, epoch) = (replay.last, replay.epoch);
                for m in events_messages(&workspace, replay, true) {
                    send_r(tx, &m).await?;
                }
                *events = Some(EventSub {
                    workspace,
                    rx,
                    after: last,
                    epoch,
                    due: false,
                    dead: false,
                });
            }
            ClientMessage::AcpPrompt { workspace, text } => {
                // Lines and indentation stay; every other control character (an escape sequence) goes.
                let text: String = text
                    .chars()
                    .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
                    .collect();
                if text.trim().is_empty() || text.len() > ACP_PROMPT_BYTES {
                    return Err(anyhow::anyhow!("The message is empty or too long"));
                }
                self.backend.acp_prompt(&workspace, &text).await?;
                // Who sent a prompt to which workspace, never what it said.
                info!(target: "audit", event = "acp_prompt_sent", provider = %identity.provider, account_id = %identity.id, session = %workspace);
            }
            ClientMessage::AcpInterrupt { workspace } => {
                self.backend.acp_interrupt(&workspace).await?;
                info!(target: "audit", event = "acp_interrupt", provider = %identity.provider, account_id = %identity.id, session = %workspace);
            }
            ClientMessage::AcpAnswer {
                workspace,
                prompt_id,
                option_id,
            } => {
                self.backend
                    .acp_answer(&workspace, &prompt_id, &option_id)
                    .await?;
                info!(target: "audit", event = "acp_answer_sent", provider = %identity.provider, account_id = %identity.id, session = %workspace);
            }
            ClientMessage::AcpMode { workspace, mode } => {
                self.backend.acp_mode(&workspace, &mode).await?;
                info!(target: "audit", event = "acp_mode_set", provider = %identity.provider, account_id = %identity.id, session = %workspace);
            }
            ClientMessage::History { from, to } => {
                let history = self.events.as_ref().and_then(|h| h.history()).cloned();
                let Some(history) = history else {
                    return Err(anyhow::anyhow!("History is not available"));
                };
                let limits = history.limits();
                let now = crate::events::now_ms();
                if from > to {
                    return Err(anyhow::anyhow!("The history range ends before it starts"));
                }
                // Nothing older than the server keeps exists, so a longer range only costs a longer scan.
                let lo =
                    from.max(to.saturating_sub((u64::from(limits.keep_days) + 1) * 86_400_000));
                // A scan of the file: off the async threads.
                let answer =
                    tokio::task::spawn_blocking(move || history.query(lo, to, now)).await?;
                // One message must fit what the direct transport carries; the newest runs that do are sent.
                let (runs, cut) = crate::history::fit(answer.runs, crate::history::REPLY_BYTES);
                send_r(
                    tx,
                    &ServerMessage::History {
                        from,
                        to,
                        runs,
                        truncated: answer.truncated || cut,
                        keep_days: limits.keep_days,
                        max_bytes: limits.max_bytes,
                    },
                )
                .await?;
            }
            ClientMessage::HealthHistory => {
                let Some(health) = &self.health else {
                    return Err(anyhow::anyhow!("Health is not available"));
                };
                send_r(tx, &health.history()).await?;
            }
            ClientMessage::HistoryDelete => {
                let history = self.events.as_ref().and_then(|h| h.history()).cloned();
                let Some(history) = history else {
                    return Err(anyhow::anyhow!("History is not available"));
                };
                let removed = tokio::task::spawn_blocking(move || history.delete()).await?;
                info!(target: "audit", event = "history_deleted", provider = %identity.provider, account_id = %identity.id, login = %identity.login, removed);
                send_r(tx, &ServerMessage::HistoryDeleted { removed }).await?;
            }
            ClientMessage::Unknown => {}
            ClientMessage::ListWorkspaces => {
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
            }
            ClientMessage::NewWorkspace {
                name,
                backend,
                dir,
                template,
            } => {
                self.registry
                    .create_workspace(
                        self.backend.as_ref(),
                        &name,
                        backend.as_deref(),
                        dir.as_deref(),
                        template.as_deref().unwrap_or("shell"),
                    )
                    .await?;
                let workspaces = self.registry.list(self.backend.as_ref()).await?;
                send_r(tx, &ServerMessage::Workspaces { workspaces }).await?;
                let _ = self.list_changed.send(conn);
            }
            ClientMessage::Attach { workspace_id, tab } => {
                *attachment = None; // detach the previous one first
                if self.registry.has(&workspace_id) {
                    let (attached, att) = self
                        .registry
                        .attach(
                            self.backend.as_ref(),
                            &workspace_id,
                            tab.as_deref(),
                            *cols,
                            *rows,
                        )
                        .await?;
                    *attachment = Some(att);
                    send_r(
                        tx,
                        &ServerMessage::WorkspaceAttached {
                            workspace_id,
                            tab: Some(attached),
                        },
                    )
                    .await?;
                } else {
                    // A workspace with no tabs (an ACP one): the old path, with no tab to name.
                    if let Some(index) = self
                        .windows
                        .as_ref()
                        .and_then(|h| h.representative(&workspace_id))
                    {
                        let _ = self.backend.focus_window(&workspace_id, index).await;
                    }
                    *attachment = Some(self.backend.attach(&workspace_id, *cols, *rows)?);
                    send_r(
                        tx,
                        &ServerMessage::WorkspaceAttached {
                            workspace_id,
                            tab: None,
                        },
                    )
                    .await?;
                }
            }
            ClientMessage::Detach => {
                *attachment = None;
            }
            ClientMessage::Input { data } => {
                if let Some(att) = attachment.as_mut() {
                    att.write(&B64.decode(data)?)?;
                }
            }
            ClientMessage::Resize { cols: c, rows: r } => {
                if c == 0 || r == 0 || c > 1000 || r > 1000 {
                    return Ok(());
                }
                *cols = c;
                *rows = r;
                if let Some(att) = attachment.as_ref() {
                    debug!("resize {c}x{r}");
                    att.resize(c, r)?;
                }
            }
        }
        Ok(())
    }
}

/// A tab's directory as clients see it: the workspace directory plus the relative part.
fn tab_path(root: &str, relative: &str) -> String {
    if relative.is_empty() {
        return root.to_string();
    }
    format!("{}/{}", root.trim_end_matches('/'), relative)
}

async fn send(tx: &LinkTx, msg: &ServerMessage) -> bool {
    tx.send(serde_json::to_string(msg).unwrap()).await
}

/// The `req` id a client put on a control message, if any.
#[derive(serde::Deserialize)]
struct ReqId {
    #[serde(default)]
    req: Option<u64>,
}

/// Answers a control message on the control channel: success, or the error that stopped it. False when the
/// connection is gone.
async fn send_ack(tx: &LinkTx, req: u64, result: std::result::Result<(), String>) -> bool {
    let msg = match result {
        Ok(()) => ServerMessage::ControlAck {
            req,
            ok: true,
            error: None,
        },
        Err(error) => ServerMessage::ControlAck {
            req,
            ok: false,
            error: Some(error),
        },
    };
    tx.send_control(serde_json::to_string(&msg).unwrap()).await
}

async fn send_control_r(tx: &LinkTx, msg: &ServerMessage) -> Result<()> {
    if tx.send_control(serde_json::to_string(msg).unwrap()).await {
        Ok(())
    } else {
        Err(anyhow::anyhow!("connection closed"))
    }
}

async fn send_r(tx: &LinkTx, msg: &ServerMessage) -> Result<()> {
    if send(tx, msg).await {
        Ok(())
    } else {
        Err(anyhow::anyhow!("connection closed"))
    }
}

/// Next chunk of terminal output; pends forever while nothing is attached.
async fn next_output(attachment: &mut Option<Attachment>) -> Option<Vec<u8>> {
    match attachment {
        Some(att) => att.rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Wakes when the ACP workspace this connection shows has a new revision. `false` means the stream is gone for
/// good; it then pends, as do connections that show no ACP workspace or already owe an update.
async fn next_event_change(events: &mut Option<EventSub>) -> bool {
    match events {
        Some(sub) if !sub.dead && !sub.due => sub.rx.changed().await.is_ok(),
        _ => std::future::pending().await,
    }
}

async fn next_acp_change(acp: &mut Option<AcpSub>) -> bool {
    match acp {
        Some(sub) if !sub.dead && !sub.due => sub.rx.changed().await.is_ok(),
        _ => std::future::pending().await,
    }
}

/// The next agent events (up to 20 at once); `None` when this connection fell behind; pends forever
/// when events are off.
async fn next_events(
    rx: &mut Option<tokio::sync::broadcast::Receiver<termenbox_core::events::AgentEvent>>,
) -> Option<Vec<termenbox_core::events::AgentEvent>> {
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};
    let Some(rx) = rx else {
        return std::future::pending().await;
    };
    let mut out = match rx.recv().await {
        Ok(e) => vec![e],
        Err(RecvError::Lagged(_)) => return None,
        Err(RecvError::Closed) => return std::future::pending().await,
    };
    while out.len() < 20 {
        match rx.try_recv() {
            Ok(e) => out.push(e),
            Err(TryRecvError::Lagged(_)) => return None,
            Err(_) => break,
        }
    }
    Some(out)
}

/// Resolves with the connection that changed the list of workspaces (`None` after a missed ping, which still means
/// "changed").
async fn next_list_change(rx: &mut tokio::sync::broadcast::Receiver<u64>) -> Option<u64> {
    use tokio::sync::broadcast::error::RecvError;
    match rx.recv().await {
        Ok(from) => Some(from),
        Err(RecvError::Lagged(_)) => None,
        Err(RecvError::Closed) => std::future::pending().await,
    }
}

async fn next_status(
    rx: &mut Option<tokio::sync::broadcast::Receiver<crate::events::StatusChange>>,
) -> Option<crate::events::StatusChange> {
    use tokio::sync::broadcast::error::RecvError;
    let Some(rx) = rx else {
        return std::future::pending().await;
    };
    match rx.recv().await {
        Ok(c) => Some(c),
        Err(RecvError::Lagged(_)) => None,
        Err(RecvError::Closed) => std::future::pending().await,
    }
}

/// The `update_status` message owed to a client when the recorded release differs from the one it was last told.
fn update_change(sent: &mut String, now: String) -> Option<ServerMessage> {
    if *sent == now {
        return None;
    }
    sent.clone_from(&now);
    Some(ServerMessage::UpdateStatus { available: now })
}

fn status_message(c: crate::events::StatusChange) -> ServerMessage {
    match c {
        crate::events::StatusChange::Activity(a) => ServerMessage::Activity {
            workspaces: vec![a],
        },
        crate::events::StatusChange::Attention(items) => ServerMessage::Attention { items },
    }
}

fn window_message(u: crate::control::WindowUpdate) -> ServerMessage {
    ServerMessage::WindowList {
        workspace: u.workspace,
        windows: u.windows,
        representative: u.representative,
    }
}

/// The next change to any workspace's windows (`None` when this connection fell behind); pends forever when
/// control mode is off.
async fn next_window(
    rx: &mut Option<tokio::sync::broadcast::Receiver<crate::control::WindowUpdate>>,
) -> Option<crate::control::WindowUpdate> {
    use tokio::sync::broadcast::error::RecvError;
    let Some(rx) = rx else {
        return std::future::pending().await;
    };
    match rx.recv().await {
        Ok(u) => Some(u),
        Err(RecvError::Lagged(_)) => None,
        Err(RecvError::Closed) => std::future::pending().await,
    }
}

/// The most text one `scrollback` part carries, so that a part fits a message of the direct transport (64 KiB).
const SCROLLBACK_PART: usize = 24 * 1024;

/// A workspace's history as `scrollback` parts of at most `SCROLLBACK_PART` bytes, cut at line ends where it can (a line
/// longer than a part is cut at a character). There is always at least one part, and the last one says so.
pub(crate) fn scrollback_parts(workspace: &str, text: &str) -> Vec<ServerMessage> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.split_inclusive('\n') {
        let mut line = line;
        while cur.len() + line.len() > SCROLLBACK_PART {
            if cur.is_empty() {
                let mut at = SCROLLBACK_PART;
                while !line.is_char_boundary(at) {
                    at -= 1;
                }
                parts.push(line[..at].to_string());
                line = &line[at..];
            } else {
                parts.push(std::mem::take(&mut cur));
            }
        }
        cur.push_str(line);
    }
    if !cur.is_empty() || parts.is_empty() {
        parts.push(cur);
    }
    let n = parts.len();
    parts
        .into_iter()
        .enumerate()
        .map(|(i, text)| ServerMessage::Scrollback {
            workspace: workspace.to_string(),
            part: i as u32,
            last: i + 1 == n,
            text,
        })
        .collect()
}

fn audit_acp_open(provider: &str, account_id: &str, workspace: &str) {
    info!(target: "audit", event = "acp_open", provider, account_id, session = %workspace);
}

fn audit_peek(provider: &str, account_id: &str, workspace: &str) {
    info!(target: "audit", event = "peek", provider, account_id, session = %workspace);
}

/// The most a snapshot carries; a phone screen is a few KB, this only stops a huge pane from being sent.
pub const SNAPSHOT_MAX: usize = 16 * 1024;

/// A workspace's visible screen for the client to draw at once (decision 0084). Never fails: a manager that cannot
/// capture (a `command` manager) gets an empty snapshot, which the client reads as "attach normally".
#[cfg(test)]
pub async fn peek(
    backend: &dyn WorkspaceBackend,
    workspace: &str,
    known: Option<&str>,
) -> ServerMessage {
    peek_window(backend, workspace, known, None).await
}

/// `peek` of one window: the one the workspace's tab speaks for, so the snapshot shows what the tab promised.
pub async fn peek_window(
    backend: &dyn WorkspaceBackend,
    workspace: &str,
    known: Option<&str>,
    window: Option<u32>,
) -> ServerMessage {
    use sha2::{Digest, Sha256};
    let none = || ServerMessage::Snapshot {
        workspace: workspace.to_string(),
        text: String::new(),
        hash: String::new(),
        unchanged: false,
    };
    let captured = match window {
        Some(i) => backend.capture_window_styled(workspace, i).await,
        None => backend.capture_styled(workspace).await,
    };
    let Ok(raw) = captured else {
        return none();
    };
    // Blank lines below the cursor carry nothing; keep the end of the text when it is too long.
    let mut text = raw.trim_end().to_string();
    if text.len() > SNAPSHOT_MAX {
        let mut cut = text.len() - SNAPSHOT_MAX;
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        text = text[cut..].to_string();
    }
    let hash: String = Sha256::digest(text.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if known == Some(hash.as_str()) {
        return ServerMessage::Snapshot {
            workspace: workspace.to_string(),
            text: String::new(),
            hash,
            unchanged: true,
        };
    }
    ServerMessage::Snapshot {
        workspace: workspace.to_string(),
        text,
        hash,
        unchanged: false,
    }
}

/// `peek` of one tab, by the tmux window it runs in.
pub async fn peek_tab(
    backend: &dyn WorkspaceBackend,
    workspace: &str,
    known: Option<&str>,
    window: Option<&str>,
) -> ServerMessage {
    use sha2::{Digest, Sha256};
    let none = || ServerMessage::Snapshot {
        workspace: workspace.to_string(),
        text: String::new(),
        hash: String::new(),
        unchanged: false,
    };
    let captured = match window {
        Some(window) => backend.capture_tab_styled(workspace, window).await,
        None => backend.capture_styled(workspace).await,
    };
    let Ok(raw) = captured else {
        return none();
    };
    let mut text = raw.trim_end().to_string();
    if text.len() > SNAPSHOT_MAX {
        let mut cut = text.len() - SNAPSHOT_MAX;
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        text = text[cut..].to_string();
    }
    let hash: String = Sha256::digest(text.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if known == Some(hash.as_str()) {
        return ServerMessage::Snapshot {
            workspace: workspace.to_string(),
            text: String::new(),
            hash,
            unchanged: true,
        };
    }
    ServerMessage::Snapshot {
        workspace: workspace.to_string(),
        text,
        hash,
        unchanged: false,
    }
}

const PROMPT_CHANGED: &str = "That prompt changed";

/// The agent the event layer found in a workspace, if any.
fn workspace_agent(hub: &EventHub, workspace: &str) -> Option<String> {
    hub.snapshot()
        .into_iter()
        .find(|s| s.workspace == workspace)
        .and_then(|s| s.agent)
}

/// The workspace view (decision 0103): the screen read by the agent's profile (T2) or the generic rules (T3). Never
/// fails; a manager that cannot capture gives an empty tail, which the client shows with the full-terminal button.
pub async fn session_view(
    backend: &dyn WorkspaceBackend,
    workspace: &str,
    agent: Option<&str>,
    known: Option<&str>,
    timeline: Option<termenbox_core::protocol::Timeline>,
) -> ServerMessage {
    use crate::profile::Profiles;
    use sha2::{Digest, Sha256};
    use termenbox_core::protocol::{ViewOption, ViewPrompt};
    let (profile, tier) = Profiles::builtin().for_agent(agent);
    // The transcript timeline is tier 1; the screen profile still reads prompts and the tail.
    let tier = if timeline.is_some() {
        crate::profile::Tier::T1
    } else {
        tier
    };
    let screen = backend.capture(workspace).await.unwrap_or_default();
    let view = profile.read(&screen);
    let prompt = view.prompt.as_ref().map(|p| ViewPrompt {
        id: p.id(),
        question: p.question.clone(),
        kind: p.kind.to_string(),
        options: p
            .options
            .iter()
            .map(|o| ViewOption {
                label: o.label.clone(),
            })
            .collect(),
    });
    let mut h = Sha256::new();
    h.update(tier.as_str());
    for l in &view.tail {
        h.update(l.as_bytes());
        h.update([b'\n']);
    }
    h.update(view.task.as_deref().unwrap_or("").as_bytes());
    h.update(
        prompt
            .as_ref()
            .map(|p| p.id.as_str())
            .unwrap_or("")
            .as_bytes(),
    );
    if let Some(t) = &timeline {
        h.update(serde_json::to_string(t).unwrap_or_default().as_bytes());
    }
    let hash: String = h.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let unchanged = known == Some(hash.as_str());
    ServerMessage::SessionView {
        workspace: workspace.to_string(),
        tier: tier.as_str().into(),
        agent: agent.map(str::to_string),
        tail: if unchanged { Vec::new() } else { view.tail },
        hash,
        unchanged,
        task: if unchanged { None } else { view.task },
        prompt: if unchanged { None } else { prompt },
        timeline: if unchanged { None } else { timeline },
    }
}

/// A tap on option `option` of the prompt the user saw. The screen is read again and the keys come from that
/// reading: they are sent only if the prompt on the pane still has the id the client saw and lists that option.
pub async fn session_answer(
    backend: &dyn WorkspaceBackend,
    workspace: &str,
    agent: Option<&str>,
    prompt_id: &str,
    option: u8,
) -> Result<()> {
    let (profile, _) = crate::profile::Profiles::builtin().for_agent(agent);
    let screen = backend.capture(workspace).await?;
    let prompt = profile
        .read(&screen)
        .prompt
        .filter(|p| p.id() == prompt_id)
        .ok_or_else(|| anyhow::anyhow!(PROMPT_CHANGED))?;
    let keys = prompt
        .keys(option as usize)
        .ok_or_else(|| anyhow::anyhow!("No such choice"))?;
    backend.send_text(workspace, keys).await
}

/// A one-tap answer (plan v8, K.4). It acts only on the prompt the user saw: the workspace's current prompt, with
/// the same id and event number, confirmed by the agent's own hook (`confidence=high`), and still on the pane.
/// The keys are the ones the screen's own choice list defines, never text from the client.
#[allow(dead_code)] // the tests answer without a tab; the handler uses answer_tab
pub async fn answer(
    backend: &dyn WorkspaceBackend,
    hub: &EventHub,
    workspace: &str,
    prompt_id: &str,
    seq: u64,
    choice: u8,
) -> Result<()> {
    answer_tab(backend, hub, workspace, None, None, prompt_id, seq, choice).await
}

/// [answer] for one tab; the keys go to that tab's pane instead of the workspace's current one.
#[allow(clippy::too_many_arguments)]
pub async fn answer_tab(
    backend: &dyn WorkspaceBackend,
    hub: &EventHub,
    workspace: &str,
    window: Option<&str>,
    tab: Option<&str>,
    prompt_id: &str,
    seq: u64,
    choice: u8,
) -> Result<()> {
    use termenbox_core::events::{Confidence, EventKind};
    // A tab carries its own hook-confirmed prompt: another tab's later prompt must not invalidate it. The keys still
    // come from this tab's screen, and the id still names the question the user saw.
    if let (Some(window), Some(tab)) = (window, tab) {
        if let Some(prompt) = hub.tab_prompt(workspace, tab) {
            if prompt.id != prompt_id {
                return Err(anyhow::anyhow!(PROMPT_CHANGED));
            }
            let screen = backend.capture_tab_styled(workspace, window).await?;
            let info = crate::adapters::parse_prompt(&screen)
                .filter(|i| prompt_id.ends_with(&crate::agentwatch::hex8(&i.question)))
                .ok_or_else(|| anyhow::anyhow!(PROMPT_CHANGED))?;
            let (_, keys) = info
                .choices
                .get(choice as usize)
                .ok_or_else(|| anyhow::anyhow!("No such choice"))?;
            return backend.send_text_to_window(workspace, window, keys).await;
        }
    }
    let current = hub
        .current_prompt(workspace)
        .ok_or_else(|| anyhow::anyhow!(PROMPT_CHANGED))?;
    let EventKind::Prompt { id, .. } = &current.kind else {
        return Err(anyhow::anyhow!(PROMPT_CHANGED));
    };
    if id != prompt_id || current.seq != seq {
        return Err(anyhow::anyhow!(PROMPT_CHANGED));
    }
    // A T0 prompt carries the options the agent offered: the tap names one by position and the answer is that
    // option's id, exactly as the agent sent it (decision 0111). There is no screen to check.
    if current.source == termenbox_core::events::Source::Acp {
        let EventKind::Prompt {
            decision: Some(decision),
            ..
        } = &current.kind
        else {
            return Err(anyhow::anyhow!(PROMPT_CHANGED));
        };
        let option = decision
            .options
            .get(choice as usize)
            .ok_or_else(|| anyhow::anyhow!("No such choice"))?;
        return backend.acp_answer(workspace, id, &option.id).await;
    }
    if current.confidence != Confidence::High {
        return Err(anyhow::anyhow!(
            "This prompt is not confirmed by the agent; open the terminal to answer"
        ));
    }
    let screen = match window {
        Some(window) => backend.capture_tab_styled(workspace, window).await?,
        None => backend.capture(workspace).await?,
    };
    let info = crate::adapters::parse_prompt(&screen)
        .filter(|i| id.ends_with(&crate::agentwatch::hex8(&i.question)))
        .ok_or_else(|| anyhow::anyhow!(PROMPT_CHANGED))?;
    let (_, keys) = info
        .choices
        .get(choice as usize)
        .ok_or_else(|| anyhow::anyhow!("No such choice"))?;
    match window {
        Some(window) => backend.send_text_to_window(workspace, window, keys).await,
        None => backend.send_text(workspace, keys).await,
    }
}

#[cfg(test)]
mod answer_tests {
    use super::*;
    use crate::agentwatch::hex8;
    use std::sync::Mutex;
    use termenbox_core::events::{AgentState, Confidence, EventKind, Source};

    struct Fake {
        screen: Mutex<String>,
        sent: Mutex<Vec<String>>,
    }
    #[async_trait::async_trait]
    impl WorkspaceBackend for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        async fn list(&self) -> Result<Vec<termenbox_core::protocol::WorkspaceInfo>> {
            Ok(vec![])
        }
        async fn create(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn attach(&self, _: &str, _: u16, _: u16) -> Result<Attachment> {
            Err(anyhow::anyhow!("no"))
        }
        async fn capture(&self, _: &str) -> Result<String> {
            Ok(self.screen.lock().unwrap().clone())
        }
        async fn send_text(&self, _: &str, text: &str) -> Result<()> {
            self.sent.lock().unwrap().push(text.to_string());
            Ok(())
        }
        async fn acp_answer(&self, _: &str, prompt_id: &str, option_id: &str) -> Result<()> {
            self.sent
                .lock()
                .unwrap()
                .push(format!("acp:{prompt_id}:{option_id}"));
            Ok(())
        }
    }

    const SCREEN: &str =
        "Edit file?\n Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, always\n   3. No\n";

    fn setup(confidence: Confidence, screen: &str) -> (Fake, EventHub, String, u64) {
        let hub = EventHub::new([3; 32]);
        let question = crate::adapters::detect_prompt(SCREEN).unwrap();
        let id = format!("1-{}", hex8(&question));
        hub.push(
            "s",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::NeedsInput,
            },
            1,
        );
        hub.push(
            "s",
            Source::Screen,
            confidence,
            EventKind::Prompt {
                id: id.clone(),
                prompt_kind: "choice".into(),
                choices: vec![],
                decision: None,
            },
            2,
        );
        let seq = hub.current_prompt("s").unwrap().seq;
        (
            Fake {
                screen: Mutex::new(screen.into()),
                sent: Mutex::default(),
            },
            hub,
            id,
            seq,
        )
    }

    /// A T0 prompt (decision 0111): the tap names an option by position, the agent receives that option's id and no
    /// screen or key is involved; a stale prompt or a position the agent did not offer is refused.
    #[tokio::test]
    async fn a_tap_on_a_t0_prompt_answers_with_the_option_id_the_agent_offered() {
        use termenbox_core::events::{Decision, DecisionOption};
        let hub = EventHub::new([3; 32]);
        let opt = |id: &str, kind: &str, name: &str| DecisionOption {
            id: id.into(),
            kind: kind.into(),
            name: name.into(),
        };
        hub.push(
            "s",
            Source::Acp,
            Confidence::High,
            EventKind::Prompt {
                id: "acp-4".into(),
                prompt_kind: "permission".into(),
                choices: vec!["Yes".into(), "No".into()],
                decision: Some(Decision {
                    options: vec![
                        opt("allow-once", "allow_once", "Yes"),
                        opt("reject", "reject_once", "No"),
                    ],
                    detail: None,
                }),
            },
            2,
        );
        let seq = hub.current_prompt("s").unwrap().seq;
        let b = Fake {
            screen: Mutex::new(String::new()),
            sent: Mutex::default(),
        };
        answer(&b, &hub, "s", "acp-4", seq, 1).await.unwrap();
        assert_eq!(*b.sent.lock().unwrap(), ["acp:acp-4:reject"]);
        for (id, seq, choice) in [("acp-3", seq, 0), ("acp-4", seq + 1, 0), ("acp-4", seq, 2)] {
            assert!(
                answer(&b, &hub, "s", id, seq, choice).await.is_err(),
                "{id} {seq} {choice}"
            );
        }
        assert_eq!(b.sent.lock().unwrap().len(), 1, "nothing else was sent");
    }

    fn view_of(
        m: ServerMessage,
    ) -> (
        String,
        Vec<String>,
        String,
        bool,
        Option<termenbox_core::protocol::ViewPrompt>,
    ) {
        match m {
            ServerMessage::SessionView {
                tier,
                tail,
                hash,
                unchanged,
                prompt,
                ..
            } => (tier, tail, hash, unchanged, prompt),
            other => panic!("not a workspace view: {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_workspace_view_lists_labels_only_and_a_tap_sends_the_profiles_keys() {
        let (b, _, _, _) = setup(Confidence::High, SCREEN);
        let (tier, tail, hash, unchanged, prompt) =
            view_of(session_view(&b, "s", None, None, None).await);
        assert_eq!((tier.as_str(), unchanged), ("t3", false));
        assert!(tail.iter().any(|l| l.contains("Do you want to proceed?")));
        let prompt = prompt.expect("a numbered Yes/No list is a prompt");
        let wire = serde_json::to_string(&prompt).unwrap();
        assert!(!wire.contains("keys"), "{wire}");
        assert_eq!(prompt.options.len(), 3);
        // a known hash answers `unchanged` with nothing else
        let (_, tail2, _, unchanged, prompt2) =
            view_of(session_view(&b, "s", None, Some(&hash), None).await);
        assert!(unchanged && tail2.is_empty() && prompt2.is_none());
        for (option, keys) in [(0u8, "1"), (1, "2"), (2, "3")] {
            session_answer(&b, "s", None, &prompt.id, option)
                .await
                .unwrap();
            assert_eq!(b.sent.lock().unwrap().last().unwrap(), keys);
        }
    }

    #[tokio::test]
    async fn a_timeline_makes_the_view_tier_one_and_without_one_it_stays_t2_or_t3() {
        use termenbox_core::protocol::{Timeline, TimelineEntry};
        let (b, _, _, _) = setup(Confidence::High, SCREEN);
        let tl = Timeline {
            entries: vec![TimelineEntry {
                kind: "prompt".into(),
                name: None,
                text: "fix it".into(),
            }],
            ..Timeline::default()
        };
        let (tier, _, hash, _, _) =
            view_of(session_view(&b, "s", None, None, Some(tl.clone())).await);
        assert_eq!(tier, "t1");
        let (_, tail, _, unchanged, _) =
            view_of(session_view(&b, "s", None, Some(&hash), Some(tl.clone())).await);
        assert!(unchanged && tail.is_empty());
        // a new step is a new hash
        let mut more = tl;
        more.entries.push(TimelineEntry {
            kind: "tool".into(),
            name: Some("Bash".into()),
            text: "ls".into(),
        });
        let (_, _, _, unchanged, _) =
            view_of(session_view(&b, "s", None, Some(&hash), Some(more)).await);
        assert!(!unchanged);
        let (tier, ..) = view_of(session_view(&b, "s", Some("claude"), None, None).await);
        assert_eq!(tier, "t2");
        let (tier, ..) = view_of(session_view(&b, "s", None, None, None).await);
        assert_eq!(tier, "t3");
    }

    #[tokio::test]
    async fn a_workspace_answer_for_another_prompt_or_option_sends_nothing() {
        let (b, _, _, _) = setup(Confidence::High, SCREEN);
        let (_, _, _, _, prompt) = view_of(session_view(&b, "s", None, None, None).await);
        let id = prompt.unwrap().id;
        for (pid, option) in [
            ("v-000000000000", 0u8),
            (id.as_str(), 3),
            (id.as_str(), 255),
        ] {
            assert!(session_answer(&b, "s", None, pid, option).await.is_err());
        }
        // the question is gone from the pane
        *b.screen.lock().unwrap() = "$ ls\nfile\n".into();
        assert!(session_answer(&b, "s", None, &id, 0).await.is_err());
        assert!(b.sent.lock().unwrap().is_empty());
        let (_, _, _, _, prompt) = view_of(session_view(&b, "s", None, None, None).await);
        assert!(
            prompt.is_none(),
            "a screen where nothing should match gives no prompt"
        );
    }

    #[tokio::test]
    async fn the_choice_sends_exactly_its_own_keys() {
        for (choice, keys) in [(0u8, "1"), (1, "2"), (2, "3")] {
            let (b, hub, id, seq) = setup(Confidence::High, SCREEN);
            answer(&b, &hub, "s", &id, seq, choice).await.unwrap();
            assert_eq!(*b.sent.lock().unwrap(), vec![keys.to_string()]);
        }
    }

    #[tokio::test]
    async fn a_stale_wrong_changed_or_unconfirmed_prompt_is_refused_and_nothing_is_sent() {
        let (b, hub, id, seq) = setup(Confidence::High, SCREEN);
        for (pid, s) in [("9-ffffffff", seq), (id.as_str(), seq + 5)] {
            let e = answer(&b, &hub, "s", pid, s, 0)
                .await
                .unwrap_err()
                .to_string();
            assert!(e.contains("changed"), "{e}");
        }
        assert!(answer(&b, &hub, "s", &id, seq, 7)
            .await
            .unwrap_err()
            .to_string()
            .contains("No such choice"));
        assert!(answer(&b, &hub, "other", &id, seq, 0).await.is_err());
        // the pane no longer shows the question
        *b.screen.lock().unwrap() = "$ ls\nfile\n".into();
        assert!(answer(&b, &hub, "s", &id, seq, 0)
            .await
            .unwrap_err()
            .to_string()
            .contains("changed"));
        // the agent moved on
        *b.screen.lock().unwrap() = SCREEN.into();
        hub.push(
            "s",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::Working,
            },
            3,
        );
        assert!(answer(&b, &hub, "s", &id, seq, 0)
            .await
            .unwrap_err()
            .to_string()
            .contains("changed"));
        // low confidence
        let (b2, hub2, id2, seq2) = setup(Confidence::Low, SCREEN);
        assert!(answer(&b2, &hub2, "s", &id2, seq2, 0)
            .await
            .unwrap_err()
            .to_string()
            .contains("not confirmed"));
        assert!(b.sent.lock().unwrap().is_empty() && b2.sent.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod peek_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Fake(Mutex<Option<String>>);
    #[async_trait::async_trait]
    impl WorkspaceBackend for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        async fn list(&self) -> Result<Vec<termenbox_core::protocol::WorkspaceInfo>> {
            Ok(vec![])
        }
        async fn create(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn attach(&self, _: &str, _: u16, _: u16) -> Result<Attachment> {
            Err(anyhow::anyhow!("no"))
        }
        async fn capture_styled(&self, _: &str) -> Result<String> {
            self.0
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| anyhow::anyhow!("no pane"))
        }
    }

    fn parts(m: ServerMessage) -> (String, String, bool) {
        match m {
            ServerMessage::Snapshot {
                text,
                hash,
                unchanged,
                ..
            } => (text, hash, unchanged),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_snapshot_carries_the_text_then_says_unchanged_for_a_known_hash() {
        let b = Fake(Mutex::new(Some("\x1b[31mred\x1b[0m word\n\n\n".into())));
        let (text, hash, unchanged) = parts(peek(&b, "s", None).await);
        assert_eq!(
            text, "\x1b[31mred\x1b[0m word",
            "colour codes stay, trailing blank lines go"
        );
        assert!(!unchanged && hash.len() == 16);
        let (text2, hash2, unchanged2) = parts(peek(&b, "s", Some(&hash)).await);
        assert!(text2.is_empty() && unchanged2 && hash2 == hash);
        *b.0.lock().unwrap() = Some("changed".into());
        assert!(
            !parts(peek(&b, "s", Some(&hash)).await).2,
            "a different screen is sent in full"
        );
    }

    #[tokio::test]
    async fn a_huge_pane_is_cut_to_its_end_and_a_manager_that_cannot_capture_gets_an_empty_snapshot(
    ) {
        let big = format!("{}END", "x".repeat(40_000));
        let b = Fake(Mutex::new(Some(big)));
        let (text, _, _) = parts(peek(&b, "s", None).await);
        assert_eq!(text.len(), SNAPSHOT_MAX);
        assert!(text.ends_with("END"));
        let none = Fake(Mutex::new(None));
        assert_eq!(
            parts(peek(&none, "s", None).await),
            (String::new(), String::new(), false)
        );
    }

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
        type Writer = Buf;
        fn make_writer(&'a self) -> Buf {
            self.clone()
        }
    }

    /// The audit line the handler writes names the workspace and nothing from the screen.
    #[tokio::test]
    async fn the_audit_line_for_a_peek_holds_no_pane_text() {
        let buf = Buf::default();
        let sub = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_ansi(false)
            .finish();
        let secret = "SECRET-SCREEN-TEXT";
        let b = Fake(Mutex::new(Some(secret.into())));
        let _guard = tracing::subscriber::set_default(sub);
        audit_peek("github", "1", "s");
        let reply = peek(&b, "s", None).await;
        assert!(parts(reply).0.contains(secret));
        let logged = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(
            logged.contains("peek") && !logged.contains(secret),
            "{logged}"
        );
    }

    /// Opening an ACP workspace's stream is logged with the workspace's name and nothing the stream holds.
    #[test]
    fn the_audit_line_for_opening_an_acp_stream_names_the_workspace_only() {
        let buf = Buf::default();
        let sub = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(sub);
        audit_acp_open("github", "1", "acp:work");
        let logged = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(
            logged.contains("acp_open") && logged.contains("acp:work") && logged.contains("audit"),
            "{logged}"
        );
    }
}

#[cfg(test)]
mod propagation_tests {
    use super::*;
    use crate::auth::{AuthState, Identity};
    use crate::cert::CertKeys;
    use std::sync::Mutex;

    /// A manager whose one workspace can be renamed and listed back.
    struct Names(Mutex<String>);
    #[async_trait::async_trait]
    impl WorkspaceBackend for Names {
        fn name(&self) -> &str {
            "fake"
        }
        async fn list(&self) -> Result<Vec<termenbox_core::protocol::WorkspaceInfo>> {
            Ok(vec![termenbox_core::protocol::WorkspaceInfo {
                id: "s".into(),
                name: self.0.lock().unwrap().clone().into(),
                agent: None,
                backend: None,
                ..Default::default()
            }])
        }
        async fn create(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn attach(&self, _: &str, _: u16, _: u16) -> Result<Attachment> {
            Err(anyhow::anyhow!("no"))
        }
        async fn rename(&self, _: &str, name: &str) -> Result<()> {
            *self.0.lock().unwrap() = name.to_string();
            Ok(())
        }
    }

    /// Every connection uses one queue and a restart restores the same pending owner request.
    #[test]
    fn update_checks_and_install_requests_are_shared_across_connections_and_survive_restart() {
        let root = std::env::temp_dir().join(format!("tm-update-gate-{}", uuid::Uuid::new_v4()));
        let dir = root.join("requests");
        let manager = || {
            WorkspaceManager::new(
                Arc::new(Names(Mutex::new("w".into()))),
                RunMode::Container,
                Arc::new(Registry::memory()),
            )
            .with_requests(dir.clone())
        };
        let first = manager();
        let other = first.clone();
        first.check_update();
        other.check_update();
        assert_eq!(
            crate::requests::take_requests(&dir),
            vec![crate::requests::Request::CheckUpdate {}]
        );
        other.check_update();
        assert!(crate::requests::take_requests(&dir).is_empty());
        let id = first.request_update(&dir).unwrap();
        assert_eq!(other.request_update(&dir).unwrap(), id);
        assert_eq!(crate::requests::take_requests(&dir).len(), 1);
        assert_eq!(manager().request_update(&dir).unwrap(), id);
        crate::requests::write_json(
            &root.join("update-result.json"),
            &crate::requests::UpdateResult {
                id: id.clone(),
                error: Some("held".into()),
            },
        )
        .unwrap();
        assert_ne!(first.request_update(&dir).unwrap(), id);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Reads messages from a connection until one lists the workspace under `name`.
    async fn sees_name(end: &mut crate::link::LinkEnd, name: &str) -> bool {
        let wanted = format!("\"name\":\"{name}\"");
        let read = async {
            while let Some(m) = end.from_workspace.recv().await {
                if m.contains("\"type\":\"workspaces\"") && m.contains(&wanted) {
                    return true;
                }
            }
            false
        };
        tokio::time::timeout(Duration::from_secs(3), read)
            .await
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn only_the_owner_can_publish_an_install_request() {
        let root = std::env::temp_dir().join(format!("tm-update-owner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let auth = AuthState::new(root.join("auth"), CertKeys::default(), "srv".into()).unwrap();
        let identity = |id: &str| Identity {
            provider: "github".into(),
            id: id.into(),
            login: format!("login-{id}"),
            issued_at: None,
        };
        let claim = auth.claim_token().unwrap().unwrap();
        auth.authorize(&identity("1"), Some(&claim)).unwrap();
        for who in ["1", "2"] {
            let dir = root.join(who).join("requests");
            let manager = WorkspaceManager::new(
                Arc::new(Names(Mutex::new("w".into()))),
                RunMode::Container,
                Arc::new(Registry::memory()),
            )
            .with_requests(dir.clone());
            let (link, _end) = crate::link::pair(None, None);
            let result = manager
                .handle(
                    1,
                    ClientMessage::UpdateServer {},
                    &auth,
                    &identity(who),
                    &mut None,
                    &mut 80,
                    &mut 24,
                    &mut None,
                    &mut None,
                    &link.tx,
                )
                .await;
            assert_eq!(result.is_ok(), who == "1");
            assert_eq!(
                crate::requests::take_requests(&dir).len(),
                usize::from(who == "1")
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_rename_on_one_connection_reaches_every_other_connection() {
        let dir = std::env::temp_dir().join(format!("tm-prop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let auth =
            Arc::new(AuthState::new(dir.clone(), CertKeys::default(), "srv".into()).unwrap());
        let mgr = WorkspaceManager::new(
            Arc::new(Names(Mutex::new("first".into()))),
            RunMode::Container,
            Arc::new(crate::registry::Registry::memory()),
        );
        let id = Identity {
            provider: "github".into(),
            id: "1".into(),
            login: "u".into(),
            issued_at: None,
        };
        let (la, mut ea) = crate::link::pair(None, None);
        let (lb, mut eb) = crate::link::pair(None, None);
        for l in [la, lb] {
            let (m, i, a) = (mgr.clone(), id.clone(), auth.clone());
            tokio::spawn(async move {
                let _ = m.serve(l, i, &a).await;
            });
        }
        // Both devices are connected and neither has asked for anything.
        tokio::time::sleep(Duration::from_millis(200)).await;
        ea.to_workspace
            .send(Inbound::Json(
                r#"{"type":"rename_workspace","workspace":"s","name":"second"}"#.into(),
            ))
            .await
            .unwrap();
        assert!(
            sees_name(&mut ea, "second").await,
            "the device that renamed sees it"
        );
        assert!(
            sees_name(&mut eb, "second").await,
            "a device that did not ask sees it too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Everything a fresh connection is sent in its first 400 ms.
    async fn first_messages(mgr: WorkspaceManager) -> Vec<String> {
        let dir = std::env::temp_dir().join(format!("tm-prev-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let auth =
            Arc::new(AuthState::new(dir.clone(), CertKeys::default(), "srv".into()).unwrap());
        let id = Identity {
            provider: "github".into(),
            id: "1".into(),
            login: "u".into(),
            issued_at: None,
        };
        let (link, mut end) = crate::link::pair(None, None);
        tokio::spawn(async move {
            let _ = mgr.serve(link, id, &auth).await;
        });
        let mut seen = Vec::new();
        let _ = tokio::time::timeout(Duration::from_millis(400), async {
            while let Some(m) = end.from_workspace.recv().await {
                seen.push(m);
            }
        })
        .await;
        let _ = std::fs::remove_dir_all(&dir);
        seen
    }

    #[tokio::test]
    async fn a_sign_in_after_an_unclean_exit_is_told_and_a_clean_one_is_not() {
        let backend = || Arc::new(Names(Mutex::new("first".into())));
        let message = termenbox_core::protocol::ServerMessage::PreviousExit {
            at_ms: 1234,
            streak: 2,
            crashed: true,
        };
        let told = first_messages(
            WorkspaceManager::new(
                backend(),
                RunMode::Container,
                Arc::new(crate::registry::Registry::memory()),
            )
            .with_previous_exit(Some(message)),
        )
        .await;
        let line = told
            .iter()
            .find(|m| m.contains("\"type\":\"previous_exit\""))
            .unwrap_or_else(|| panic!("no previous_exit in {told:?}"));
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            (
                v["at_ms"].as_u64(),
                v["streak"].as_u64(),
                v["crashed"].as_bool()
            ),
            (Some(1234), Some(2), Some(true))
        );
        let quiet = first_messages(WorkspaceManager::new(
            backend(),
            RunMode::Container,
            Arc::new(crate::registry::Registry::memory()),
        ))
        .await;
        assert!(
            quiet.iter().all(|m| !m.contains("previous_exit")),
            "{quiet:?}"
        );
    }

    /// What a connection signed in as `who` is sent for a `diagnostics_bundle` request, when account 1 is the owner.
    async fn bundle_replies(who: &str) -> Vec<String> {
        let dir = std::env::temp_dir().join(format!("tm-bundle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        std::fs::write(
            dir.join("logs/termenbox.jsonl"),
            "{\"fields\":{\"event\":\"start\",\"peer\":\"198.51.100.7:4000\"}}\n",
        )
        .unwrap();
        let auth =
            Arc::new(AuthState::new(dir.clone(), CertKeys::default(), "srv".into()).unwrap());
        let owner = |id: &str| Identity {
            provider: "github".into(),
            id: id.into(),
            login: format!("login-{id}"),
            issued_at: None,
        };
        let claim = auth.claim_token().unwrap().unwrap();
        auth.authorize(&owner("1"), Some(&claim)).unwrap();
        let mgr = WorkspaceManager::new(
            Arc::new(Names(Mutex::new("first".into()))),
            RunMode::Container,
            Arc::new(crate::registry::Registry::memory()),
        );
        let (link, mut end) = crate::link::pair(None, None);
        let me = owner(who);
        let a = auth.clone();
        tokio::spawn(async move {
            let _ = mgr.serve(link, me, &a).await;
        });
        end.to_workspace
            .send(Inbound::Json(r#"{"type":"diagnostics_bundle"}"#.into()))
            .await
            .unwrap();
        let mut seen = Vec::new();
        let _ = tokio::time::timeout(Duration::from_millis(600), async {
            while let Some(m) = end.from_workspace.recv().await {
                seen.push(m);
            }
        })
        .await;
        let _ = std::fs::remove_dir_all(&dir);
        seen
    }

    #[tokio::test]
    async fn the_owner_gets_a_scrubbed_bundle_and_anyone_else_an_error() {
        let told = bundle_replies("1").await;
        let parts: Vec<&String> = told
            .iter()
            .filter(|m| m.contains("\"type\":\"diagnostics_bundle\""))
            .collect();
        assert!(!parts.is_empty(), "no bundle in {told:?}");
        let all = parts.iter().map(|m| m.as_str()).collect::<String>();
        assert!(all.contains("termenbox server diagnostics"), "{all}");
        assert!(!all.contains("198.51.100.7"), "{all}");
        let other = bundle_replies("2").await;
        assert!(
            other.iter().all(|m| !m.contains("diagnostics_bundle")),
            "{other:?}"
        );
        assert!(
            other.iter().any(|m| m.contains("\"type\":\"error\"")),
            "{other:?}"
        );
    }
}

#[cfg(test)]
mod scrollback_tests {
    use super::*;

    fn texts(parts: &[ServerMessage]) -> Vec<(u32, bool, String)> {
        parts
            .iter()
            .map(|m| match m {
                ServerMessage::Scrollback {
                    part, last, text, ..
                } => (*part, *last, text.clone()),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn history_is_cut_into_parts_that_fit_one_message_and_join_back_to_the_text() {
        let text: String = (0..5000).map(|i| format!("line number {i}\n")).collect();
        let parts = texts(&scrollback_parts("s", &text));
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|(_, _, t)| t.len() <= SCROLLBACK_PART));
        assert_eq!(
            parts.iter().map(|(_, _, t)| t.as_str()).collect::<String>(),
            text
        );
        assert_eq!(
            parts.iter().map(|(p, _, _)| *p).collect::<Vec<_>>(),
            (0..parts.len() as u32).collect::<Vec<_>>()
        );
        assert_eq!(parts.iter().filter(|(_, last, _)| *last).count(), 1);
        assert!(parts.last().unwrap().1);
        // A part cut at a line end starts with a whole line.
        assert!(parts[1].2.starts_with("line number "));
    }

    #[test]
    fn an_empty_history_is_one_empty_last_part_and_a_very_long_line_is_cut_at_a_character() {
        assert_eq!(
            texts(&scrollback_parts("s", "")),
            vec![(0, true, String::new())]
        );
        let long = "é".repeat(SCROLLBACK_PART);
        let parts = texts(&scrollback_parts("s", &long));
        assert!(parts.len() >= 2 && parts.iter().all(|(_, _, t)| t.len() <= SCROLLBACK_PART));
        assert_eq!(
            parts.iter().map(|(_, _, t)| t.as_str()).collect::<String>(),
            long
        );
    }

    #[test]
    fn a_reply_part_is_far_below_the_limit_of_the_direct_transport() {
        let text = "x".repeat(SCROLLBACK_PART);
        let msg = &scrollback_parts("workspace-with-a-long-name", &text)[0];
        assert!(serde_json::to_string(msg).unwrap().len() < 64 * 1024);
    }
}

#[cfg(test)]
mod update_change_tests {
    use super::*;

    #[test]
    fn a_connection_is_told_once_when_the_recorded_release_changes_or_clears() {
        let mut sent = String::new();
        assert_eq!(update_change(&mut sent, String::new()), None);
        assert_eq!(
            update_change(&mut sent, "0.1.54".into()),
            Some(ServerMessage::UpdateStatus {
                available: "0.1.54".into()
            })
        );
        assert_eq!(update_change(&mut sent, "0.1.54".into()), None);
        assert_eq!(
            update_change(&mut sent, String::new()),
            Some(ServerMessage::UpdateStatus {
                available: String::new()
            })
        );
    }
}

#[cfg(test)]
mod replay_message_tests {
    use super::*;
    use crate::event_log::Replay;
    use serde_json::json;

    fn replay(frames: Vec<serde_json::Value>, reset: bool) -> Replay {
        Replay {
            epoch: 5,
            last: 99,
            reset,
            frames,
        }
    }

    fn frame(n: usize) -> serde_json::Value {
        json!({"sessionId": "a", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "x".repeat(n)}}})
    }

    #[test]
    fn replay_a_big_replay_is_cut_into_messages_the_direct_transport_carries_and_only_the_first_resets(
    ) {
        let frames: Vec<_> = (0..30).map(|_| frame(5_000)).collect();
        let msgs = events_messages("w", replay(frames, true), false);
        assert!(msgs.len() >= 4, "{} messages", msgs.len());
        let mut total = 0;
        for (i, m) in msgs.iter().enumerate() {
            let ServerMessage::AcpEvents {
                reset,
                frames,
                last,
                epoch,
                ..
            } = m
            else {
                panic!()
            };
            assert_eq!(*reset, i == 0);
            assert_eq!((*last, *epoch), (99, 5));
            assert!(serde_json::to_string(m).unwrap().len() < 48 * 1024);
            total += frames.len();
        }
        assert_eq!(total, 30);
    }

    #[test]
    fn replay_nothing_to_say_is_silent_for_a_live_tail_and_one_message_for_an_open_or_a_reset() {
        assert!(events_messages("w", replay(vec![], false), false).is_empty());
        assert_eq!(events_messages("w", replay(vec![], false), true).len(), 1);
        let reset = events_messages("w", replay(vec![], true), false);
        assert!(
            matches!(&reset[..], [ServerMessage::AcpEvents { reset: true, frames, .. }] if frames.is_empty())
        );
    }
}
