//! Conformance of the wire to ACP v1 (step 2.3, guardrail `protocol-core-stays-acp-v1`, decision 0136): every recorded
//! capture in `testdata/acp/` and one sample of each of the 14 standard `session/update` kinds is replayed through the
//! server's adapter (`Stream`) with termenbox extensions added, the extensions are stripped with
//! `ExtMeta::strip_all`, and what is left must be what the agent sent: it decodes with the schema crate to the same
//! value, and the adapter shows the same view for it. Decoding is lenient per message (fact 0057): the raw
//! `sessionUpdate` / `type` string is read first, because the schema types fail on an unknown one.
//!
//! The same samples, and the `AcpView` the adapter makes of them (all together, and each kind alone), are `testdata/acp-wire/standard-kinds.json`; the
//! client test feeds that view to its reducer. Set `UPDATE_VECTORS=1` to rewrite the file after a deliberate change.
use super::*;
use crate::acp_stream::Stream;
use serde_json::Map;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use termenbox_core::ext::ExtMeta;

/// The `session/update` kinds of ACP v1 (fact 0057); the unstable ones are not part of the core.
const STANDARD: [&str; 14] = [
    "user_message_chunk",
    "agent_message_chunk",
    "agent_thought_chunk",
    "tool_call",
    "tool_call_update",
    "plan",
    "available_commands_update",
    "current_mode_update",
    "config_option_update",
    "session_info_update",
    "usage_update",
    "notice",
    "compaction_update",
    "compaction_summary_chunk",
];

const CONTENT_TYPES: [&str; 5] = ["text", "image", "audio", "resource_link", "resource"];

fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}

/// One valid sample per standard kind, in the order of `STANDARD`.
fn samples() -> Vec<Value> {
    vec![
        json!({"sessionUpdate": "user_message_chunk", "content": text("fix the build")}),
        json!({"sessionUpdate": "agent_message_chunk", "content": text("Looking at it.")}),
        json!({"sessionUpdate": "agent_thought_chunk", "content": text("The lockfile is stale.")}),
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit build.rs", "kind": "edit",
            "status": "in_progress", "locations": [{"path": "/work/build.rs", "line": 3}],
            "content": [{"type": "diff", "path": "/work/build.rs", "oldText": "a", "newText": "b"}]}),
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"}),
        json!({"sessionUpdate": "plan", "entries": [
            {"content": "Read the log", "priority": "high", "status": "completed"},
            {"content": "Fix it", "priority": "medium", "status": "in_progress"}]}),
        json!({"sessionUpdate": "available_commands_update",
            "availableCommands": [{"name": "compact", "description": "Shrink the context"}]}),
        json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"}),
        json!({"sessionUpdate": "config_option_update", "configOptions": [
            {"id": "model", "name": "Model", "type": "select", "currentValue": "a",
             "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}]}]}),
        json!({"sessionUpdate": "session_info_update", "title": "Fix the build"}),
        json!({"sessionUpdate": "usage_update", "used": 1200, "size": 200000}),
        json!({"sessionUpdate": "notice", "severity": "warning", "title": "Rate limited", "description": "Slowing down."}),
        json!({"sessionUpdate": "compaction_update", "compactionId": "c1", "status": "completed",
            "summary": [text("Earlier work, summarised.")]}),
        json!({"sessionUpdate": "compaction_summary_chunk", "compactionId": "c1", "content": text("Earlier work")}),
    ]
}

fn files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/acp");
    let mut files = Vec::new();
    for agent in std::fs::read_dir(&root).unwrap() {
        let agent = agent.unwrap().path();
        if agent.is_dir() {
            for f in std::fs::read_dir(&agent).unwrap() {
                let f = f.unwrap().path();
                if f.extension().is_some_and(|e| e == "jsonl") {
                    files.push(f);
                }
            }
        }
    }
    files.sort();
    files
}

/// `(agent -> client?, message)` for each line of a capture.
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

/// Adds what termenbox would add to every object: our keys in `_meta`, and the offer under `termenbox.dev`. Existing
/// `_meta` values are left as they are (other namespaces travel through untouched, which `ext.rs` tests).
fn extend(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let mut o: Map<String, Value> = o
                .iter()
                .map(|(k, v)| (k.clone(), if k == "_meta" { v.clone() } else { extend(v) }))
                .collect();
            let meta = o.entry("_meta").or_insert_with(|| json!({}));
            if let Some(meta) = meta.as_object_mut() {
                meta.insert("termenbox.dev/seq".into(), json!(9));
                meta.insert("termenbox.dev/tier".into(), json!({"tier": "T1"}));
                meta.insert("termenbox.dev".into(), json!({"seq": 1}));
            }
            Value::Object(o)
        }
        Value::Array(a) => Value::Array(a.iter().map(extend).collect()),
        other => other.clone(),
    }
}

/// Removes every termenbox key from every `_meta`, and a `_meta` that holds nothing else, so what remains is plain ACP.
fn strip(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let mut out = Map::new();
            for (k, v) in o {
                if k == "_meta" {
                    let mut meta: ExtMeta = serde_json::from_value(v.clone()).unwrap();
                    meta.strip_all();
                    if !meta.0.is_empty() {
                        out.insert(k.clone(), Value::Object(meta.0));
                    }
                } else {
                    out.insert(k.clone(), strip(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(strip).collect()),
        other => other.clone(),
    }
}

/// True if no object anywhere holds a key of our namespace in its `_meta`.
fn has_no_termenbox_key(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.iter().all(|(k, v)| {
            if k == "_meta" {
                v.as_object().is_some_and(|m| {
                    m.keys()
                        .all(|k| !k.starts_with("termenbox.dev/") && k != "termenbox.dev")
                })
            } else {
                has_no_termenbox_key(v)
            }
        }),
        Value::Array(a) => a.iter().all(has_no_termenbox_key),
        _ => true,
    }
}

/// Decodes `v` as `T` and writes it out again: what the schema makes of the message.
fn typed<T: serde::de::DeserializeOwned + Serialize>(v: &Value, what: &str) -> Value {
    let t: T = serde_json::from_value(v.clone())
        .unwrap_or_else(|e| panic!("{what} does not validate against the schema: {e}\n{v}"));
    forget_empty_meta(serde_json::to_value(t).unwrap())
}

/// `_meta: {}` and no `_meta` mean the same in ACP.
fn forget_empty_meta(v: Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.into_iter()
                .filter(|(k, v)| !(k == "_meta" && v.as_object().is_some_and(Map::is_empty)))
                .map(|(k, v)| (k, forget_empty_meta(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(forget_empty_meta).collect()),
        other => other,
    }
}

/// Validates one message the way the adapter reads it (by its raw string first) and checks that stripping the
/// extensions off an extended copy gives the same schema value. Returns the update kind when it was an update.
fn conforms(msg: &Value, incoming: bool, seen: &mut Seen) {
    let method = msg["method"].as_str();
    let check = |name: &str, f: &dyn Fn(&Value) -> Value, v: &Value| {
        let (plain, stripped) = (f(v), f(&strip(&extend(v))));
        assert_eq!(plain, stripped, "{name}: extensions left a trace in {v}");
    };
    match (incoming, method) {
        (true, Some("session/update")) => {
            let u = &msg["params"]["update"];
            let kind = u["sessionUpdate"].as_str().unwrap_or("");
            if STANDARD.contains(&kind) {
                seen.kinds.insert(kind.to_string());
                check(kind, &|v| typed::<acp::SessionUpdate>(v, kind), u);
            } else {
                // Not ours and not an error: the schema cannot decode it, the adapter skips it (fact 0057).
                assert!(
                    serde_json::from_value::<acp::SessionUpdate>(u.clone()).is_err(),
                    "{kind} decodes but is not in the standard list"
                );
                seen.skipped.insert(kind.to_string());
            }
            let blocks: Vec<&Value> = match &u["content"] {
                Value::Array(a) => a.iter().collect(),
                one => vec![one],
            };
            for block in blocks {
                if let Some(t) = block["type"].as_str() {
                    if CONTENT_TYPES.contains(&t) {
                        seen.contents.insert(t.to_string());
                    }
                }
            }
        }
        (true, Some("session/request_permission")) => {
            check(
                "request_permission",
                &|v| typed::<acp::RequestPermissionRequest>(v, "request_permission"),
                &msg["params"],
            );
            seen.permissions += 1;
        }
        (true, None) if msg.get("result").is_some() => {
            let r = &msg["result"];
            if r.get("agentCapabilities").is_some() {
                check(
                    "initialize",
                    &|v| typed::<acp::InitializeResponse>(v, "initialize"),
                    r,
                );
            } else if r.get("sessionId").is_some() {
                check(
                    "session/new",
                    &|v| typed::<acp::NewSessionResponse>(v, "session/new"),
                    r,
                );
            } else if r.get("stopReason").is_some() {
                check(
                    "session/prompt",
                    &|v| typed::<acp::PromptResponse>(v, "session/prompt"),
                    r,
                );
            }
        }
        _ => {}
    }
}

#[derive(Default)]
struct Seen {
    kinds: BTreeSet<String>,
    skipped: BTreeSet<String>,
    contents: BTreeSet<String>,
    permissions: usize,
}

/// What the adapter shows after `updates`, as JSON.
fn view_of(updates: &[Value]) -> Value {
    let mut st = Stream::new("agent", "/work", 1);
    for u in updates {
        st.update(u);
    }
    serde_json::to_value(st.view("w", None, None, 0).0).unwrap()
}

#[test]
fn conformance_every_capture_is_plain_acp_once_extensions_are_stripped() {
    let files = files();
    assert!(
        files.len() >= 40,
        "the captures are missing: {}",
        files.len()
    );
    let mut seen = Seen::default();
    for file in &files {
        let mut updates = Vec::new();
        for (incoming, msg) in messages(file) {
            conforms(&msg, incoming, &mut seen);
            if incoming && msg["method"] == "session/update" {
                updates.push(msg["params"]["update"].clone());
            }
        }
        // The adapter ignores what it does not know: extensions on every object change nothing it shows, and the
        // stripped copy is the capture again.
        let extended: Vec<Value> = updates.iter().map(extend).collect();
        let stripped: Vec<Value> = extended.iter().map(strip).collect();
        let plain = view_of(&updates);
        assert_eq!(
            plain,
            view_of(&extended),
            "extensions changed the view of {}",
            file.display()
        );
        assert_eq!(
            plain,
            view_of(&stripped),
            "stripping changed the view of {}",
            file.display()
        );
        assert!(stripped.iter().all(has_no_termenbox_key));
    }
    assert!(
        seen.permissions > 20,
        "{} permission requests",
        seen.permissions
    );
    assert!(
        seen.kinds.len() >= 8,
        "the captures cover too few standard kinds: {:?}",
        seen.kinds
    );
}

#[test]
fn conformance_each_standard_update_kind_validates_and_survives_stripping() {
    let samples = samples();
    assert_eq!(
        samples
            .iter()
            .map(|s| s["sessionUpdate"].as_str().unwrap())
            .collect::<Vec<_>>(),
        STANDARD
    );
    let mut seen = Seen::default();
    for u in &samples {
        conforms(
            &json!({"method": "session/update", "params": {"update": u}}),
            true,
            &mut seen,
        );
    }
    assert_eq!(seen.kinds.len(), STANDARD.len());
    assert_eq!(
        samples
            .iter()
            .map(extend)
            .map(|v| strip(&v))
            .collect::<Vec<_>>()
            .len(),
        STANDARD.len()
    );
    // An extension on the update changes nothing the adapter shows.
    assert_eq!(
        view_of(&samples),
        view_of(&samples.iter().map(extend).collect::<Vec<_>>())
    );
}

#[test]
fn conformance_an_unknown_kind_or_content_type_is_skipped_not_fatal() {
    let unknown = [
        json!({"sessionUpdate": "from_the_future", "x": 1}),
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "hologram", "data": "?"}}),
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t9", "status": "on_fire"}),
    ];
    // The schema fails on the first two, as fact 0057 says; the adapter drops each alone and goes on.
    for u in &unknown[..2] {
        assert!(serde_json::from_value::<acp::SessionUpdate>(u.clone()).is_err());
    }
    let mut all = vec![samples()[1].clone()];
    all.extend(unknown);
    all.push(samples()[9].clone());
    let view = view_of(&all);
    let kinds: Vec<&str> = view["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"message"), "{view}");
    assert_eq!(view["info"]["title"], "Fix the build");
}

#[test]
fn conformance_the_wire_vector_matches_the_adapter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/acp-wire/standard-kinds.json");
    let per_kind: Map<String, Value> = samples()
        .iter()
        .map(|u| {
            let kind = u["sessionUpdate"].as_str().unwrap().to_string();
            (kind, view_of(std::slice::from_ref(u)))
        })
        .collect();
    let vector = json!({"updates": samples(), "view": view_of(&samples()), "per_kind": per_kind});
    if std::env::var_os("UPDATE_VECTORS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&vector).unwrap() + "\n").unwrap();
    }
    let held: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!("{} is missing: run with UPDATE_VECTORS=1", path.display())
        }))
        .unwrap();
    assert_eq!(
        held, vector,
        "the adapter's view of the standard kinds changed: run with UPDATE_VECTORS=1 if that is deliberate"
    );
}
