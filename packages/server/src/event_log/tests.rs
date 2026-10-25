use super::*;
use serde_json::json;
use std::collections::BTreeMap;

fn chunk(kind: &str, text: &str) -> Value {
    json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}})
}
fn call(id: &str, title: &str) -> Value {
    json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": title, "kind": "edit", "status": "in_progress",
           "rawInput": {"command": "secret"}})
}
fn tool_update(id: &str, status: &str, text: Option<&str>) -> Value {
    let mut u = json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": status});
    if let Some(t) = text {
        u["content"] = json!([{"type": "content", "content": {"type": "text", "text": t}}]);
    }
    u
}
fn plan(done: usize) -> Value {
    let entries: Vec<Value> = (0..3)
        .map(|i| json!({"content": format!("step {i}"), "priority": "medium", "status": if i < done {"completed"} else {"pending"}}))
        .collect();
    json!({"sessionUpdate": "plan", "entries": entries})
}

/// The state a client builds from `session/update` frames: the folding every ACP client does, kept simple.
#[derive(Debug, Default, PartialEq)]
struct State {
    /// Ordered: `("agent_message_chunk", text)`, `("tool", id)`.
    items: Vec<(String, String)>,
    /// Per tool: status and joined content text; the title is the last one sent.
    tools: BTreeMap<String, (String, String, String)>,
    plan: Option<Value>,
    mode: Option<Value>,
    usage: Option<Value>,
    title: Option<Value>,
    commands: Option<Value>,
}

fn fold(frames: &[Value]) -> State {
    let mut st = State::default();
    for f in frames {
        let u = &f["update"];
        let kind = u["sessionUpdate"].as_str().unwrap();
        match kind {
            "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk" => {
                let t = u["content"]["text"].as_str().unwrap().to_string();
                match st.items.last_mut() {
                    Some((k, text)) if k == kind => text.push_str(&t),
                    _ => st.items.push((kind.to_string(), t)),
                }
            }
            "tool_call" | "tool_call_update" => {
                let id = u["toolCallId"].as_str().unwrap().to_string();
                if !st.tools.contains_key(&id) {
                    st.items.push(("tool".into(), id.clone()));
                }
                let e = st.tools.entry(id).or_default();
                if let Some(t) = u["title"].as_str() {
                    e.0 = t.into();
                }
                if let Some(s) = u["status"].as_str() {
                    e.1 = s.into();
                }
                if let Some(c) = u["content"].as_array() {
                    // A sent `content` replaces the earlier one (ACP: update fields replace).
                    e.2 = c
                        .iter()
                        .filter_map(|x| x["content"]["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("|");
                }
            }
            "plan" => st.plan = Some(u["entries"].clone()),
            "current_mode_update" => st.mode = Some(u["currentModeId"].clone()),
            "usage_update" => st.usage = Some(json!([u["used"], u["size"]])),
            "session_info_update" => st.title = Some(u["title"].clone()),
            "available_commands_update" => st.commands = Some(u["availableCommands"].clone()),
            other => panic!("unexpected {other}"),
        }
    }
    st
}

/// A turn of `n` rounds: a reply streamed in chunks, a tool call with updates, a plan step, usage.
fn turn(n: usize) -> Vec<Value> {
    let mut v = vec![json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"})];
    for i in 0..n {
        for part in ["Let me ", "look at ", &format!("file {i}. ")] {
            v.push(chunk("agent_message_chunk", part));
        }
        v.push(call(&format!("t{i}"), &format!("Edit {i}")));
        v.push(tool_update(
            &format!("t{i}"),
            "in_progress",
            Some("working"),
        ));
        v.push(tool_update(&format!("t{i}"), "completed", Some("done")));
        v.push(plan(i.min(3)));
        v.push(json!({"sessionUpdate": "usage_update", "used": 100 * (i + 1), "size": 200000}));
    }
    v
}

fn log_with(budget: usize, updates: &[Value]) -> EventLog {
    let mut log = EventLog::new("sess-1", 77, budget);
    for u in updates {
        log.push(u).unwrap();
    }
    log
}

fn seqs(frames: &[Value]) -> Vec<u64> {
    frames.iter().map(|f| stamp_of(f).unwrap().seq).collect()
}

#[test]
fn replay_resume_mid_turn_is_gapless() {
    let all = turn(6);
    let log = log_with(DEFAULT_BUDGET_BYTES, &all);
    assert_eq!(log.last_seq(), all.len() as u64);
    // The client applied the first 17 frames and was cut off.
    let held = fold(&log.resume(None, None).frames[..0]);
    assert_eq!(held, State::default());
    let full = log.resume(Some(0), Some(77));
    assert!(!full.reset);
    assert_eq!(
        seqs(&full.frames),
        (1..=all.len() as u64).collect::<Vec<_>>()
    );
    let tail = log.resume(Some(17), Some(77));
    assert!(!tail.reset, "the log holds everything after 17");
    assert_eq!(
        seqs(&tail.frames),
        (18..=all.len() as u64).collect::<Vec<_>>()
    );
    // Head (what the client applied) + tail = the full stream, with no gap and no duplicate.
    let head = &full.frames[..17];
    let joined: Vec<Value> = head.iter().chain(&tail.frames).cloned().collect();
    assert_eq!(fold(&joined), fold(&full.frames));
    // Caught up: nothing to send, and it is not a reset.
    let none = log.resume(Some(log.last_seq()), Some(77));
    assert!(!none.reset && none.frames.is_empty());
    assert_eq!(none.last, log.last_seq());
}

#[test]
fn replay_after_the_ring_wrapped_is_a_reset_that_rebuilds_the_same_state() {
    let all = turn(40);
    let full = fold(
        &log_with(usize::MAX / 2, &all)
            .resume(Some(0), Some(77))
            .frames,
    );
    // A ring far smaller than the stream: most of it fell out.
    let big = log_with(usize::MAX / 2, &all);
    let small = log_with(big.bytes() * 2 / 3, &all);
    eprintln!(
        "full {} B in {} frames; small ring {} B: {} raw + {} compacted",
        big.bytes(),
        all.len(),
        small.bytes(),
        small.ring.len(),
        small.base.len()
    );
    assert!(
        small.ring.len() < all.len() * 3 / 4 && !small.base.is_empty(),
        "{} raw frames kept",
        small.ring.len()
    );
    let r = small.resume(Some(3), Some(77));
    assert!(r.reset, "frame 4 is long gone");
    assert_eq!(r.last, all.len() as u64);
    // Compacted: far fewer frames than the stream, still standard updates, flagged reset on the first.
    assert!(r.frames.len() < all.len() / 2, "{} frames", r.frames.len());
    let first = r.frames[0]["update"]["_meta"].as_object().unwrap().clone();
    let reset = ExtMeta(first).get::<Reset>(&both_ways()).unwrap().unwrap();
    assert_eq!(reset.last, all.len() as u64);
    // Applying it gives the state the full stream gives.
    assert_eq!(fold(&r.frames), full);
    // Chunks are merged and each tool is one call.
    let kinds: Vec<&str> = r
        .frames
        .iter()
        .map(|f| f["update"]["sessionUpdate"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "tool_call_update").count(),
        0
    );
    assert_eq!(kinds.iter().filter(|k| **k == "plan").count(), 1);
    // The seq of every compacted frame is that of the last original it covers, so the client resumes after `last`.
    assert_eq!(seqs(&r.frames).iter().max().copied(), Some(r.last));
    let again = small.resume(Some(r.last), Some(77));
    assert!(!again.reset && again.frames.is_empty());
}

#[test]
fn replay_a_different_epoch_or_a_client_holding_nothing_gets_a_reset() {
    let all = turn(3);
    let log = log_with(DEFAULT_BUDGET_BYTES, &all);
    let full = fold(&log.resume(Some(0), Some(77)).frames);
    for (after, epoch) in [
        (Some(5), Some(78)),
        (None, None),
        (Some(5), None),
        (None, Some(77)),
    ] {
        let r = log.resume(after, epoch);
        assert!(r.reset, "{after:?} {epoch:?}");
        assert_eq!(fold(&r.frames), full, "{after:?} {epoch:?}");
        assert_eq!(r.epoch, 77);
    }
    // A client from the future (the log was rebuilt shorter) is reset too.
    assert!(log.resume(Some(10_000), Some(77)).reset);
    // An empty log: a client at 0 is current; anyone else resets to nothing.
    let empty = EventLog::new("s", 1, 1000);
    assert!(!empty.resume(Some(0), Some(1)).reset);
    assert!(empty.resume(None, None).reset && empty.resume(None, None).frames.is_empty());
}

#[test]
fn replay_the_ring_never_exceeds_its_byte_budget() {
    let budget = 20_000;
    let mut log = EventLog::new("s", 1, budget);
    let mut peak = 0;
    for round in 0..3000 {
        log.push(&chunk(
            "agent_message_chunk",
            &format!("a reply that goes on and on {round} "),
        ))
        .unwrap();
        log.push(&call(&format!("t{round}"), &format!("Tool number {round}")))
            .unwrap();
        log.push(&tool_update(&format!("t{round}"), "completed", Some("ok")))
            .unwrap();
        peak = peak.max(log.bytes());
        assert!(
            log.bytes() <= budget,
            "{} bytes after round {round}",
            log.bytes()
        );
    }
    assert!(peak > budget / 2, "the test filled the ring: peak {peak}");
    assert_eq!(log.last_seq(), 9000);
    // The process-wide figure the health sample reports includes this log (other tests' logs share it, so only a
    // lower bound can be asserted).
    assert!(total_bytes() >= log.bytes() as u64);
}

#[test]
fn replay_frames_are_scrubbed_stamped_and_plain_without_the_extensions() {
    let mut log = EventLog::new("sess-1", 5, DEFAULT_BUDGET_BYTES);
    let seq = log
        .push(&json!({"sessionUpdate": "tool_call", "toolCallId": "t", "title": "run\u{1b}[31m it", "rawInput": {"k": "v"}, "rawOutput": "x",
                       "content": [{"type": "content", "content": {"type": "text", "text": "ok\u{7}"}}]}))
        .unwrap();
    let r = log.resume(Some(0), Some(5));
    let u = &r.frames[0]["update"];
    assert_eq!(r.frames[0]["sessionId"], "sess-1");
    assert!(
        u.get("rawInput").is_none() && u.get("rawOutput").is_none(),
        "raw tool data is never copied"
    );
    assert!(!u.to_string().contains('\u{1b}') && !u.to_string().contains('\u{7}'));
    assert_eq!(stamp_of(&r.frames[0]), Some(SeqStamp { seq, epoch: 5 }));
    // A peer that did not negotiate the extensions gets plain ACP: no termenbox key left, and it still decodes.
    let plain = plain(&r.frames);
    assert!(plain[0]["update"]["_meta"].as_object().unwrap().is_empty());
    assert!(
        serde_json::from_value::<agent_client_protocol_schema::v1::SessionUpdate>(
            plain[0]["update"].clone()
        )
        .is_ok()
    );
    // What is not an object, or too large, is dropped and counted, and the sequence has no hole.
    assert_eq!(log.push(&json!("x")), None);
    assert_eq!(
        log.push(
            &json!({"sessionUpdate": "plan", "entries": [], "pad": vec!["word ".repeat(1600); 100]})
        ),
        None
    );
    assert_eq!(log.dropped(), 2);
    assert_eq!(
        log.push(&chunk("agent_message_chunk", "next")),
        Some(seq + 1)
    );
}

/// The default ring size, measured: the biggest recorded conversation in `testdata/acp/` scrubs to this many bytes
/// of `session/update` frames, so the default holds several such conversations raw.
#[test]
fn replay_event_log_budget_default_is_a_multiple_of_the_largest_recorded_conversation() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/acp");
    let mut largest = (0usize, String::new());
    for agent in std::fs::read_dir(&root).unwrap() {
        let agent = agent.unwrap().path();
        if !agent.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(&agent).unwrap() {
            let f = f.unwrap().path();
            if f.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let mut log = EventLog::new("s", 1, usize::MAX / 2);
            for line in std::fs::read_to_string(&f)
                .unwrap()
                .lines()
                .filter(|l| !l.trim().is_empty())
            {
                let rec: Value = serde_json::from_str(line).unwrap();
                if rec["dir"] == "in" && rec["msg"]["method"] == "session/update" {
                    log.push(&rec["msg"]["params"]["update"]);
                }
            }
            if log.bytes() > largest.0 {
                largest = (log.bytes(), f.display().to_string());
            }
        }
    }
    eprintln!(
        "largest recorded conversation: {} bytes of frames ({})",
        largest.0, largest.1
    );
    assert!(largest.0 > 0);
    assert!(
        DEFAULT_BUDGET_BYTES >= largest.0 * 2,
        "the default {DEFAULT_BUDGET_BYTES} is under twice the largest conversation {}",
        largest.0
    );
}
