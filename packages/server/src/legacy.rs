//! THE LEGACY SHIM (decision 0138, step 3.2; removed by step 8.1): the only place that knows the wire names from
//! before the rename of "session" to "workspace". A client that announces no protocol version in its `hello` is an
//! older one: its messages are translated up to the current vocabulary on the way in, and ours down on the way out,
//! so everything else in the server speaks one vocabulary. Delete this file, its tests and its two call sites
//! (`link.rs`, `connection.rs`) to remove it.
//!
//! The translation is on JSON, by key and message type only: no value is touched (a workspace named "session" stays
//! as it is), and the one map keyed by workspace ids (`agent_sync`'s `after`) is not entered.
use serde_json::{Map, Value};

/// (old `type`, new `type`) of the messages whose name changed.
const TYPES: [(&str, &str); 8] = [
    ("list_sessions", "list_workspaces"),
    ("new_session", "new_workspace"),
    ("close_session", "close_workspace"),
    ("rename_session", "rename_workspace"),
    ("sessions", "workspaces"),
    ("session_attached", "workspace_attached"),
    ("session_view", "workspace_view"),
    ("session_answer", "workspace_answer"),
];

/// (old key, new key) of the fields whose name changed.
const KEYS: [(&str, &str); 4] = [
    ("session", "workspace"),
    ("session_id", "workspace_id"),
    ("sessions", "workspaces"),
    ("kill_sessions", "kill_workspaces"),
];

/// Objects under these keys are maps keyed by workspace id: their keys are data.
const DATA_KEYED: [&str; 1] = ["after"];

fn rename_keys(v: &mut Value, table: &[(&str, &str)], from_old: bool) {
    match v {
        Value::Object(o) => {
            let mut out = Map::new();
            for (k, mut val) in std::mem::take(o) {
                if !DATA_KEYED.contains(&k.as_str()) {
                    rename_keys(&mut val, table, from_old);
                }
                let key = table
                    .iter()
                    .find_map(|(old, new)| {
                        let (a, b) = if from_old { (old, new) } else { (new, old) };
                        (k == *a).then(|| b.to_string())
                    })
                    .unwrap_or(k);
                out.insert(key, val);
            }
            *o = out;
        }
        Value::Array(a) => a.iter_mut().for_each(|x| rename_keys(x, table, from_old)),
        _ => {}
    }
}

fn translate(text: &str, from_old: bool) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(text) else {
        return text.to_string();
    };
    if !v.is_object() {
        return text.to_string();
    }
    let ty = v.get("type").and_then(Value::as_str).map(String::from);
    rename_keys(&mut v, &KEYS, from_old);
    if let Some(ty) = ty {
        let renamed = TYPES.iter().find_map(|(old, new)| {
            let (a, b) = if from_old { (old, new) } else { (new, old) };
            (ty == *a).then_some(*b)
        });
        if let Some(t) = renamed {
            v["type"] = Value::String(t.into());
        }
    }
    serde_json::to_string(&v).unwrap_or_else(|_| text.to_string())
}

/// A message from an older client, in the current vocabulary.
pub fn inbound(text: &str) -> String {
    translate(text, true)
}

/// A message of ours, in the vocabulary an older client reads.
pub fn outbound(text: &str) -> String {
    translate(text, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use termenbox_core::protocol::{ClientMessage, ServerMessage};

    fn value(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn legacy_every_old_client_message_parses_after_translation() {
        let old = [
            r#"{"type":"list_sessions"}"#,
            r#"{"type":"new_session","name":"n","template":"claude"}"#,
            r#"{"type":"close_session","session":"s"}"#,
            r#"{"type":"rename_session","session":"s","name":"x"}"#,
            r#"{"type":"attach","session_id":"s"}"#,
            r#"{"type":"session_view","session":"s"}"#,
            r#"{"type":"session_answer","session":"s","prompt_id":"p","option":0}"#,
            r#"{"type":"acp_open","session":"s"}"#,
            r#"{"type":"windows","session":"s"}"#,
            r#"{"type":"uninstall","confirm_name":"n","kill_sessions":true}"#,
            r#"{"type":"agent_sync","after":{"session":4,"sessions":1}}"#,
        ];
        for text in old {
            let new = inbound(text);
            serde_json::from_str::<ClientMessage>(&new)
                .unwrap_or_else(|e| panic!("{text} -> {new}: {e}"));
            assert!(
                !new.contains("_sessions\"") && !new.contains("session_id"),
                "{new}"
            );
        }
        // The map keyed by workspace ids is data.
        assert_eq!(
            value(&inbound(old[10]))["after"],
            json!({"session": 4, "sessions": 1})
        );
    }

    #[test]
    fn legacy_every_current_server_message_reads_in_the_old_vocabulary_and_back() {
        let msgs = [
            ServerMessage::Workspaces { workspaces: vec![] },
            ServerMessage::WorkspaceAttached {
                workspace_id: "s".into(),
                tab: None,
            },
            ServerMessage::Snapshot {
                workspace: "s".into(),
                text: "session".into(),
                hash: "h".into(),
                unchanged: false,
            },
            ServerMessage::Activity { workspaces: vec![] },
        ];
        let want_types = ["sessions", "session_attached", "snapshot", "activity"];
        for (m, ty) in msgs.iter().zip(want_types) {
            let new = serde_json::to_string(m).unwrap();
            let old = outbound(&new);
            assert_eq!(value(&old)["type"], ty, "{old}");
            assert!(!old.contains("workspace"), "{old}");
            // Values are not touched: the screen text "session" survives both ways.
            assert_eq!(value(&inbound(&old)), value(&new));
        }
        assert!(outbound(&serde_json::to_string(&msgs[2]).unwrap()).contains(r#""session":"s""#));
        assert!(outbound(&serde_json::to_string(&msgs[2]).unwrap()).contains(r#""text":"session""#));
    }

    #[test]
    fn legacy_messages_that_did_not_change_pass_through_and_garbage_is_left_alone() {
        for t in [
            r#"{"type":"ping"}"#,
            r#"{"type":"detach"}"#,
            "not json",
            "[1]",
            "\"x\"",
        ] {
            assert_eq!(value_or_text(&inbound(t)), value_or_text(t));
            assert_eq!(value_or_text(&outbound(t)), value_or_text(t));
        }
    }

    fn value_or_text(s: &str) -> Value {
        serde_json::from_str(s).unwrap_or(Value::String(s.into()))
    }

    /// The vectors shared with the client's `legacy.ts`.
    #[test]
    fn legacy_the_shared_vectors_translate_both_ways() {
        let v: Value =
            serde_json::from_str(include_str!("../../core/testdata/legacy-vectors.json")).unwrap();
        for p in v["client_to_server"].as_array().unwrap() {
            let (old, new) = (p["old"].to_string(), p["new"].to_string());
            assert_eq!(value(&inbound(&old)), p["new"], "{old}");
            assert_eq!(value(&outbound(&new)), p["old"], "{new}");
        }
        for p in v["server_to_client"].as_array().unwrap() {
            let (old, new) = (p["old"].to_string(), p["new"].to_string());
            assert_eq!(value(&outbound(&new)), p["old"], "{new}");
            assert_eq!(value(&inbound(&old)), p["new"], "{old}");
        }
    }

    #[test]
    fn legacy_an_old_hello_and_auth_ok_without_a_version_parse() {
        let hello: ClientMessage = serde_json::from_str(r#"{"type":"hello"}"#).unwrap();
        assert_eq!(hello, ClientMessage::Hello { protocol: 0 });
    }
}
