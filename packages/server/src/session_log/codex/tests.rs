use std::path::{Path, PathBuf};

use super::super::contract::{self, Case, Cases};
use super::*;

const DIR: &str = "/home/fixture/project";
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/session_log/codex");

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termenbox-codex-log-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn session(id: &str) -> SessionRef {
    SessionRef {
        agent: AGENT.into(),
        id: id.into(),
        dir: PathBuf::from(DIR),
    }
}

fn case(id: &str, phrase: &'static str) -> Case {
    Case {
        session: session(id),
        phrase,
    }
}

/// The fixtures copied under a sessions directory in a date folder, named as Codex names a rollout for the id.
fn sessions(root: &Path) -> PathBuf {
    let dir = root.join("2026").join("10").join("31");
    std::fs::create_dir_all(&dir).unwrap();
    for f in std::fs::read_dir(FIXTURES).unwrap() {
        let f = f.unwrap().path();
        if f.extension().is_some_and(|e| e == "jsonl") {
            let id = f.file_stem().unwrap().to_string_lossy().into_owned();
            std::fs::copy(
                &f,
                dir.join(format!("rollout-2026-10-31T10-00-00-{id}.jsonl")),
            )
            .unwrap();
        }
    }
    dir
}

fn cases(dir: &Path) -> Cases {
    Cases {
        short: case("short", "by size"),
        file: dir.join("rollout-2026-10-31T10-00-00-short.jsonl"),
        branch: None,
        torn: (case("torn", "list files"), "cut off mid"),
        truncated: session("truncated"),
        unknown_version: session("unknown-version"),
        tool: (
            case("tool-secret", "will not repeat"),
            vec![
                "sk-FAKE-0123456789abcdef0123456789abcdef",
                "DEPLOY_TOKEN",
                "Script completed",
            ],
        ),
        unmappable: None,
        secret_text: (case("secret-text", "visible reply"), "hunter2xyz"),
    }
}

#[test]
fn the_codex_adapter_passes_the_contract_suite() {
    let root = tmp();
    let dir = sessions(&root);
    // The suite needs a session whose text the scrubber hides; the recorded fixtures have none.
    let mut body = std::fs::read_to_string(dir.join("rollout-2026-10-31T10-00-00-short.jsonl"))
        .unwrap()
        .lines()
        .take(1)
        .map(|l| format!("{l}\n"))
        .collect::<String>();
    body.push_str(r#"{"timestamp":"2026-10-31T10:00:02.000Z","ordinal":1,"type":"response_item","payload":{"type":"message","id":"m1","role":"assistant","content":[{"type":"output_text","text":"The visible reply. password=hunter2xyz"}]}}"#);
    body.push('\n');
    std::fs::write(
        dir.join("rollout-2026-10-31T10-00-00-secret-text.jsonl"),
        body,
    )
    .unwrap();
    let log = CodexLog::new(root.clone());
    contract::run(&log, &cases(&dir));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn setup_messages_and_events_are_skipped_and_tools_show_a_name_only() {
    let root = tmp();
    sessions(&root);
    let log = CodexLog::new(root.clone());
    let all = log
        .page(&session("short"), None, Direction::Older, 100)
        .unwrap()
        .entries;
    let texts: Vec<(&str, Kind)> = all.iter().map(|e| (e.turn.as_str(), e.kind)).collect();
    assert_eq!(
        texts,
        [
            ("3", Kind::User),
            ("4", Kind::Message),
            ("8", Kind::User),
            ("9", Kind::Message)
        ],
        "the developer setup message and the event stream are not shown"
    );
    let tool = log
        .page(&session("tool-secret"), None, Direction::Older, 100)
        .unwrap()
        .entries;
    let kinds: Vec<(Kind, &str)> = tool.iter().map(|e| (e.kind, e.text.as_str())).collect();
    assert!(kinds.contains(&(Kind::Tool, "exec")));
    assert!(kinds.contains(&(Kind::Notice, "Conversation compacted")));
    let hits = log.search(&session("short"), "by size", 10).unwrap();
    assert_eq!(hits.len(), 2, "the user question and the assistant answer");
    assert_eq!(hits[0].turn, "4", "newest first");
    assert_eq!(hits[0].at.as_deref(), Some("2026-10-31T10:00:04.000Z"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_fork_reads_as_one_conversation_with_the_parents_records() {
    let root = tmp();
    sessions(&root);
    let log = CodexLog::new(root.clone());
    let hits = log.search(&session("fork"), "FORKED-BRANCH", 10).unwrap();
    assert_eq!(hits.len(), 2);
    let w = log.window(&session("fork"), &hits[1].turn, 5, 0).unwrap();
    assert_eq!(
        w.entries.first().unwrap().text,
        "How do I list files by size in bash?"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_window_returns_the_hit_turn_and_fits_the_cap() {
    let root = tmp();
    let dir = sessions(&root);
    let big = "lorem ipsum ".repeat(1200);
    let head = std::fs::read_to_string(dir.join("rollout-2026-10-31T10-00-00-short.jsonl"))
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    let mut body = format!("{head}\n");
    for i in 1..=30 {
        let (role, ty) = if i % 2 == 1 {
            ("user", "input_text")
        } else {
            ("assistant", "output_text")
        };
        body.push_str(&format!(
            r#"{{"timestamp":"2026-10-31T10:00:00.000Z","ordinal":{i},"type":"response_item","payload":{{"type":"message","role":"{role}","content":[{{"type":"{ty}","text":"{big} needle{i}"}}]}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(dir.join("rollout-2026-10-31T10-00-00-big.jsonl"), body).unwrap();
    let log = CodexLog::new(root.clone());
    let hits = log.search(&session("big"), "needle15", 5).unwrap();
    assert_eq!(hits[0].turn, "15");
    let w = log.window(&session("big"), "15", 200, 200).unwrap();
    assert!(w.entries.iter().any(|e| e.turn == "15"));
    let size: usize = w
        .entries
        .iter()
        .map(|e| e.text.len() + e.turn.len() + 48)
        .sum();
    assert!(size <= WINDOW_BYTES, "a window fits 48 KiB, was {size}");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_session_in_another_directory_or_with_an_odd_id_is_not_read() {
    let root = tmp();
    sessions(&root);
    let log = CodexLog::new(root.clone());
    let mut other = session("short");
    other.dir = PathBuf::from("/home/fixture/elsewhere");
    assert_eq!(log.search(&other, "by size", 5), Err(LogError::Missing));
    for id in ["../short", "", "sh/ort", "hort"] {
        assert_eq!(
            log.search(&session(id), "by size", 5),
            Err(LogError::Missing),
            "id {id:?}"
        );
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_session_still_being_born_is_empty_not_off() {
    let root = tmp();
    let dir = sessions(&root);
    std::fs::write(
        dir.join("rollout-2026-10-31T10-00-00-born.jsonl"),
        r#"{"timestamp":"2026-10-31T10:00:00.000Z","ordinal":0,"type":"session_me"#,
    )
    .unwrap();
    let log = CodexLog::new(root.clone());
    // Its directory is not known yet, so it is not read; and it is not turned off either.
    assert_eq!(log.search(&session("born"), "x", 5), Err(LogError::Missing));
    assert!(log.search(&session("short"), "by size", 5).is_ok());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn appends_keep_a_cursor_and_the_cache_follows_the_file() {
    use std::io::Write;
    let root = tmp();
    let dir = sessions(&root);
    let file = dir.join("rollout-2026-10-31T10-00-00-short.jsonl");
    let log = CodexLog::new(root.clone());
    let s = session("short");
    let first = log.page(&s, None, Direction::Older, 1).unwrap();
    let cursor = first.next.unwrap();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    writeln!(f, r#"{{"timestamp":"2026-10-31T10:00:12.000Z","ordinal":12,"type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"appended later"}}]}}}}"#).unwrap();
    drop(f);
    assert!(log.page(&s, Some(&cursor), Direction::Older, 1).is_ok());
    assert_eq!(log.search(&s, "appended later", 5).unwrap().len(), 1);
    let last = log.page(&s, None, Direction::Older, 1).unwrap();
    assert_eq!(last.entries[0].text, "appended later");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn shared_caps_count_hits_across_sessions_and_stop_at_the_budget() {
    let root = tmp();
    sessions(&root);
    let log = CodexLog::new(root.clone());
    let limits = Limits {
        max_hits: 3,
        per_session: 2,
        ..Limits::default()
    };
    let mut run = SearchRun::new(limits, Cancel::default());
    assert_eq!(
        log.search_in(&session("short"), "by size", &mut run)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        log.search_in(&session("fork"), "by size", &mut run)
            .unwrap()
            .len(),
        1
    );
    assert!(log
        .search_in(&session("short"), "by size", &mut run)
        .unwrap()
        .is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_needle_in_a_big_tool_output_costs_no_parse() {
    use super::scan::tests::allocs;
    use std::io::Write;
    let root = tmp();
    let dir = sessions(&root);
    let log = CodexLog::new(root.clone());
    let rollout = |id: &str| dir.join(format!("rollout-2026-10-31T10-00-00-{id}.jsonl"));
    for (id, filler) in [
        ("withneedle", "zebra quartz "),
        ("without", "filler words "),
    ] {
        std::fs::copy(rollout("short"), rollout(id)).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(rollout(id))
            .unwrap();
        let out = filler.repeat(40_000);
        writeln!(f, r#"{{"timestamp":"2026-10-31T10:00:20.000Z","ordinal":90,"type":"response_item","payload":{{"type":"function_call_output","call_id":"c1","output":"{out}"}}}}"#).unwrap();
    }
    let count = |id: &str| {
        let s = session(id);
        log.search(&s, "zebra quartz", 5).unwrap();
        let before = allocs();
        let hits = log.search(&s, "zebra quartz", 5).unwrap();
        (allocs() - before, hits)
    };
    let (with, hits) = count("withneedle");
    let (without, _) = count("without");
    assert!(hits.is_empty(), "tool output is never a hit");
    assert_eq!(with, without, "the tool output was parsed");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn only_a_message_or_reasoning_line_is_worth_parsing() {
    assert!(may_hold_text(
        br#"{"type":"response_item","payload":{"type":"message","role":"user","content":[]}}"#
    ));
    assert!(may_hold_text(
        br#"{"type":"response_item","payload":{"type":"reasoning","summary":[]}}"#
    ));
    assert!(!may_hold_text(
        br#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{}"}}"#
    ));
    assert!(!may_hold_text(
        br#"{"type":"response_item","payload":{"type":"function_call_output","output":"{\"type\":\"message\"}"}}"#
    ));
}
