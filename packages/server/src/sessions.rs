use std::time::Duration;

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use tracing::{debug, info};

use termenbox_core::protocol::{ClientMessage, ServerMessage};

use std::sync::Arc;

use crate::backend::{RunMode, SessionBackend};
use crate::control::WindowHub;
use crate::events::EventHub;
use crate::link::{Inbound, Link, LinkTx};
use crate::pty::Attachment;

/// Clients ping every 5 s, so a minute of silence means the peer is gone.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Serves one authenticated WebSocket: lists/creates tmux sessions and bridges one attached session.
/// Persistence lives in the session backend (tmux by default); a dropped connection only detaches (see pty.rs).
#[derive(Clone)]
pub struct SessionManager {
    backend: Arc<dyn SessionBackend>,
    mode: RunMode,
    server_name: String,
    /// Called after an uninstall has wiped the configuration and told the client; stops the process.
    on_uninstalled: Arc<dyn Fn() + Send + Sync>,
    events: Option<EventHub>,
    push: Option<Arc<crate::push::Push>>,
    windows: Option<WindowHub>,
    /// tmux version text and whether control mode runs (plan H.3); reported in `auth_ok`.
    tmux_version: String,
    control_mode: bool,
    prefix: String,
    /// Bounded readers of the transcripts hooks named (tier 1, decision 0103).
    transcripts: Arc<crate::transcript::Transcripts>,
    /// Where the owner can enable Claude Code's hooks from the app (U10).
    hooks: Option<crate::claude_hooks::Target>,
}

impl SessionManager {
    pub fn new(backend: Arc<dyn SessionBackend>, mode: RunMode) -> Self {
        Self {
            backend,
            mode,
            server_name: String::new(),
            on_uninstalled: Arc::new(|| {}),
            events: None,
            push: None,
            windows: None,
            tmux_version: String::new(),
            control_mode: false,
            prefix: String::new(),
            transcripts: Arc::default(),
            hooks: None,
        }
    }

    /// The transcript timeline for a session whose hooks named one (tier 1). The path is the hook's, so it is
    /// opened only if it resolves under the user's Claude config directory; the read is bounded and runs off the
    /// async threads. Nothing from it is logged.
    async fn timeline(&self, session: &str) -> Option<termenbox_core::protocol::Timeline> {
        let path = self.events.as_ref()?.transcript(session)?;
        let root = crate::transcript::claude_root()?;
        let readers = self.transcripts.clone();
        let session = session.to_string();
        tokio::task::spawn_blocking(move || readers.timeline(&root, &session, &path))
            .await
            .ok()
            .flatten()
    }

    /// Lets the owner enable and remove Claude Code's hooks from the app (U10); without it the state is `unavailable`.
    pub fn with_claude_hooks(mut self, target: crate::claude_hooks::Target) -> Self {
        self.hooks = Some(target);
        self
    }

    /// The session managers a client may pick from when it creates a session.
    pub fn backends(&self) -> Vec<String> {
        self.backend.kinds()
    }

    /// Serves the agent event stream (plan v6) to every connection.
    pub fn with_events(mut self, hub: EventHub) -> Self {
        self.events = Some(hub);
        self
    }

    /// Lets devices subscribe to Web Push and tell which session they show (plan A.6).
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
        let mut window_rx = self.windows.as_ref().map(|h| h.subscribe());
        let mut status_rx = self.events.as_ref().map(|h| h.subscribe_status());

        loop {
            tokio::select! {
                msg = link.rx.recv() => {
                    let Some(msg) = msg else { break };
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
                    let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) else { continue };
                    if let (true, ClientMessage::Attach { session_id }) = (self.mode.is_host(), &client_msg) {
                        // Who got a shell on the host and as whom; never what they typed.
                        info!(target: "audit", event = "host_session", provider = %identity.provider, account_id = %identity.id, login = %identity.login, run_as = %self.mode.label(), session = %session_id);
                    }
                    if let Err(e) = self.handle(conn, client_msg, auth, &identity, &mut attachment, &mut cols, &mut rows, &link.tx).await {
                        if !send(&link.tx, &ServerMessage::Error { message: e.to_string() }).await { break; }
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
                            // The attach client exited: session killed, or detached from inside.
                            attachment = None;
                            if !send(&link.tx, &ServerMessage::Error { message: "Session ended".into() }).await { break; }
                            let sessions = self.backend.list().await.unwrap_or_default();
                            if !send(&link.tx, &ServerMessage::Sessions { sessions }).await { break; }
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
                update = next_window(&mut window_rx) => {
                    let msg = match update {
                        Some(u) => window_message(u),
                        None => {
                            // Fell behind: send everything again.
                            for u in self.windows.as_ref().map(|h| h.all()).unwrap_or_default() {
                                if !send(&link.tx, &window_message(u)).await { return Ok(()); }
                            }
                            continue;
                        }
                    };
                    if !send(&link.tx, &msg).await { break; }
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

    /// Every session's Activity and the open Attention items, for a client that is new or fell behind.
    fn status_messages(&self) -> Vec<ServerMessage> {
        let Some(hub) = &self.events else {
            return Vec::new();
        };
        vec![
            ServerMessage::Activity {
                sessions: hub.activities(),
            },
            ServerMessage::Attention {
                items: hub.attention_items(),
            },
        ]
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
        tx: &LinkTx,
    ) -> Result<()> {
        match msg {
            // The handshake happens once, before this loop (connection.rs).
            ClientMessage::Hello | ClientMessage::Auth { .. } => {}
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
            ClientMessage::RevokeDevice { device } => {
                auth.revoke_device(&device)?;
                send_r(tx, &ServerMessage::DeviceRevoked { device }).await?;
            }
            ClientMessage::Ping => send_r(tx, &ServerMessage::Pong).await?,
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
                        },
                    )
                    .await?;
                }
            }
            ClientMessage::Foreground { session } => {
                if let Some(p) = &self.push {
                    p.set_foreground(conn, session);
                }
            }
            ClientMessage::Ack { attention_id } | ClientMessage::Dismiss { attention_id } => {
                if let Some(hub) = &self.events {
                    hub.ack(&attention_id);
                }
            }
            ClientMessage::Answer {
                session,
                prompt_id,
                seq,
                choice,
            } => {
                let hub = self
                    .events
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Answers are not available"))?;
                answer(
                    self.backend.as_ref(),
                    hub,
                    &session,
                    &prompt_id,
                    seq,
                    choice,
                )
                .await?;
                info!(target: "audit", event = "answer_sent", provider = %identity.provider, account_id = %identity.id, session = %session, choice);
            }
            ClientMessage::Reply { session, text } => {
                // One line, no control characters: it is typed, then Enter.
                let line: String = text.chars().filter(|c| !c.is_control()).take(500).collect();
                if line.trim().is_empty() {
                    return Ok(());
                }
                self.backend
                    .send_text(&session, &format!("{line}\r"))
                    .await?;
                info!(target: "audit", event = "reply_sent", provider = %identity.provider, account_id = %identity.id, session = %session);
            }
            ClientMessage::AgentSync { after } => {
                let Some(hub) = &self.events else {
                    return Ok(());
                };
                let states = hub.snapshot();
                let mut events = Vec::new();
                for st in &states {
                    if let Some(seq) = after.get(&st.session) {
                        events.extend(hub.since(&st.session, *seq));
                    }
                }
                send_r(tx, &ServerMessage::AgentSnapshot { states }).await?;
                for m in self.status_messages() {
                    send_r(tx, &m).await?;
                }
                if !events.is_empty() {
                    send_r(tx, &ServerMessage::AgentEvents { events }).await?;
                }
                // A tab speaks for one window of its session: send them all along with the agent states.
                for u in self.windows.as_ref().map(|h| h.all()).unwrap_or_default() {
                    send_r(tx, &window_message(u)).await?;
                }
            }
            ClientMessage::Windows { session } => {
                let windows = self.backend.windows(&session).await?;
                send_r(
                    tx,
                    &ServerMessage::WindowList {
                        session,
                        representative: crate::control::representative(&windows),
                        windows: windows.into_iter().map(|w| w.info).collect(),
                    },
                )
                .await?;
            }
            ClientMessage::SelectWindow { session, index } => {
                self.backend.select_window(&session, index).await?;
            }
            ClientMessage::NewWindow { session } => {
                info!(target: "audit", event = "new_window", provider = %identity.provider, account_id = %identity.id, session = %session);
                self.backend.new_window(&session).await?;
            }
            ClientMessage::CloseWindow { session, index } => {
                info!(target: "audit", event = "close_window", provider = %identity.provider, account_id = %identity.id, session = %session, window = index);
                self.backend.kill_window(&session, index).await?;
                // Closing the last window ends the session: the lists follow.
                let sessions = self.backend.list().await?;
                send_r(tx, &ServerMessage::Sessions { sessions }).await?;
            }
            ClientMessage::CloseSession { session } => {
                info!(target: "audit", event = "close_session", provider = %identity.provider, account_id = %identity.id, login = %identity.login, session = %session);
                self.backend.kill(&session).await?;
                let sessions = self.backend.list().await?;
                send_r(tx, &ServerMessage::Sessions { sessions }).await?;
            }
            ClientMessage::Peek { session, hash } => {
                // Only a counter in the audit log: what is on the screen is never logged.
                audit_peek(&identity.provider, &identity.id, &session);
                let window = self
                    .windows
                    .as_ref()
                    .and_then(|h| h.representative(&session));
                let reply =
                    peek_window(self.backend.as_ref(), &session, hash.as_deref(), window).await;
                send_r(tx, &reply).await?;
            }
            ClientMessage::SessionView { session, hash } => {
                // Only a counter in the audit log, as for a peek: the screen is never logged.
                audit_peek(&identity.provider, &identity.id, &session);
                let agent = self
                    .events
                    .as_ref()
                    .and_then(|h| session_agent(h, &session));
                let timeline = self.timeline(&session).await;
                let reply = session_view(
                    self.backend.as_ref(),
                    &session,
                    agent.as_deref(),
                    hash.as_deref(),
                    timeline,
                )
                .await;
                send_r(tx, &reply).await?;
            }
            ClientMessage::SessionAnswer {
                session,
                prompt_id,
                option,
            } => {
                let agent = self
                    .events
                    .as_ref()
                    .and_then(|h| session_agent(h, &session));
                session_answer(
                    self.backend.as_ref(),
                    &session,
                    agent.as_deref(),
                    &prompt_id,
                    option,
                )
                .await?;
                info!(target: "audit", event = "session_answer_sent", provider = %identity.provider, account_id = %identity.id, session = %session, option);
            }
            ClientMessage::AgentHooks { agent, action } => {
                let target = self.hooks.clone();
                let (a, act) = (agent.clone(), action.clone());
                let reply = tokio::task::spawn_blocking(move || {
                    crate::claude_hooks::respond(target.as_ref(), &a, &act)
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
            ClientMessage::Unknown => {}
            ClientMessage::ListSessions => {
                let sessions = self.backend.list().await?;
                send_r(tx, &ServerMessage::Sessions { sessions }).await?;
            }
            ClientMessage::NewSession {
                name,
                backend,
                dir,
                template,
            } => {
                self.backend
                    .create_with(
                        &name,
                        backend.as_deref(),
                        dir.as_deref(),
                        template.as_deref(),
                    )
                    .await?;
                let sessions = self.backend.list().await?;
                send_r(tx, &ServerMessage::Sessions { sessions }).await?;
            }
            ClientMessage::Attach { session_id } => {
                *attachment = None; // detach the previous one first
                                    // Land in the window the tab showed, unless somebody else is looking at the session.
                if let Some(index) = self
                    .windows
                    .as_ref()
                    .and_then(|h| h.representative(&session_id))
                {
                    let _ = self.backend.focus_window(&session_id, index).await;
                }
                *attachment = Some(self.backend.attach(&session_id, *cols, *rows)?);
                send_r(tx, &ServerMessage::SessionAttached { session_id }).await?;
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

async fn send(tx: &LinkTx, msg: &ServerMessage) -> bool {
    tx.send(serde_json::to_string(msg).unwrap()).await
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

fn status_message(c: crate::events::StatusChange) -> ServerMessage {
    match c {
        crate::events::StatusChange::Activity(a) => ServerMessage::Activity { sessions: vec![a] },
        crate::events::StatusChange::Attention(items) => ServerMessage::Attention { items },
    }
}

fn window_message(u: crate::control::WindowUpdate) -> ServerMessage {
    ServerMessage::WindowList {
        session: u.session,
        windows: u.windows,
        representative: u.representative,
    }
}

/// The next change to any session's windows (`None` when this connection fell behind); pends forever when
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

fn audit_peek(provider: &str, account_id: &str, session: &str) {
    info!(target: "audit", event = "peek", provider, account_id, session = %session);
}

/// The most a snapshot carries; a phone screen is a few KB, this only stops a huge pane from being sent.
pub const SNAPSHOT_MAX: usize = 16 * 1024;

/// A session's visible screen for the client to draw at once (decision 0084). Never fails: a manager that cannot
/// capture (a `command` manager) gets an empty snapshot, which the client reads as "attach normally".
#[cfg(test)]
pub async fn peek(
    backend: &dyn SessionBackend,
    session: &str,
    known: Option<&str>,
) -> ServerMessage {
    peek_window(backend, session, known, None).await
}

/// `peek` of one window: the one the session's tab speaks for, so the snapshot shows what the tab promised.
pub async fn peek_window(
    backend: &dyn SessionBackend,
    session: &str,
    known: Option<&str>,
    window: Option<u32>,
) -> ServerMessage {
    use sha2::{Digest, Sha256};
    let none = || ServerMessage::Snapshot {
        session: session.to_string(),
        text: String::new(),
        hash: String::new(),
        unchanged: false,
    };
    let captured = match window {
        Some(i) => backend.capture_window_styled(session, i).await,
        None => backend.capture_styled(session).await,
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
            session: session.to_string(),
            text: String::new(),
            hash,
            unchanged: true,
        };
    }
    ServerMessage::Snapshot {
        session: session.to_string(),
        text,
        hash,
        unchanged: false,
    }
}

const PROMPT_CHANGED: &str = "That prompt changed";

/// The agent the event layer found in a session, if any.
fn session_agent(hub: &EventHub, session: &str) -> Option<String> {
    hub.snapshot()
        .into_iter()
        .find(|s| s.session == session)
        .and_then(|s| s.agent)
}

/// The session view (decision 0103): the screen read by the agent's profile (T2) or the generic rules (T3). Never
/// fails; a manager that cannot capture gives an empty tail, which the client shows with the full-terminal button.
pub async fn session_view(
    backend: &dyn SessionBackend,
    session: &str,
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
    let screen = backend.capture(session).await.unwrap_or_default();
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
        session: session.to_string(),
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
    backend: &dyn SessionBackend,
    session: &str,
    agent: Option<&str>,
    prompt_id: &str,
    option: u8,
) -> Result<()> {
    let (profile, _) = crate::profile::Profiles::builtin().for_agent(agent);
    let screen = backend.capture(session).await?;
    let prompt = profile
        .read(&screen)
        .prompt
        .filter(|p| p.id() == prompt_id)
        .ok_or_else(|| anyhow::anyhow!(PROMPT_CHANGED))?;
    let keys = prompt
        .keys(option as usize)
        .ok_or_else(|| anyhow::anyhow!("No such choice"))?;
    backend.send_text(session, keys).await
}

/// A one-tap answer (plan v8, K.4). It acts only on the prompt the user saw: the session's current prompt, with
/// the same id and event number, confirmed by the agent's own hook (`confidence=high`), and still on the pane.
/// The keys are the ones the screen's own choice list defines, never text from the client.
pub async fn answer(
    backend: &dyn SessionBackend,
    hub: &EventHub,
    session: &str,
    prompt_id: &str,
    seq: u64,
    choice: u8,
) -> Result<()> {
    use termenbox_core::events::{Confidence, EventKind};
    let current = hub
        .current_prompt(session)
        .ok_or_else(|| anyhow::anyhow!(PROMPT_CHANGED))?;
    let EventKind::Prompt { id, .. } = &current.kind else {
        return Err(anyhow::anyhow!(PROMPT_CHANGED));
    };
    if id != prompt_id || current.seq != seq {
        return Err(anyhow::anyhow!(PROMPT_CHANGED));
    }
    if current.confidence != Confidence::High {
        return Err(anyhow::anyhow!(
            "This prompt is not confirmed by the agent; open the terminal to answer"
        ));
    }
    let screen = backend.capture(session).await?;
    let info = crate::adapters::parse_prompt(&screen)
        .filter(|i| id.ends_with(&crate::agentwatch::hex8(&i.question)))
        .ok_or_else(|| anyhow::anyhow!(PROMPT_CHANGED))?;
    let (_, keys) = info
        .choices
        .get(choice as usize)
        .ok_or_else(|| anyhow::anyhow!("No such choice"))?;
    backend.send_text(session, keys).await
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
    impl SessionBackend for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        async fn list(&self) -> Result<Vec<termenbox_core::protocol::SessionInfo>> {
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
            other => panic!("not a session view: {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_session_view_lists_labels_only_and_a_tap_sends_the_profiles_keys() {
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
    async fn a_session_answer_for_another_prompt_or_option_sends_nothing() {
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
    impl SessionBackend for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        async fn list(&self) -> Result<Vec<termenbox_core::protocol::SessionInfo>> {
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

    /// The audit line the handler writes names the session and nothing from the screen.
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
}
