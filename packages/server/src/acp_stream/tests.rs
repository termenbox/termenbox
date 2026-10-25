//! The stream built from the recorded wire captures in `testdata/acp/` (what the agents really send) and from a few
//! hostile inputs. The replay feeds the same payloads `acp.rs` feeds in production.
use super::*;
use serde_json::json;
use termenbox_core::protocol::ServerMessage;

fn fixture(path: &str) -> Vec<Value> {
    let full = format!("{}/testdata/acp/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(full)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// Replays a capture: updates, permission requests, the owner's answers, prompts and their responses.
fn replay(path: &str) -> Stream {
    let mut st = Stream::new("agent", "/work", 1);
    let mut seq = 0;
    let mut last_perm = String::new();
    for rec in fixture(path) {
        let m = &rec["msg"];
        let method = m["method"].as_str().unwrap_or("");
        if rec["dir"] == "in" {
            match method {
                "session/update" => st.update(&m["params"]["update"]),
                "session/request_permission" => {
                    seq += 1;
                    last_perm = format!("acp-{seq}");
                    st.permission(
                        &last_perm,
                        &m["params"],
                        crate::acp::permission_options(&m["params"]),
                    );
                }
                "" if m["result"].get("sessionId").is_some() => st.workspace_state(&m["result"]),
                "" if m["result"].get("stopReason").is_some() => {
                    st.turn_end(m["result"]["stopReason"].as_str(), None)
                }
                _ => {}
            }
        } else if method == "session/prompt" {
            st.user(m["params"]["prompt"][0]["text"].as_str().unwrap_or(""));
        } else if let Some(opt) = m["result"]["outcome"]["optionId"].as_str() {
            st.permission_answered(&last_perm, opt);
        }
    }
    st
}

fn all(st: &mut Stream) -> (AcpView, u64) {
    st.view("s", None, None, 0)
}

fn bodies(st: &mut Stream) -> Vec<AcpBody> {
    all(st).0.entries.into_iter().map(|e| e.body).collect()
}

#[test]
fn a_claude_edit_the_owner_refused_reads_as_prompt_tool_permission_and_reply() {
    let mut st = replay("claude/permission-plan-mode.jsonl");
    let b = bodies(&mut st);
    // The write the agent prepared: one tool entry merged from tool_call and its updates, with the diff it carried.
    let tool = b
        .iter()
        .find_map(|e| match e {
            AcpBody::Tool {
                title,
                tool_kind,
                status,
                diffs,
                locations,
                ..
            } if title == "Write a.txt" => Some((tool_kind, status, diffs, locations)),
            _ => None,
        })
        .expect("the Write tool entry");
    assert_eq!(tool.0, "edit");
    assert_eq!(tool.1, "failed", "the refusal made it failed");
    assert_eq!(
        tool.2[0].path,
        "/home/ubuntu/data/acp-scratch/work-claude/a.txt"
    );
    assert_eq!(tool.2[0].old_text, None);
    assert_eq!(tool.2[0].new_text, "hi");
    assert_eq!(tool.3[0].line, None);
    // The permission: only the one-time options, the diff, and how it ended.
    let perm = b
        .iter()
        .find_map(|e| match e {
            AcpBody::Permission {
                options,
                state,
                chosen,
                diffs,
                tool_id,
                ..
            } if !diffs.is_empty() => Some((options, state, chosen, tool_id)),
            _ => None,
        })
        .expect("the permission entry");
    assert_eq!(
        perm.0.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["allow-once", "reject"]
    );
    assert_eq!(perm.1, "answered");
    assert_eq!(perm.2.as_deref(), Some("reject"));
    assert!(perm.3.starts_with("toolu_"));
    // The reply is the chunks joined, as one message.
    assert!(b
        .iter()
        .any(|e| matches!(e, AcpBody::Message { text, .. } if text == "I didn't create `a.txt`:")));
    // The owner's prompts are in the stream, and the cancelled turn says so.
    assert!(b
        .iter()
        .any(|e| matches!(e, AcpBody::User { text } if text.starts_with("Create a file a.txt"))));
    assert!(b
        .iter()
        .any(|e| matches!(e, AcpBody::Notice { text, .. } if text == "Interrupted")));
}

#[test]
fn plan_mode_approval_carries_the_plan_text_and_the_agents_modes() {
    let mut st = replay("claude/permission-plan-mode.jsonl");
    let (view, _) = all(&mut st);
    let plan = view
        .entries
        .iter()
        .find_map(|e| match &e.body {
            AcpBody::Permission {
                tool_kind, plan, ..
            } if tool_kind == "switch_mode" => plan.clone(),
            _ => None,
        })
        .expect("the plan approval");
    assert!(plan.starts_with("# Plan: add b.txt"));
    let info = view.info.unwrap();
    assert_eq!(
        info.modes.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [
            "default",
            "acceptEdits",
            "plan",
            "auto",
            "bypassPermissions"
        ]
    );
    // The capture ends after set_mode(plan): claude announces it with config_option_update.
    assert_eq!(info.mode.as_deref(), Some("plan"));
    assert_eq!(st.mode_via(), &ModeVia::Modes);
    // usage_update, available_commands_update and session_info_update fill the facts.
    assert_eq!(info.usage.as_ref().map(|u| u.size), Some(1_000_000));
    assert!(info
        .commands
        .iter()
        .any(|c| c.name == "design" && c.hint.as_deref() == Some("consent | revoke")));
    assert_eq!(info.title.as_deref(), Some("Create a.txt file"));
}

#[test]
fn kilo_todos_arrive_as_a_tool_call_and_become_the_plan() {
    let mut st = replay("kilo/todowrite.jsonl");
    let b = bodies(&mut st);
    let items = b
        .iter()
        .find_map(|e| match e {
            AcpBody::Plan { items } => Some(items.clone()),
            _ => None,
        })
        .expect("a plan entry from todowrite");
    assert_eq!(
        items
            .iter()
            .map(|i| (i.text.as_str(), i.status.as_str()))
            .collect::<Vec<_>>(),
        [
            ("write a.txt", "pending"),
            ("write b.txt", "pending"),
            ("run ls", "pending")
        ]
    );
    // Updates replace the one plan entry; they do not stack up.
    assert_eq!(
        b.iter()
            .filter(|e| matches!(e, AcpBody::Plan { .. }))
            .count(),
        1
    );
    // Reasoning and the reply are separate entries.
    assert!(b.iter().any(|e| matches!(e, AcpBody::Thought { .. })));
    assert!(b
        .iter()
        .any(|e| matches!(e, AcpBody::Message { text, .. } if text.starts_with("bash, edit"))));
    // kilo lists its modes only as a config option; the change goes through set_config_option.
    assert_eq!(st.mode_via(), &ModeVia::Config("mode".into()));
    assert_eq!(st.info.mode.as_deref(), Some("code"));
    assert!(st.mode_ids().contains(&"ask".to_string()));
}

#[test]
fn an_opencode_command_the_owner_rejected_ends_cancelled_with_its_tool_failed() {
    let mut st = replay("opencode/permission-reject.jsonl");
    let b = bodies(&mut st);
    assert!(b.iter().any(|e| matches!(e, AcpBody::Tool { title, tool_kind, status, .. } if title == "ls /etc" && tool_kind == "execute" && status == "failed")));
    assert!(b.iter().any(|e| matches!(e, AcpBody::Permission { state, chosen, .. } if state == "answered" && chosen.as_deref() == Some("reject"))));
    // Neither the rejected call's raw input nor its raw output is in anything the client receives.
    let wire = serde_json::to_string(&all(&mut st).0).unwrap();
    assert!(!wire.contains("timeout"), "raw tool input leaked");
    assert!(
        !wire.contains("declined this tool call"),
        "tool output leaked"
    );
}

#[test]
fn every_recorded_capture_replays_without_breaking_the_bounds() {
    let root = format!("{}/testdata/acp", env!("CARGO_MANIFEST_DIR"));
    let mut n = 0;
    for agent in std::fs::read_dir(root).unwrap() {
        for f in std::fs::read_dir(agent.unwrap().path()).unwrap() {
            let path = f.unwrap().path();
            let rel = path
                .strip_prefix(format!("{}/testdata/acp", env!("CARGO_MANIFEST_DIR")))
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let st = replay(&rel);
            assert!(
                st.slots.len() <= MAX_ENTRIES && st.bytes <= MAX_BYTES,
                "{rel}"
            );
            n += 1;
        }
    }
    assert!(n >= 40, "the captures were found ({n})");
}

#[test]
fn a_secret_split_over_two_chunks_is_scrubbed_in_the_whole() {
    let mut st = Stream::new("a", "/w", 1);
    let chunk = |t: &str| json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}});
    st.update(&chunk("the key is ghp_abcdefghij"));
    st.update(&chunk("klmnopqrstuvwxyz0123 ok\n\n\n\nnext\x1b[31m line"));
    let b = bodies(&mut st);
    let AcpBody::Message { text, .. } = &b[0] else {
        panic!()
    };
    assert!(text.contains("[token]") && !text.contains("ghp_"), "{text}");
    assert!(!text.contains('\x1b'), "escape sequences are removed");
    assert!(
        text.contains("ok\n\nnext"),
        "blank runs collapse, newlines stay: {text:?}"
    );
}

#[test]
fn a_reply_keeps_its_markdown_layout_and_stops_growing_at_the_limit() {
    let mut st = Stream::new("a", "/w", 1);
    let chunk = |t: &str| json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}});
    st.update(&chunk(
        "# Title\n\n- one\n    indented code\n```rs\nfn x() {}\n```\n",
    ));
    let AcpBody::Message { text, .. } = &bodies(&mut st)[0] else {
        panic!()
    };
    assert!(text.contains("    indented code") && text.contains("```rs\nfn x() {}\n```"));
    for _ in 0..4000 {
        st.update(&chunk(&"word ".repeat(40)));
    }
    let AcpBody::Message { text, .. } = &bodies(&mut st)[0] else {
        panic!()
    };
    assert!(text.chars().count() <= 16_000);
    assert!(st.bytes <= MAX_BYTES);
}

#[test]
fn the_stream_drops_its_oldest_entries_and_says_where_it_starts() {
    let mut st = Stream::new("a", "/w", 1);
    for i in 0..(MAX_ENTRIES + 25) {
        st.user(&format!("prompt {i}"));
    }
    let (view, _) = all(&mut st);
    assert_eq!(view.entries.len(), MAX_ENTRIES);
    assert_eq!(view.first, 26);
    assert_eq!(view.entries[0].id, 26);
    // A changed entry whose id is gone is ignored instead of panicking.
    st.set(1, AcpBody::User { text: "x".into() });
    assert_eq!(all(&mut st).0.entries.len(), MAX_ENTRIES);
}

#[test]
fn a_client_gets_only_what_changed_and_a_reset_when_its_copy_is_not_this_stream() {
    let mut st = Stream::new("a", "/w", 42);
    st.user("one");
    let (first, info_rev) = st.view("s", None, None, 0);
    assert!(first.reset && first.info.is_some());
    st.update(
        &json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hi"}}),
    );
    // The same epoch and the revision it holds: the new reply only, and no facts (they did not change).
    let (delta, _) = st.view("s", Some(first.rev), Some(first.epoch), info_rev);
    assert!(!delta.reset && delta.info.is_none());
    assert_eq!(delta.entries.len(), 1);
    assert!(matches!(&delta.entries[0].body, AcpBody::Message { text, .. } if text == "hi"));
    // The reply grows: it is sent whole again, once.
    st.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": " there"}}));
    let (delta2, _) = st.view("s", Some(delta.rev), Some(first.epoch), info_rev);
    assert!(matches!(&delta2.entries[0].body, AcpBody::Message { text, .. } if text == "hi there"));
    // Another epoch (the server restarted), or a revision from the future: everything again.
    assert!(st.view("s", Some(2), Some(7), 0).0.reset);
    assert!(st.view("s", Some(9999), Some(42), 0).0.reset);
    assert!(st.view("s", Some(delta.rev), None, 0).0.reset);
}

#[test]
fn turn_end_cancels_open_permissions_and_a_dead_agent_says_so() {
    let mut st = Stream::new("a", "/w", 1);
    let params = json!({"toolCall": {"toolCallId": "t", "title": "Run rm", "kind": "execute"},
        "options": [{"optionId": "ok", "kind": "allow_once", "name": "Yes"}]});
    st.permission("acp-1", &params, crate::acp::permission_options(&params));
    st.user("go");
    assert!(st.info.working);
    st.turn_end(Some("cancelled"), None);
    let b = bodies(&mut st);
    assert!(b
        .iter()
        .any(|e| matches!(e, AcpBody::Permission { state, .. } if state == "cancelled")));
    assert!(!st.info.working);
    st.status("failed", Some("exit status: 1"));
    assert_eq!(st.info.status, "failed");
    assert!(bodies(&mut st)
        .iter()
        .any(|e| matches!(e, AcpBody::Notice { level, .. } if level == "error")));
}

#[test]
fn diffs_are_capped_and_marked_and_only_diff_content_is_copied() {
    let mut st = Stream::new("a", "/w", 1);
    let big = "line of code\n".repeat(500);
    st.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t", "title": "Write big", "kind": "edit",
        "content": [
            {"type": "diff", "path": "/w/big.rs", "oldText": null, "newText": big},
            {"type": "content", "content": {"type": "text", "text": "SECRET-OUTPUT"}}
        ], "rawInput": {"content": "SECRET-RAW"}}));
    let wire = serde_json::to_string(&all(&mut st).0).unwrap();
    assert!(!wire.contains("SECRET-OUTPUT") && !wire.contains("SECRET-RAW"));
    let AcpBody::Tool { diffs, .. } = &bodies(&mut st)[0] else {
        panic!()
    };
    assert_eq!(diffs.len(), 1);
    assert!(diffs[0].truncated);
    assert!(diffs[0].new_text.lines().count() <= DIFF_LINES);
}

#[test]
fn thousands_of_chunks_cost_one_cleaning_pass_per_read_not_one_per_chunk() {
    let mut st = Stream::new("a", "/w", 1);
    let chunk = json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "word "}});
    let started = std::time::Instant::now();
    for _ in 0..9000 {
        st.update(&chunk);
    }
    let (view, _) = st.view("s", None, None, 0);
    // 9000 chunks of 5 bytes fit in the raw buffer: the reply is the first 16 000 characters, cleaned once.
    let AcpBody::Message { text, .. } = &view.entries[0].body else {
        panic!()
    };
    assert_eq!(text.chars().count(), 16_000);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    // A read cleans what arrived since the last one; an unchanged stream is not stamped again.
    let rev = st.rev();
    let _ = st.view("s", None, None, 0);
    assert_eq!(st.rev(), rev);
}

fn msg_chunk(t: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}})
}

fn truncated_of(b: &AcpBody) -> bool {
    match b {
        AcpBody::Message { truncated, .. } | AcpBody::Thought { truncated, .. } => *truncated,
        other => panic!("not a reply: {other:?}"),
    }
}

#[test]
fn a_reply_that_was_cut_says_so_and_a_short_one_does_not() {
    let mut st = Stream::new("a", "/w", 1);
    st.update(&msg_chunk("short and whole"));
    assert!(!truncated_of(&bodies(&mut st)[0]));
    // Cut by the character limit.
    st.user("next");
    st.update(&msg_chunk(&"word ".repeat(4_000)));
    let b = bodies(&mut st);
    assert!(
        truncated_of(&b[2]),
        "20 000 characters become 16 000 and the entry says so"
    );
    // Cut by the line limit.
    st.user("lines");
    st.update(&msg_chunk(&"x\n".repeat(MESSAGE_LINES + 10)));
    assert!(truncated_of(&bodies(&mut st)[4]));
    // Cut because the raw buffer was full, even though the shown text is short: the tail is a blank run.
    st.user("raw");
    st.update(&msg_chunk(&"a".repeat(100)));
    st.update(&msg_chunk(&" ".repeat(RAW_BYTES)));
    st.update(&msg_chunk("tail that was never kept"));
    assert!(truncated_of(&bodies(&mut st)[6]));
    // A thought is marked the same way.
    st.user("think");
    st.update(&json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "w ".repeat(10_000)}}));
    assert!(truncated_of(&bodies(&mut st)[8]));
}

#[test]
fn a_reply_past_the_cut_is_not_sent_again_while_it_streams_on() {
    let mut st = Stream::new("a", "/w", 1);
    st.update(&msg_chunk(&"word ".repeat(4_000)));
    let (first, _) = all(&mut st);
    let seen = first.rev;
    // 4 000 more chunks: the shown text (the first 16 000 characters) cannot change.
    for _ in 0..4_000 {
        st.update(&msg_chunk("more words "));
    }
    let (delta, _) = st.view("s", Some(seen), Some(first.epoch), 0);
    assert!(!delta.reset);
    assert!(
        delta.entries.is_empty(),
        "an identical body was sent again: {}",
        delta.entries.len()
    );
}

#[test]
fn an_entry_that_had_to_be_halved_to_fit_says_it_was_cut() {
    let mut e = AcpEntry {
        id: 1,
        rev: 1,
        body: AcpBody::Message {
            text: "😀\"\n".repeat(7_000),
            truncated: false,
        },
    };
    fit(&mut e);
    assert!(truncated_of(&e.body));
}

/// What the client does with the parts, as `lib/acpStream.ts` does: a reset replaces, a part adds or replaces by id.
fn apply(held: &mut std::collections::BTreeMap<u64, AcpEntry>, held_rev: &mut u64, v: &AcpView) {
    if v.reset {
        held.clear();
    }
    for e in &v.entries {
        held.insert(e.id, e.clone());
    }
    *held_rev = v.rev;
}

#[test]
fn a_view_is_cut_into_messages_the_direct_transport_can_carry_and_loses_nothing() {
    let mut st = Stream::new("a", "/w", 1);
    // Worst-case text: four-byte characters, quotes and newlines (JSON escapes), in every kind of entry.
    let nasty = "😀\"\n".repeat(5_000);
    for i in 0..60 {
        st.user(&format!("{i} {nasty}"));
        st.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": nasty}}));
        st.update(&json!({"sessionUpdate": "tool_call", "toolCallId": format!("t{i}"), "title": "Write", "kind": "edit",
            "content": [{"type": "diff", "path": "/w/a", "oldText": nasty, "newText": nasty}, {"type": "diff", "path": "/w/b", "oldText": nasty, "newText": nasty}]}));
    }
    st.update(&json!({"sessionUpdate": "available_commands_update",
        "availableCommands": (0..200).map(|i| json!({"name": format!("cmd{i}-{}", "é".repeat(60)), "description": "ü".repeat(160), "input": {"hint": "ß".repeat(80)}})).collect::<Vec<_>>()}));
    let (view, _) = st.view("s", None, None, 0);
    let total = view.entries.len();
    let parts = split(view.clone());
    assert!(
        parts.len() > 3,
        "a big view is several messages ({})",
        parts.len()
    );
    assert!(
        parts[0].reset && parts[1..].iter().all(|p| !p.reset),
        "only the first part resets"
    );
    for p in &parts {
        let n = serde_json::to_string(&ServerMessage::AcpView(p.clone()))
            .unwrap()
            .len();
        assert!(
            n < 64 * 1024,
            "a message of {n} bytes would close the direct connection"
        );
    }
    // After every part the client holds every entry up to that part's revision: a drop mid-way resumes without a gap.
    let (mut held, mut rev) = (std::collections::BTreeMap::new(), 0);
    let mut last = 0;
    for p in &parts {
        apply(&mut held, &mut rev, p);
        assert!(p.rev >= last, "revisions never go back");
        last = p.rev;
        let missing = view
            .entries
            .iter()
            .filter(|e| e.rev <= rev && !held.contains_key(&e.id))
            .count();
        assert_eq!(missing, 0, "entries at or below rev {rev} are held");
    }
    assert_eq!(held.len(), total);
    assert_eq!(rev, view.rev);
    // The facts come last, shortened to fit.
    let facts = parts.last().unwrap().info.as_ref().expect("the facts");
    assert!(!facts.commands.is_empty() && facts.commands.len() < 200);
    assert!(parts[..parts.len() - 1].iter().all(|p| p.info.is_none()));
}

#[test]
fn a_small_or_empty_view_is_one_message() {
    let mut st = Stream::new("a", "/w", 1);
    assert_eq!(
        split(st.view("s", None, None, 0).0).len(),
        2,
        "an empty reset and the facts"
    );
    st.user("hi");
    let (delta, _) = st.view("s", Some(0), Some(1), 99);
    let parts = split(delta);
    assert_eq!(parts.len(), 1);
    assert!(!parts[0].reset && parts[0].entries.len() == 1 && parts[0].info.is_none());
}
