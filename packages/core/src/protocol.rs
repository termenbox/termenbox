use serde::{Deserialize, Serialize};
use std::fmt::Display;

/// Client → Server (WebSocket)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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
    /// A message from a newer client; ignored.
    #[serde(other)]
    Unknown,
}

/// Server → Client (WebSocket)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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
    Error {
        message: String,
    },
    Pong,
    /// A message from a newer server; ignored.
    #[serde(other)]
    Unknown,
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
        })
        .unwrap();
        assert_eq!(v["type"], "auth_ok");
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
