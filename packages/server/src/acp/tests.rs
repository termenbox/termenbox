//! Tests against a scripted fake ACP agent. The fake is this test binary started again on `fake_agent` (no
//! other program is needed); `FAKE_MODE` picks what it can do and `FAKE_LOG` records what it received.
use super::*;
use std::io::{BufRead, Write};
use std::path::Path;
use termenbox_core::events::AgentEvent;
use termenbox_core::protocol::{AcpBody, AcpView};

// ---------------------------------------------------------------------------------------------------
// The fake agent
// ---------------------------------------------------------------------------------------------------

fn put(v: Value) {
    let mut o = std::io::stdout().lock();
    writeln!(o, "{v}").unwrap();
    o.flush().unwrap();
}

fn update(u: Value) {
    put(
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "acp-1", "update": u}}),
    );
}

fn fake_prompt(
    text: &str,
    lines: &mut impl Iterator<Item = String>,
    log: &dyn Fn(&Value),
) -> Value {
    match text {
        "updates" => {
            update(
                json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "hmm "}}),
            );
            update(
                json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "ok"}}),
            );
            update(
                json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Hello "}}),
            );
            update(
                json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "world"}}),
            );
            update(
                json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Run ls\nsecond line", "kind": "execute", "status": "pending", "rawInput": {"command": "SECRET-RAW"}}),
            );
            update(
                json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": "SECRET-OUTPUT"}}]}),
            );
            update(
                json!({"sessionUpdate": "plan", "entries": [{"content": "step one", "priority": "medium", "status": "in_progress"}]}),
            );
            update(
                json!({"sessionUpdate": "usage_update", "used": 500, "size": 1000, "cost": {"amount": 0.5, "currency": "USD"}}),
            );
            update(json!({"sessionUpdate": "session_info_update", "title": "My title"}));
            update(json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"}));
            update(
                json!({"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "compact", "description": "x"}]}),
            );
            json!({"stopReason": "end_turn"})
        }
        "perm" => {
            put(
                json!({"jsonrpc": "2.0", "id": 100, "method": "session/request_permission", "params": {
                "sessionId": "acp-1",
                "toolCall": {"toolCallId": "t2", "title": "Write a.txt", "kind": "edit"},
                "options": [
                    {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
                    {"optionId": "allow-with-updates", "name": "Always", "kind": "allow_always"},
                    {"optionId": "reject", "name": "No", "kind": "reject_once"},
                    {"optionId": "reject-forever", "name": "Never", "kind": "reject_always"}
                ]}}),
            );
            for line in lines {
                let Ok(m) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                log(&m);
                if m["id"] == 100 {
                    let outcome = m["result"]["outcome"]["outcome"].as_str().unwrap_or("");
                    return json!({"stopReason": if outcome == "selected" { "end_turn" } else { "cancelled" }});
                }
                if m["method"] == "session/cancel" {
                    // A real agent drops the open request and ends the turn.
                    return json!({"stopReason": "cancelled"});
                }
            }
            json!({"stopReason": "cancelled"})
        }
        "crash" => std::process::exit(3),
        "hostile" => {
            let evil = format!(
                "\u{1b}[31mred\u{1b}[0m ghp_{} {}",
                "a".repeat(40),
                "x".repeat(100_000)
            );
            update(
                json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": evil}}),
            );
            update(
                json!({"sessionUpdate": "session_info_update", "title": format!("\u{7}bell {}", "t".repeat(5000))}),
            );
            // One line far over the cap, then a normal one.
            println!("{{\"junk\":\"{}\"}}", "z".repeat(2 << 20));
            json!({"stopReason": "end_turn"})
        }
        _ => json!({"stopReason": "end_turn"}),
    }
}

/// Run by the backend as the "agent"; a plain test run skips it.
#[test]
fn fake_agent() {
    let Ok(mode) = std::env::var("FAKE_MODE") else {
        return;
    };
    // The harness prints `test name ... ` without a newline; the first JSON line must start its own line.
    println!();
    let log_path = std::env::var("FAKE_LOG").unwrap();
    let log = |v: &Value| {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .unwrap();
        writeln!(f, "{v}").unwrap();
    };
    let mut lines = std::io::stdin().lock().lines().map_while(|l| l.ok());
    while let Some(line) = lines.next() {
        let Ok(m) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        log(&m);
        let id = m["id"].clone();
        let reply = |r: Value| put(json!({"jsonrpc": "2.0", "id": id, "result": r}));
        match m["method"].as_str().unwrap_or("") {
            "initialize" => reply(json!({"protocolVersion": 1, "agentCapabilities": {
                "loadSession": mode.contains("load"),
                "sessionCapabilities": if mode.contains("resume") { json!({"resume": {}}) } else { json!({}) }
            }})),
            "session/new" if mode.contains("authfail") => put(json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32000, "message": "Authentication required"}})),
            "session/new" if mode.contains("modes") => reply(json!({"sessionId": "acp-1",
                "modes": {"currentModeId": "ask", "availableModes": [
                    {"id": "ask", "name": "Ask", "description": "Ask first"}, {"id": "plan", "name": "Plan"}]}})),
            "session/new" => reply(json!({"sessionId": "acp-1"})),
            "session/set_mode" => reply(json!({})),
            "session/resume" => reply(json!({})),
            "session/load" => {
                update(
                    json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "REPLAYED"}}),
                );
                reply(json!({}))
            }
            "session/prompt" => {
                let text = m["params"]["prompt"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let r = fake_prompt(&text, &mut lines, &log);
                reply(r);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------

struct Rig {
    backend: AcpBackend,
    hub: EventHub,
    log: PathBuf,
    dir: PathBuf,
}

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("tbx-acp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn profile(mode: &str, log: &Path) -> AgentProfile {
    AgentProfile {
        command: std::env::current_exe().unwrap().display().to_string(),
        args: [
            "acp::tests::fake_agent",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ]
        .map(String::from)
        .into(),
        env: HashMap::from([
            ("FAKE_MODE".to_string(), mode.to_string()),
            ("FAKE_LOG".to_string(), log.display().to_string()),
        ]),
        env_remove: vec![],
        elicitation_form: false,
    }
}

fn rig_with(mode: &str, max: usize, dir: PathBuf, state: bool) -> Rig {
    let log = dir.join(format!("{}.log", uuid::Uuid::new_v4()));
    let config = AcpConfig {
        max_sessions: max,
        builtin: false,
        agents: HashMap::from([("fake".to_string(), profile(mode, &log))]),
    };
    let hub = EventHub::new([7; 32]);
    let mut backend = AcpBackend::new(config).with_events(hub.clone());
    if state {
        backend = backend.with_state_file(dir.join("sessions.json"));
    }
    Rig {
        backend,
        hub,
        log,
        dir,
    }
}

fn rig(mode: &str) -> Rig {
    rig_with(mode, 2, tmp(), false)
}

impl Rig {
    async fn create(&self, name: &str) {
        self.backend
            .create_with(name, None, self.dir.to_str(), Some("fake"))
            .await
            .unwrap();
    }
    fn events(&self, name: &str) -> Vec<AgentEvent> {
        self.hub.since(name, 0)
    }
    fn kinds(&self, name: &str) -> Vec<EventKind> {
        self.events(name).into_iter().map(|e| e.kind).collect()
    }
    fn logged(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
    async fn prompt(&self, name: &str, text: &str) {
        self.backend.send_text(name, text).await.unwrap();
    }
}

async fn until(what: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

fn last_state(r: &Rig, name: &str) -> Option<AgentState> {
    r.kinds(name).into_iter().rev().find_map(|k| match k {
        EventKind::State { state } => Some(state),
        _ => None,
    })
}

// ---------------------------------------------------------------------------------------------------
// Done-when
// ---------------------------------------------------------------------------------------------------

#[test]
fn the_allowlist_file_parses_and_refuses_nonsense() {
    let c = AcpConfig::parse(
        "max_sessions = 3\n[agents.claude]\ncommand = \"claude-agent-acp\"\nenv_remove = [\"CLAUDECODE\"]\n",
    )
    .unwrap();
    assert_eq!(c.max_sessions, 3);
    assert_eq!(c.agents["claude"].env_remove, ["CLAUDECODE"]);
    assert!(AcpConfig::parse("max_sessions = 0").is_err());
    assert!(AcpConfig::parse("[agents.\"a b\"]\ncommand = \"x\"").is_err());
    assert!(AcpConfig::parse("[agents.x]\ncommand = \"\"").is_err());
}

#[test]
fn no_file_means_the_built_ins_and_a_file_is_exclusive_unless_it_asks() {
    let none = AcpConfig::load(Path::new("/nonexistent/acp.toml")).unwrap();
    assert_eq!(none, AcpConfig::default());
    assert_eq!(none.agents, builtin_agents());
    assert_eq!(none.max_sessions, 6);

    let only = AcpConfig::parse("[agents.mine]\ncommand = \"mine\"").unwrap();
    assert_eq!(only.agents.keys().collect::<Vec<_>>(), ["mine"]);
    assert_eq!(only.max_sessions, 6);

    let both =
        AcpConfig::parse("builtin = true\nmax_sessions = 3\n[agents.mine]\ncommand = \"mine\"")
            .unwrap();
    assert_eq!(both.max_sessions, 3);
    let mut names: Vec<_> = both.agents.keys().cloned().collect();
    names.sort();
    assert_eq!(names, ["claude", "mine", "opencode"]);

    // A same-named entry in the file wins over the built-in.
    let over =
        AcpConfig::parse("builtin = true\n[agents.claude]\ncommand = \"my-claude\"").unwrap();
    assert_eq!(over.agents["claude"].command, "my-claude");
    assert!(over.agents["claude"].env_remove.is_empty());
    assert!(over.agents.contains_key("opencode"));

    // An old file without the new keys parses unchanged.
    let old = AcpConfig::parse(
        "max_sessions = 2\n[agents.claude]\ncommand = \"claude-agent-acp\"\nenv_remove = [\"CLAUDECODE\"]\n",
    )
    .unwrap();
    assert!(!old.builtin && !old.agents["claude"].elicitation_form);
    assert_eq!(old.agents.len(), 1);
}

#[test]
fn each_built_in_matches_the_probe_script() {
    let probe = include_str!("../../../../scripts/acp-probe.mjs");
    let b = builtin_agents();
    assert!(probe.contains("opencode: ['opencode', 'acp']"));
    assert_eq!(
        (b["opencode"].command.as_str(), &b["opencode"].args[..]),
        ("opencode", &["acp".to_string()][..])
    );
    assert!(b["opencode"].elicitation_form);
    assert!(probe.contains("claude: [`${NM}/claude-agent-acp`]"));
    assert_eq!(b["claude"].command, "claude-agent-acp");
    assert!(b["claude"].args.is_empty());
    assert_eq!(b["claude"].env_remove, ["CLAUDECODE"]);
}

#[test]
fn on_path_finds_executables_in_path_and_by_absolute_path() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp();
    let exe = d.join("tbx-fake-adapter");
    let plain = d.join("tbx-not-exec");
    std::fs::write(&exe, "#!/bin/sh\n").unwrap();
    std::fs::write(&plain, "x").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(on_path(exe.to_str().unwrap()));
    assert!(!on_path(plain.to_str().unwrap()));
    assert!(!on_path(d.join("missing").to_str().unwrap()));
    assert!(
        !on_path(d.to_str().unwrap()),
        "a directory is not a command"
    );

    // PATH is process-wide: this is the only test that changes it.
    let old = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<_> = std::env::split_paths(&old).collect();
    paths.push(d.clone());
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    assert!(on_path("tbx-fake-adapter"));
    assert!(!on_path("tbx-not-exec"));
    assert!(!on_path("tbx-never-installed"));
    std::env::set_var("PATH", old);
}

#[tokio::test]
async fn an_adapter_installed_later_shows_up_and_a_missing_one_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp();
    let cmd = d.join("tbx-late-adapter");
    let config = AcpConfig {
        max_sessions: 2,
        builtin: false,
        agents: HashMap::from([(
            "late".to_string(),
            AgentProfile {
                command: cmd.display().to_string(),
                args: vec![],
                env: HashMap::new(),
                env_remove: vec![],
                elicitation_form: false,
            },
        )]),
    };
    let backend = AcpBackend::new(config);
    assert!(backend.acp_agents().is_empty());
    let e = backend
        .create_with("x", None, d.to_str(), Some("late"))
        .await
        .unwrap_err();
    assert_eq!(e.to_string(), "That agent is not installed on this server");

    std::fs::write(&cmd, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&cmd, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(backend.acp_agents(), ["late"]);
}

#[test]
fn elicitation_is_advertised_only_when_the_profile_asks() {
    let off = initialize_params(&initialize_request(false));
    assert!(off["clientCapabilities"].get("elicitation").is_none());
    let on = initialize_params(&initialize_request(true));
    assert_eq!(on["clientCapabilities"]["elicitation"], json!({"form": {}}));
}

#[tokio::test]
async fn a_command_outside_the_allowlist_is_refused_and_nothing_is_started() {
    let r = rig("");
    for bad in [Some("sh"), Some("/bin/sh -c id"), Some("../fake"), None] {
        let e = r
            .backend
            .create_with("x", None, r.dir.to_str(), bad)
            .await
            .unwrap_err();
        assert!(e.to_string().contains("Unknown agent"), "{e}");
    }
    assert!(r.backend.list().await.unwrap().is_empty());
    assert!(r.logged().is_empty(), "no child was started");
    assert!(r.backend.create("x").await.is_err());
}

#[test]
fn every_update_kind_maps_to_an_additive_event() {
    let mut m = UpdateMapper::default();
    let mut all = Vec::new();
    for u in [
        json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "think"}}),
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "a"}}),
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "b"}}),
        json!({"sessionUpdate": "tool_call", "toolCallId": "1", "title": "Run ls\nmore", "kind": "execute", "rawInput": {"c": "RAW"}}),
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "1", "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": "OUT"}}]}),
        json!({"sessionUpdate": "plan", "entries": [{"content": "p", "priority": "low", "status": "pending"}]}),
        json!({"sessionUpdate": "usage_update", "used": 1, "size": 4, "cost": {"amount": 2, "currency": "USD"}}),
        json!({"sessionUpdate": "session_info_update", "title": "T"}),
        json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"}),
        json!({"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "compact"}]}),
        json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "ignored"}}),
        json!({"sessionUpdate": "something_new"}),
    ] {
        all.extend(m.map(&u));
    }
    all.extend(m.flush());
    assert_eq!(
        all,
        vec![
            EventKind::Thought {
                summary: "think".into()
            },
            EventKind::Message {
                summary: "ab".into()
            },
            EventKind::Tool {
                name: "execute".into(),
                summary: "Run ls".into(),
                phase: ToolPhase::Started
            },
            EventKind::Tool {
                name: "execute".into(),
                summary: "Run ls".into(),
                phase: ToolPhase::Finished
            },
            EventKind::Plan {
                items: vec![PlanItem {
                    text: "p".into(),
                    status: "pending".into()
                }]
            },
            EventKind::Usage {
                used: 1,
                size: 4,
                cost: Some("2 USD".into())
            },
            EventKind::Title { title: "T".into() },
            EventKind::Mode {
                mode: "plan".into()
            },
            EventKind::Commands {
                names: vec!["compact".into()]
            },
        ]
    );
    // Raw tool input and output never reach an event (decision 0095).
    assert!(!format!("{all:?}").contains("RAW") && !format!("{all:?}").contains("OUT\""));
}

#[tokio::test]
async fn a_session_starts_with_minimal_capabilities_and_streams_every_update_kind() {
    let r = rig("");
    r.create("s1").await;
    let log = r.logged();
    let init = log.iter().find(|m| m["method"] == "initialize").unwrap();
    assert_eq!(init["params"]["clientCapabilities"]["terminal"], false);
    assert_eq!(
        init["params"]["clientCapabilities"]["fs"]["readTextFile"],
        false
    );
    assert_eq!(
        init["params"]["clientCapabilities"]["fs"]["writeTextFile"],
        false
    );
    let new = log.iter().find(|m| m["method"] == "session/new").unwrap();
    assert_eq!(new["params"]["cwd"], r.dir.to_str().unwrap());
    assert_eq!(
        r.backend.list().await.unwrap()[0].agent.as_deref(),
        Some("fake")
    );
    assert!(r.backend.attach("s1", 80, 24).is_err());

    r.prompt("s1", "updates").await;
    until("the turn to finish", || {
        last_state(&r, "s1") == Some(AgentState::Done)
    })
    .await;
    let k = r.kinds("s1");
    let has = |f: &dyn Fn(&EventKind) -> bool| k.iter().any(f);
    assert!(has(
        &|k| matches!(k, EventKind::Thought { summary } if summary == "hmm ok")
    ));
    assert!(has(
        &|k| matches!(k, EventKind::Message { summary } if summary == "Hello world")
    ));
    assert!(has(
        &|k| matches!(k, EventKind::Tool { phase: ToolPhase::Started, summary, .. } if summary == "Run ls")
    ));
    assert!(has(&|k| matches!(
        k,
        EventKind::Tool {
            phase: ToolPhase::Finished,
            ..
        }
    )));
    assert!(has(
        &|k| matches!(k, EventKind::Plan { items } if items[0].text == "step one")
    ));
    assert!(has(&|k| matches!(
        k,
        EventKind::Usage {
            used: 500,
            size: 1000,
            ..
        }
    )));
    assert!(has(
        &|k| matches!(k, EventKind::Title { title } if title == "My title")
    ));
    assert!(has(
        &|k| matches!(k, EventKind::Mode { mode } if mode == "plan")
    ));
    assert!(has(
        &|k| matches!(k, EventKind::Commands { names } if names == &["compact"])
    ));
    assert!(r.events("s1").iter().all(|e| e.source == Source::Acp));
    assert!(!format!("{k:?}").contains("SECRET"));
    r.backend.kill("s1").await.unwrap();
}

#[test]
fn only_one_time_options_are_ever_offered() {
    let p = json!({"options": [
        {"optionId": "a", "name": "A", "kind": "allow_once"},
        {"optionId": "b", "name": "B", "kind": "allow_always"},
        {"optionId": "c", "name": "C", "kind": "reject_once"},
        {"optionId": "d", "name": "D", "kind": "reject_always"},
        {"optionId": "e", "name": "E", "kind": "future_kind"}
    ]});
    let ids: Vec<String> = permission_options(&p).into_iter().map(|o| o.id).collect();
    assert_eq!(ids, ["a", "c"]);
}

#[tokio::test]
async fn a_permission_round_trip_selects_exactly_the_tapped_option() {
    let r = rig("");
    r.create("s1").await;
    r.prompt("s1", "perm").await;
    until("the prompt", || r.hub.current_prompt("s1").is_some()).await;
    assert_eq!(last_state(&r, "s1"), Some(AgentState::NeedsInput));
    let EventKind::Prompt {
        id,
        prompt_kind,
        decision,
        ..
    } = r.hub.current_prompt("s1").unwrap().kind
    else {
        panic!()
    };
    assert_eq!(prompt_kind, "permission");
    let d = decision.unwrap();
    assert_eq!(d.detail.as_deref(), Some("Write a.txt"));
    let shown: Vec<(&str, &str)> = d
        .options
        .iter()
        .map(|o| (o.id.as_str(), o.kind.as_str()))
        .collect();
    assert_eq!(
        shown,
        [("allow-once", "allow_once"), ("reject", "reject_once")]
    );

    // An option the agent offered but we never showed, an invented one, and a stale prompt are all refused.
    for bad in ["allow-with-updates", "reject-forever", "nope", ""] {
        assert!(
            r.backend.answer_permission("s1", &id, bad).await.is_err(),
            "{bad}"
        );
    }
    assert!(r
        .backend
        .answer_permission("s1", "acp-99", "reject")
        .await
        .is_err());
    assert!(
        r.logged().iter().all(|m| m["id"] != 100),
        "nothing was sent for a refused tap"
    );

    r.backend
        .answer_permission("s1", &id, "reject")
        .await
        .unwrap();
    until("the answer to arrive", || {
        r.logged().iter().any(|m| m["id"] == 100)
    })
    .await;
    let reply = r.logged().into_iter().find(|m| m["id"] == 100).unwrap();
    assert_eq!(
        reply["result"],
        json!({"outcome": {"outcome": "selected", "optionId": "reject"}})
    );
    // The same prompt cannot be answered twice.
    assert!(r
        .backend
        .answer_permission("s1", &id, "allow-once")
        .await
        .is_err());
    until("the turn to end", || {
        last_state(&r, "s1") == Some(AgentState::Done)
    })
    .await;
    r.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn cancel_answers_the_open_permission_cancelled_and_sends_session_cancel() {
    let r = rig("");
    r.create("s1").await;
    r.prompt("s1", "perm").await;
    until("the prompt", || r.hub.current_prompt("s1").is_some()).await;
    r.backend.interrupt("s1").await.unwrap();
    until("the cancel", || {
        r.logged().iter().any(|m| m["method"] == "session/cancel")
    })
    .await;
    let log = r.logged();
    let reply = log.iter().find(|m| m["id"] == 100).unwrap();
    assert_eq!(
        reply["result"],
        json!({"outcome": {"outcome": "cancelled"}})
    );
    assert_eq!(
        log.iter()
            .find(|m| m["method"] == "session/cancel")
            .unwrap()["params"]["sessionId"],
        "acp-1"
    );
    until("the turn to end", || {
        last_state(&r, "s1") == Some(AgentState::Idle)
    })
    .await;
    // The same session takes the next prompt.
    r.prompt("s1", "updates").await;
    until("the next turn", || {
        last_state(&r, "s1") == Some(AgentState::Done)
    })
    .await;
    r.backend.kill("s1").await.unwrap();
}

#[test]
fn oversized_text_is_capped_in_the_buffer() {
    let mut m = UpdateMapper::default();
    for _ in 0..50 {
        m.map(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "é".repeat(1000)}}));
    }
    let Some(EventKind::Message { summary }) = m.flush() else {
        panic!()
    };
    assert!(summary.len() <= MAX_CHUNK_BUFFER);
}

#[tokio::test]
async fn hostile_text_is_scrubbed_oversized_lines_are_dropped_and_the_session_survives() {
    let r = rig("");
    r.create("s1").await;
    r.prompt("s1", "hostile").await;
    until("the turn to finish", || {
        last_state(&r, "s1") == Some(AgentState::Done)
    })
    .await;
    let k = r.kinds("s1");
    let msg = k
        .iter()
        .find_map(|k| match k {
            EventKind::Message { summary } => Some(summary.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        msg.chars().count() <= crate::events::SUMMARY_CHARS,
        "{}",
        msg.len()
    );
    assert!(!msg.contains('\u{1b}') && !msg.contains("ghp_"), "{msg}");
    let title = k
        .iter()
        .find_map(|k| match k {
            EventKind::Title { title } => Some(title.clone()),
            _ => None,
        })
        .unwrap();
    assert!(title.chars().count() <= crate::events::TASK_CHARS && !title.contains('\u{7}'));
    // The 2 MB line did not kill the reader: another turn still works.
    r.prompt("s1", "updates").await;
    until("the second turn", || {
        r.kinds("s1")
            .iter()
            .any(|k| matches!(k, EventKind::Usage { .. }))
    })
    .await;
    assert_eq!(r.backend.status("s1"), Some(SessionStatus::Running));
    r.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn a_crash_becomes_failed_with_the_exit_status() {
    let r = rig("");
    r.create("s1").await;
    r.prompt("s1", "crash").await;
    until("the failure", || {
        matches!(r.backend.status("s1"), Some(SessionStatus::Failed(_)))
    })
    .await;
    let Some(SessionStatus::Failed(why)) = r.backend.status("s1") else {
        panic!()
    };
    assert!(why.contains('3'), "{why}");
    until("the error state", || {
        last_state(&r, "s1") == Some(AgentState::Error)
    })
    .await;
    assert!(r.backend.send_text("s1", "again").await.is_err());
    // A failed session frees its slot.
    r.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn concurrent_sessions_are_capped() {
    let r = rig_with("", 1, tmp(), false);
    r.create("a").await;
    let e = r
        .backend
        .create_with("b", None, r.dir.to_str(), Some("fake"))
        .await
        .unwrap_err();
    assert!(e.to_string().contains("At most 1"), "{e}");
    r.backend.kill("a").await.unwrap();
    r.create("b").await;
    r.backend.kill("b").await.unwrap();
}

#[tokio::test]
async fn after_a_restart_sessions_come_back_by_resume_or_load_else_end() {
    // resume wins, and replays nothing
    for (mode, method) in [("resume", "session/resume"), ("load", "session/load")] {
        let dir = tmp();
        let a = rig_with(mode, 2, dir.clone(), true);
        a.create("s1").await;
        let saved = std::fs::read_to_string(dir.join("sessions.json")).unwrap();
        assert!(saved.contains("acp-1") && !saved.contains("hello"));
        // "Restart": a new backend with the same state file and the same agent.
        let b = rig_with(mode, 2, dir.clone(), true);
        let b_log = b.log.clone();
        let b = Rig {
            backend: b.backend,
            hub: b.hub,
            log: b_log,
            dir: dir.clone(),
        };
        b.backend.restore().await;
        assert_eq!(
            b.backend.status("s1"),
            Some(SessionStatus::Running),
            "{mode}"
        );
        let call = b
            .logged()
            .into_iter()
            .find(|m| m["method"] == method)
            .unwrap();
        assert_eq!(call["params"]["sessionId"], "acp-1");
        assert!(
            !format!("{:?}", b.kinds("s1")).contains("REPLAYED"),
            "history replay is not news"
        );
        b.prompt("s1", "updates").await;
        until("a turn after restart", || {
            last_state(&b, "s1") == Some(AgentState::Done)
        })
        .await;
        b.backend.kill("s1").await.unwrap();
        a.backend.kill("s1").await.unwrap();
    }
    // An agent that offers neither: the session is ended, not silently gone.
    let dir = tmp();
    let a = rig_with("", 2, dir.clone(), true);
    a.create("s1").await;
    let b = rig_with("", 2, dir.clone(), true);
    b.backend.restore().await;
    assert_eq!(b.backend.status("s1"), Some(SessionStatus::Ended));
    assert_eq!(b.backend.list().await.unwrap().len(), 1);
    assert!(b.backend.send_text("s1", "hi").await.is_err());
    assert_eq!(last_state(&b, "s1"), Some(AgentState::Done));
    a.backend.kill("s1").await.unwrap();
}

// ---------------------------------------------------------------------------------------------------
// T0 session view: the stream a client opens, the way it is answered, interrupted and switched
// ---------------------------------------------------------------------------------------------------

fn view(r: &Rig, name: &str) -> AcpView {
    r.backend.acp_view(name, None, None, 0).unwrap().0
}

fn bodies(v: &AcpView) -> Vec<AcpBody> {
    v.entries.iter().map(|e| e.body.clone()).collect()
}

#[tokio::test]
async fn the_stream_keeps_the_whole_turn_and_wakes_the_watcher_when_it_changes() {
    let r = rig("");
    r.create("s1").await;
    let mut rx = r.backend.acp_watch("s1").unwrap();
    let before = *rx.borrow_and_update();
    r.prompt("s1", "updates").await;
    until("the turn to finish", || {
        last_state(&r, "s1") == Some(AgentState::Done)
    })
    .await;
    assert!(rx.has_changed().unwrap(), "a watcher is woken by the turn");
    assert!(*rx.borrow() > before);
    let v = view(&r, "s1");
    let b = bodies(&v);
    assert!(matches!(&b[0], AcpBody::User { text } if text == "updates"));
    assert!(b
        .iter()
        .any(|e| matches!(e, AcpBody::Thought { text, .. } if text == "hmm ok")));
    assert!(b
        .iter()
        .any(|e| matches!(e, AcpBody::Message { text, .. } if text == "Hello world")));
    assert!(b.iter().any(
        |e| matches!(e, AcpBody::Tool { title, tool_kind, status, .. }
        if title == "Run ls" && tool_kind == "execute" && status == "completed")
    ));
    assert!(b
        .iter()
        .any(|e| matches!(e, AcpBody::Plan { items } if items[0].text == "step one")));
    let info = v.info.unwrap();
    assert_eq!(info.agent, "fake");
    assert_eq!(
        r.hub
            .snapshot()
            .into_iter()
            .find(|s| s.session == "s1")
            .and_then(|s| s.agent)
            .as_deref(),
        Some("fake"),
        "Home and the tabs know the agent without a process scan"
    );
    assert_eq!(info.dir, r.dir.to_str().unwrap());
    assert_eq!(
        (info.title.as_deref(), info.mode.as_deref()),
        (Some("My title"), Some("plan"))
    );
    assert_eq!(
        info.usage.as_ref().map(|u| (u.used, u.size)),
        Some((500, 1000))
    );
    assert_eq!(info.commands[0].name, "compact");
    assert!(!info.working && info.status == "running");
    // Raw tool input and output are not in the stream.
    let wire = serde_json::to_string(&view(&r, "s1")).unwrap();
    assert!(!wire.contains("SECRET"), "{wire}");
    r.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn a_permission_shows_in_the_stream_and_records_exactly_the_tapped_option() {
    let r = rig("");
    r.create("s1").await;
    r.prompt("s1", "perm").await;
    until("the prompt", || r.hub.current_prompt("s1").is_some()).await;
    let open = bodies(&view(&r, "s1"));
    let Some(AcpBody::Permission {
        prompt_id,
        options,
        state,
        title,
        ..
    }) = open
        .iter()
        .find(|b| matches!(b, AcpBody::Permission { .. }))
        .cloned()
    else {
        panic!("no permission entry in {open:?}")
    };
    assert_eq!(state, "open");
    assert_eq!(title, "Write a.txt");
    assert_eq!(
        options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["allow-once", "reject"]
    );
    r.backend
        .acp_answer("s1", &prompt_id, "reject")
        .await
        .unwrap();
    until("the turn", || {
        last_state(&r, "s1") == Some(AgentState::Done)
    })
    .await;
    let done = bodies(&view(&r, "s1"));
    assert!(done
        .iter()
        .any(|b| matches!(b, AcpBody::Permission { state, chosen, .. }
        if state == "answered" && chosen.as_deref() == Some("reject"))));
    r.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn an_interrupt_cancels_the_open_request_and_the_stream_says_interrupted() {
    let r = rig("");
    r.create("s1").await;
    r.prompt("s1", "perm").await;
    until("the prompt", || r.hub.current_prompt("s1").is_some()).await;
    r.backend.acp_interrupt("s1").await.unwrap();
    until("the turn to end", || {
        bodies(&view(&r, "s1"))
            .iter()
            .any(|b| matches!(b, AcpBody::Notice { text, .. } if text == "Interrupted"))
    })
    .await;
    let b = bodies(&view(&r, "s1"));
    assert!(b.iter().any(
        |b| matches!(b, AcpBody::Permission { state, chosen: None, .. } if state == "cancelled")
    ));
    assert!(!view(&r, "s1").info.unwrap().working);
    r.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn a_mode_is_sent_only_if_the_agent_listed_it() {
    let r = rig("modes");
    r.create("s1").await;
    let info = view(&r, "s1").info.unwrap();
    assert_eq!(info.mode.as_deref(), Some("ask"));
    assert_eq!(
        info.modes.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["ask", "plan"]
    );
    assert!(r.backend.acp_mode("s1", "yolo").await.is_err());
    assert!(
        r.logged().iter().all(|m| m["method"] != "session/set_mode"),
        "an unlisted mode is never sent"
    );
    r.backend.acp_mode("s1", "plan").await.unwrap();
    let sent = r
        .logged()
        .into_iter()
        .find(|m| m["method"] == "session/set_mode")
        .unwrap();
    assert_eq!(sent["params"]["modeId"], "plan");
    assert_eq!(sent["params"]["sessionId"], "acp-1");
    assert_eq!(view(&r, "s1").info.unwrap().mode.as_deref(), Some("plan"));
    // An agent without modes has nothing to switch.
    let none = rig("");
    none.create("s2").await;
    assert!(none.backend.acp_mode("s2", "plan").await.is_err());
    r.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn a_crash_is_failed_in_the_stream_and_a_closed_session_has_no_stream() {
    let r = rig("");
    r.create("s1").await;
    r.prompt("s1", "crash").await;
    until("the failure", || {
        r.backend
            .acp_view("s1", None, None, 0)
            .unwrap()
            .0
            .info
            .is_some_and(|i| i.status == "failed")
    })
    .await;
    let v = view(&r, "s1");
    assert!(bodies(&v)
        .iter()
        .any(|b| matches!(b, AcpBody::Notice { level, .. } if level == "error")));
    assert!(v.info.unwrap().status_text.unwrap().contains('3'));
    r.backend.kill("s1").await.unwrap();
    assert!(r.backend.acp_view("s1", None, None, 0).is_err());
    assert!(r.backend.acp_watch("s1").is_err());
}

#[tokio::test]
async fn hostile_text_is_scrubbed_and_capped_in_the_stream_too() {
    let r = rig("");
    r.create("s1").await;
    r.prompt("s1", "hostile").await;
    until("the turn", || {
        last_state(&r, "s1") == Some(AgentState::Done)
    })
    .await;
    let b = bodies(&view(&r, "s1"));
    let Some(AcpBody::Message { text, .. }) =
        b.iter().find(|b| matches!(b, AcpBody::Message { .. }))
    else {
        panic!("no message")
    };
    assert!(
        !text.contains('\u{1b}') && !text.contains("ghp_"),
        "{}",
        &text[..60]
    );
    assert!(text.chars().count() <= 16_000);
    assert!(view(&r, "s1").info.unwrap().title.unwrap().chars().count() <= 200);
    r.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn a_restored_session_says_that_its_earlier_output_is_not_shown() {
    let dir = tmp();
    let a = rig_with("load", 2, dir.clone(), true);
    a.create("s1").await;
    a.prompt("s1", "updates").await;
    until("the turn", || {
        last_state(&a, "s1") == Some(AgentState::Done)
    })
    .await;
    let b = rig_with("load", 2, dir, true);
    b.backend.restore().await;
    let v = view(&b, "s1");
    assert!(bodies(&v)
        .iter()
        .any(|e| matches!(e, AcpBody::Notice { text, .. } if text.contains("Restored"))));
    assert!(!bodies(&v)
        .iter()
        .any(|e| matches!(e, AcpBody::Message { text, .. } if text.contains("REPLAYED"))));
    a.backend.kill("s1").await.unwrap();
    b.backend.kill("s1").await.unwrap();
}

#[tokio::test]
async fn a_refused_session_new_tells_the_owner_to_log_in() {
    let r = rig("authfail");
    let e = r
        .backend
        .create_with("s1", None, r.dir.to_str(), Some("fake"))
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        "fake would not start: Authentication required. Log in to it on the server, then try again."
    );
    assert!(r.backend.list().await.unwrap().is_empty());
}
