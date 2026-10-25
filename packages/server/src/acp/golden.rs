//! The golden check for the move onto the schema crate (plan acp-session-modes, step 0): every recorded capture in
//! `testdata/acp/<agent>/<scenario>.jsonl` goes through the mapper as it was before the move (a frozen copy, below)
//! and through the current one, and the two must give the same `EventKind` stream and the same offered permission
//! options. The frozen copy is the old `serde_json::Value` mapping verbatim; it is not used anywhere else.
use super::*;
use std::path::Path;

const MAX_CHUNK_BUFFER: usize = super::MAX_CHUNK_BUFFER;
const MAX_TOOLS: usize = super::MAX_TOOLS;

// ---- frozen copy of the pre-schema mapper ---------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum OldChunk {
    Message,
    Thought,
}

/// Turns `session/update` payloads into the agent event model. Streamed chunks are joined into one event,
/// flushed when another kind arrives or the turn ends; tool calls are merged by `toolCallId`.
#[derive(Default)]
struct OldMapper {
    tools: HashMap<String, (String, String)>,
    buf: Option<(OldChunk, String)>,
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn first_line(t: &str) -> String {
    t.lines().next().unwrap_or("").to_string()
}

/// Cuts at a char boundary at most `max` bytes in.
fn cap(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl OldMapper {
    fn flush(&mut self) -> Option<EventKind> {
        let (kind, text) = self.buf.take()?;
        if text.trim().is_empty() {
            return None;
        }
        Some(match kind {
            OldChunk::Message => EventKind::Message { summary: text },
            OldChunk::Thought => EventKind::Thought { summary: text },
        })
    }

    fn chunk(&mut self, kind: OldChunk, u: &Value) -> Vec<EventKind> {
        let mut out = Vec::new();
        if self.buf.as_ref().map(|(k, _)| *k != kind).unwrap_or(false) {
            out.extend(self.flush());
        }
        let content = u.get("content").unwrap_or(&Value::Null);
        if s(content, "type") == "text" {
            let buf = &mut self.buf.get_or_insert((kind, String::new())).1;
            let room = MAX_CHUNK_BUFFER.saturating_sub(buf.len());
            buf.push_str(cap(s(content, "text"), room));
        }
        out
    }

    fn map(&mut self, u: &Value) -> Vec<EventKind> {
        let kind = s(u, "sessionUpdate");
        match kind {
            "agent_message_chunk" => return self.chunk(OldChunk::Message, u),
            "agent_thought_chunk" => return self.chunk(OldChunk::Thought, u),
            _ => {}
        }
        let mut out: Vec<EventKind> = self.flush().into_iter().collect();
        match kind {
            "tool_call" => {
                let id = s(u, "toolCallId").to_string();
                let name = match s(u, "kind") {
                    "" => "tool".to_string(),
                    k => k.to_string(),
                };
                let summary = first_line(s(u, "title"));
                if self.tools.len() >= MAX_TOOLS {
                    self.tools.clear();
                }
                let done = matches!(s(u, "status"), "completed" | "failed");
                out.push(EventKind::Tool {
                    name: name.clone(),
                    summary: summary.clone(),
                    phase: ToolPhase::Started,
                });
                if done {
                    out.push(EventKind::Tool {
                        name,
                        summary,
                        phase: ToolPhase::Finished,
                    });
                } else {
                    self.tools.insert(id, (name, summary));
                }
            }
            "tool_call_update" => {
                let id = s(u, "toolCallId");
                let (name, mut summary) = self
                    .tools
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| ("tool".into(), String::new()));
                if !s(u, "title").is_empty() {
                    summary = first_line(s(u, "title"));
                }
                if matches!(s(u, "status"), "completed" | "failed") {
                    self.tools.remove(id);
                    out.push(EventKind::Tool {
                        name,
                        summary,
                        phase: ToolPhase::Finished,
                    });
                } else if let Some(t) = self.tools.get_mut(id) {
                    t.1 = summary;
                }
            }
            "plan" => {
                let items: Vec<PlanItem> = u
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .take(crate::events::PLAN_ITEMS)
                            .map(|e| PlanItem {
                                text: s(e, "content").to_string(),
                                status: s(e, "status").to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !items.is_empty() {
                    out.push(EventKind::Plan { items });
                }
            }
            "usage_update" => {
                if let (Some(used), Some(size)) = (
                    u.get("used").and_then(Value::as_u64),
                    u.get("size").and_then(Value::as_u64),
                ) {
                    let cost = u.get("cost").filter(|c| c.is_object()).map(|c| {
                        format!(
                            "{} {}",
                            c.get("amount").map(|a| a.to_string()).unwrap_or_default(),
                            s(c, "currency")
                        )
                    });
                    out.push(EventKind::Usage { used, size, cost });
                }
            }
            "session_info_update" => {
                if let Some(title) = u.get("title").and_then(Value::as_str) {
                    out.push(EventKind::Title {
                        title: title.to_string(),
                    });
                }
            }
            "current_mode_update" => out.push(EventKind::Mode {
                mode: s(u, "currentModeId").to_string(),
            }),
            "available_commands_update" => out.push(EventKind::Commands {
                names: u
                    .get("availableCommands")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|c| s(c, "name").to_string()).collect())
                    .unwrap_or_default(),
            }),
            _ => {}
        }
        out
    }
}

/// The options of a `session/request_permission` that may be shown: `allow_once` and `reject_once` only
/// (decision 0104); `allow_always`, `reject_always` and anything unknown are dropped.
fn old_permission_options(params: &Value) -> Vec<DecisionOption> {
    params
        .get("options")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|o| matches!(s(o, "kind"), "allow_once" | "reject_once"))
                .filter(|o| !s(o, "optionId").is_empty())
                .map(|o| DecisionOption {
                    id: s(o, "optionId").to_string(),
                    kind: s(o, "kind").to_string(),
                    name: s(o, "name").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---- the comparison ---------------------------------------------------------------------------------

fn captures() -> Vec<std::path::PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/acp");
    let mut files = Vec::new();
    for agent in std::fs::read_dir(&root).unwrap() {
        let agent = agent.unwrap().path();
        if !agent.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(&agent).unwrap() {
            let f = f.unwrap().path();
            if f.extension().is_some_and(|e| e == "jsonl") {
                files.push(f);
            }
        }
    }
    files.sort();
    files
}

fn messages(file: &Path) -> Vec<(bool, Value)> {
    std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let rec: Value = serde_json::from_str(l).unwrap();
            (rec["dir"] == "in", rec["msg"].clone())
        })
        .collect()
}

#[test]
fn the_schema_mapper_matches_the_old_mapper_on_every_capture() {
    let files = captures();
    assert!(
        files.len() >= 40,
        "the captures are missing: {}",
        files.len()
    );
    let (mut updates, mut permissions) = (0, 0);
    for file in &files {
        let (mut old, mut new) = (OldMapper::default(), UpdateMapper::default());
        let (mut old_events, mut new_events) = (Vec::new(), Vec::new());
        for (incoming, msg) in messages(file) {
            if !incoming {
                continue;
            }
            match msg["method"].as_str() {
                Some("session/update") => {
                    updates += 1;
                    let u = &msg["params"]["update"];
                    old_events.extend(old.map(u));
                    new_events.extend(new.map(u));
                }
                Some("session/request_permission") => {
                    permissions += 1;
                    let (o, n) = (
                        old_permission_options(&msg["params"]),
                        permission_options(&msg["params"]),
                    );
                    assert_eq!(o, n, "permission options differ in {}", file.display());
                }
                _ => {}
            }
        }
        // The captures were shortened with strings like "<17 more>" inside the command lists. The old mapper read those
        // as commands with an empty name; they are not commands and the schema types skip them.
        let nameless = |e: &EventKind| match e {
            EventKind::Commands { names } => EventKind::Commands {
                names: names.iter().filter(|n| !n.is_empty()).cloned().collect(),
            },
            other => other.clone(),
        };
        old_events.extend(old.flush());
        new_events.extend(new.flush());
        assert_eq!(
            old_events.iter().map(nameless).collect::<Vec<_>>(),
            new_events,
            "event streams differ in {}",
            file.display()
        );
    }
    assert!(
        updates > 500 && permissions > 20,
        "{updates} updates, {permissions} requests"
    );
}

#[test]
fn the_schema_types_decode_what_the_agents_really_send() {
    // The captures are what the agents really send. A message that fails here is dropped in production (counted,
    // not shown), so a failure means a schema type is stricter than an agent (fact 0040).
    let mut failed = Vec::new();
    for file in captures() {
        let name = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap()
            .display()
            .to_string();
        for (incoming, msg) in messages(&file) {
            if !incoming {
                continue;
            }
            let method = msg["method"].as_str();
            let mut check = |what: &str, ok: bool| {
                if !ok {
                    failed.push(format!("{what} in {name}: {msg}"));
                }
            };
            match method {
                Some("session/update") => {
                    let u = &msg["params"]["update"];
                    if HANDLED_UPDATES.contains(&s(u, "sessionUpdate")) {
                        check("update", acp::SessionUpdate::deserialize(u).is_ok());
                    }
                }
                Some("session/request_permission") => {
                    check(
                        "permission request",
                        acp::RequestPermissionRequest::deserialize(&msg["params"]).is_ok(),
                    );
                }
                None if msg.get("result").is_some() => {
                    let r = &msg["result"];
                    if r.get("agentCapabilities").is_some() {
                        check(
                            "initialize",
                            acp::InitializeResponse::deserialize(r).is_ok(),
                        );
                    } else if r.get("sessionId").is_some() {
                        check(
                            "session/new",
                            acp::NewSessionResponse::deserialize(r).is_ok(),
                        );
                    } else if r.get("stopReason").is_some() {
                        check(
                            "session/prompt",
                            acp::PromptResponse::deserialize(r).is_ok(),
                        );
                    }
                }
                _ => {}
            }
        }
    }
    assert!(
        failed.is_empty(),
        "{} messages do not decode:\n{}",
        failed.len(),
        failed
            .iter()
            .map(|f| f.chars().take(400).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn a_message_that_does_not_decode_is_dropped_alone() {
    let before = DROPPED.load(Ordering::Relaxed);
    let mut m = UpdateMapper::default();
    let chunk = |t: &str| json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}});
    assert!(m.map(&chunk("kept ")).is_empty());
    // A tool call without its id, an update of a known kind with the wrong shape: dropped, counted, nothing else.
    assert!(m
        .map(&json!({"sessionUpdate": "agent_message_chunk", "content": 7}))
        .is_empty());
    let flushed = m.map(&json!({"sessionUpdate": "tool_call", "title": "no id"}));
    assert_eq!(
        flushed,
        vec![EventKind::Message {
            summary: "kept ".into()
        }]
    );
    assert!(m
        .map(&json!({"sessionUpdate": "usage_update", "used": "many"}))
        .is_empty());
    assert!(DROPPED.load(Ordering::Relaxed) >= before + 3);
    // An unknown kind is ignored and is not a drop; the mapper still works afterwards.
    let unknown = DROPPED.load(Ordering::Relaxed);
    assert!(m
        .map(&json!({"sessionUpdate": "from_the_future", "x": 1}))
        .is_empty());
    assert!(DROPPED.load(Ordering::Relaxed) >= unknown);
    assert!(
        m.map(&json!({"sessionUpdate": "session_info_update", "title": "T"}))
            .len()
            == 1
    );
}

#[test]
fn a_permission_request_with_an_odd_tool_call_still_offers_its_options() {
    let p = json!({"toolCall": "nonsense", "options": [{"optionId": "a", "name": "A", "kind": "allow_once"}]});
    assert_eq!(permission_options(&p).len(), 1);
    assert_eq!(permission_tool(&p).fields.title, None);
}

#[test]
fn the_requests_have_the_wire_form_they_always_had() {
    let old_initialize = json!({
        "protocolVersion": 1,
        "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
        "clientInfo": {"name": "termenbox", "version": env!("CARGO_PKG_VERSION")},
    });
    let new_initialize = initialize_params(&initialize_request(false));
    assert_eq!(new_initialize, old_initialize);
    assert_eq!(
        wire(&acp::NewSessionRequest::new("/d")),
        json!({"cwd": "/d", "mcpServers": []})
    );
    assert_eq!(
        wire(&acp::LoadSessionRequest::new("s".to_string(), "/d")),
        json!({"sessionId": "s", "cwd": "/d", "mcpServers": []})
    );
    assert_eq!(
        wire(&acp::SetSessionModeRequest::new(
            "s".to_string(),
            "plan".to_string()
        )),
        json!({"sessionId": "s", "modeId": "plan"})
    );
    assert_eq!(
        wire(&acp::SetSessionConfigOptionRequest::new(
            "s".to_string(),
            "model".to_string(),
            acp::SessionConfigOptionValue::value_id("haiku".to_string())
        )),
        json!({"sessionId": "s", "configId": "model", "value": "haiku"})
    );
    assert_eq!(
        wire(&acp::PromptRequest::new(
            "s".to_string(),
            vec![acp::ContentBlock::Text(acp::TextContent::new("hi"))]
        )),
        json!({"sessionId": "s", "prompt": [{"type": "text", "text": "hi"}]})
    );
    assert_eq!(
        wire(&acp::CancelNotification::new("s".to_string())),
        json!({"sessionId": "s"})
    );
    assert_eq!(
        wire(&acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled
        )),
        json!({"outcome": {"outcome": "cancelled"}})
    );
    assert_eq!(
        wire(&acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                "o".to_string()
            ))
        )),
        json!({"outcome": {"outcome": "selected", "optionId": "o"}})
    );
    assert_eq!(
        wire(&acp::Error::method_not_found()),
        json!({"code": -32601, "message": "Method not found"})
    );
}
