use std::path::{Path, PathBuf};

use super::super::contract::{self, Case, Cases};
use super::*;

const DIR: &str = "/home/fixture/project";
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/session_log/claude");

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termenbox-claude-log-{}", uuid::Uuid::new_v4()));
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

/// The fixtures copied under a projects directory, where `ClaudeLog` looks for them.
fn projects(root: &Path) -> PathBuf {
    let dir = root.join("-home-fixture-project");
    std::fs::create_dir_all(&dir).unwrap();
    for f in std::fs::read_dir(FIXTURES).unwrap() {
        let f = f.unwrap().path();
        if f.extension().is_some_and(|e| e == "jsonl") {
            std::fs::copy(&f, dir.join(f.file_name().unwrap())).unwrap();
        }
    }
    dir
}

fn cases(dir: &Path) -> Cases {
    Cases {
        short: case("short", "by size"),
        file: dir.join("short.jsonl"),
        branch: Some((session("rewind"), "abandoned-branch", "newest files")),
        torn: (case("torn", "first question"), "cut off mid"),
        truncated: session("truncated"),
        unknown_version: session("unknown-version"),
        tool: (
            case("tool-secret", "deploy listing"),
            vec![
                "sk-FAKE-0123456789abcdef0123456789abcdef",
                "echo DEPLOY_TOKEN",
                "Print token",
            ],
        ),
        unmappable: Some((session("unmappable"), "x3")),
        secret_text: (case("secret-text", "visible reply"), "hunter2xyz"),
    }
}

#[test]
fn the_claude_adapter_passes_the_contract_suite() {
    let root = tmp();
    let dir = projects(&root);
    let log = ClaudeLog::new(root.clone());
    contract::run(&log, &cases(&dir));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_search_on_a_rewound_session_finds_only_the_active_branch_with_the_hit_turn() {
    let root = tmp();
    projects(&root);
    let log = ClaudeLog::new(root.clone());
    let hits = log.search(&session("rewind"), "newest files", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].turn, "a7", "newest first");
    assert_eq!(hits[1].turn, "u6");
    assert_eq!(hits[0].kind, Kind::Message);
    assert_eq!(hits[1].kind, Kind::User);
    assert_eq!(hits[0].at.as_deref(), Some("2026-10-31T10:00:07.500Z"));
    let w = log.window(&session("rewind"), "u6", 5, 5).unwrap();
    let turns: Vec<&str> = w.entries.iter().map(|e| e.turn.as_str()).collect();
    assert_eq!(
        turns,
        ["u1", "a2", "u6", "a7"],
        "the dropped branch is skipped"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn thoughts_are_searchable_and_tools_show_a_name_only() {
    let root = tmp();
    projects(&root);
    let log = ClaudeLog::new(root.clone());
    let hits = log.search(&session("short"), "pipe to head", 10).unwrap();
    assert!(hits
        .iter()
        .any(|h| h.kind == Kind::Thought && h.turn == "a4"));
    let tool = log
        .page(&session("tool-secret"), None, Direction::Older, 100)
        .unwrap();
    let kinds: Vec<(Kind, &str)> = tool
        .entries
        .iter()
        .map(|e| (e.kind, e.text.as_str()))
        .collect();
    assert!(kinds.contains(&(Kind::Tool, "Bash")));
    assert!(kinds.contains(&(Kind::Notice, "Conversation compacted")));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_window_returns_the_hit_turn_and_fits_the_cap() {
    let root = tmp();
    let dir = projects(&root);
    // 30 turns of about 15 KB each: the whole session is far over 48 KiB.
    let big = "lorem ipsum ".repeat(1200);
    let mut body = std::fs::read_to_string(dir.join("short.jsonl"))
        .unwrap()
        .lines()
        .take(4)
        .map(|l| format!("{l}\n"))
        .collect::<String>();
    let mut parent = "null".to_string();
    for i in 0..30 {
        let (ty, role) = if i % 2 == 0 {
            ("user", "user")
        } else {
            ("assistant", "assistant")
        };
        let content = if i % 2 == 0 {
            format!("\"{big} needle{i}\"")
        } else {
            format!("[{{\"type\":\"text\",\"text\":\"{big} needle{i}\"}}]")
        };
        let parent_json = if i == 0 {
            "null".to_string()
        } else {
            format!("\"{parent}\"")
        };
        body.push_str(&format!(
            "{{\"parentUuid\":{parent_json},\"type\":\"{ty}\",\"uuid\":\"n{i}\",\"timestamp\":\"2026-10-31T10:00:00Z\",\"message\":{{\"role\":\"{role}\",\"content\":{content}}},\"version\":\"2.1.308\"}}\n"
        ));
        parent = format!("n{i}");
    }
    std::fs::write(dir.join("big.jsonl"), body).unwrap();
    let log = ClaudeLog::new(root.clone());
    let hits = log.search(&session("big"), "needle17", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].turn, "n17");
    let w = log.window(&session("big"), &hits[0].turn, 10, 10).unwrap();
    assert!(
        w.entries.iter().any(|e| e.turn == "n17"),
        "the window holds the hit's turn"
    );
    let bytes: usize = w
        .entries
        .iter()
        .map(|e| e.text.len() + e.turn.len() + 48)
        .sum();
    assert!(bytes <= WINDOW_BYTES, "{bytes} bytes");
    assert!(
        w.entries.len() >= 3,
        "it still shows context around the hit"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn fit_window_cuts_from_the_far_ends_and_keeps_the_anchor() {
    let e = |t: &str, n: usize| Entry::new(t, Kind::Message, &"word ".repeat(n / 5));
    let mut v = vec![
        e("a", 400),
        e("b", 400),
        e("c", 400),
        e("d", 400),
        e("e", 400),
    ];
    let at = fit_window(&mut v, 2, 1200);
    assert_eq!(v[at].turn, "c");
    assert!(v.len() < 5 && v.len() >= 2);
    let mut one = vec![e("a", 4000)];
    fit_window(&mut one, 0, 1000);
    assert!(one[0].text.len() <= 1000 && one[0].truncated);
    assert!(one[0].text.len() > 500, "it keeps what fits");
}

#[test]
fn a_session_in_another_directory_or_with_a_odd_id_is_not_read() {
    let root = tmp();
    projects(&root);
    let log = ClaudeLog::new(root.clone());
    let elsewhere = SessionRef {
        dir: PathBuf::from("/somewhere/else"),
        ..session("short")
    };
    assert_eq!(log.search(&elsewhere, "by size", 5), Err(LogError::Missing));
    for id in ["../short", "a/b", "", "short.jsonl"] {
        assert_eq!(
            log.search(&session(id), "x", 5),
            Err(LogError::Missing),
            "{id:?}"
        );
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_session_still_being_born_is_empty_not_off() {
    let root = tmp();
    let dir = projects(&root);
    std::fs::write(dir.join("new.jsonl"), "{\"type\":\"queue-oper").unwrap();
    std::fs::write(dir.join("empty.jsonl"), "").unwrap();
    let log = ClaudeLog::new(root.clone());
    for id in ["new", "empty"] {
        assert_eq!(log.search(&session(id), "x", 5), Ok(vec![]));
        assert_eq!(
            log.page(&session(id), None, Direction::Older, 5)
                .unwrap()
                .entries,
            vec![]
        );
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn appends_keep_a_cursor_and_the_cache_follows_the_file() {
    let root = tmp();
    let dir = projects(&root);
    let log = ClaudeLog::new(root.clone());
    let s = session("short");
    let first = log.page(&s, None, Direction::Older, 1).unwrap();
    assert_eq!(first.entries[0].turn, "a5");
    let line = r#"{"parentUuid":"a5","type":"assistant","uuid":"a6","timestamp":"2026-10-31T10:00:09Z","message":{"role":"assistant","content":[{"type":"text","text":"one more"}]},"version":"2.1.308"}"#;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("short.jsonl"))
        .unwrap();
    use std::io::Write;
    writeln!(f, "{line}").unwrap();
    drop(f);
    let hits = log.search(&s, "one more", 5).unwrap();
    assert_eq!(hits.len(), 1, "the rebuilt chain sees the appended turn");
    let again = log.page(&s, first.next.as_ref(), Direction::Older, 1);
    assert!(again.is_ok(), "growth does not invalidate a cursor");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn shared_caps_count_hits_across_sessions_and_stop_at_the_budget() {
    let root = tmp();
    projects(&root);
    let log = ClaudeLog::new(root.clone());
    let mut run = SearchRun::new(
        scan::Limits {
            max_hits: 3,
            per_session: 2,
            ..scan::Limits::default()
        },
        scan::Cancel::default(),
    );
    let a = log.search_in(&session("short"), "the", &mut run).unwrap();
    assert!(a.len() <= 2, "per-session cap");
    let b = log.search_in(&session("rewind"), "the", &mut run).unwrap();
    assert!(a.len() + b.len() <= 3);
    assert_eq!(run.budget.hits(), a.len() + b.len());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_rewind_after_a_cursor_was_taken_makes_it_stale() {
    let root = tmp();
    let dir = projects(&root);
    let log = ClaudeLog::new(root.clone());
    let s = session("short");
    let cursor = log
        .page(&s, None, Direction::Older, 1)
        .unwrap()
        .next
        .unwrap();
    // The user rewinds to a2 and asks something else: u9 hangs under a2, so a4 and a5 leave the branch.
    let line = r#"{"parentUuid":"a2","type":"user","uuid":"u9","timestamp":"2026-10-31T10:00:09Z","message":{"role":"user","content":"a different turn"},"version":"2.1.308"}"#;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("short.jsonl"))
        .unwrap();
    use std::io::Write;
    writeln!(f, "{line}").unwrap();
    drop(f);
    assert_eq!(
        log.page(&s, Some(&cursor), Direction::Older, 1),
        Err(LogError::StaleCursor)
    );
    let fresh = log.page(&s, None, Direction::Older, 1).unwrap();
    assert_eq!(fresh.entries[0].turn, "u9");
    assert!(log
        .page(&s, fresh.next.as_ref(), Direction::Older, 1)
        .is_ok());
    let forged = Cursor { pos: 3, ..cursor };
    assert_eq!(
        log.page(&s, Some(&forged), Direction::Older, 1),
        Err(LogError::StaleCursor)
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_long_directory_name_is_found_by_its_head() {
    let root = tmp();
    projects(&root);
    let long = format!("/home/{}", "deep/".repeat(60));
    let encoded: String = long
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let shortened = format!("{}-h4sh9x", &encoded[..200]);
    let dir = root.join(&shortened);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        Path::new(FIXTURES).join("short.jsonl"),
        dir.join("short.jsonl"),
    )
    .unwrap();
    let log = ClaudeLog::new(root.clone());
    let at = |id: &str| SessionRef {
        dir: PathBuf::from(&long),
        ..session(id)
    };
    assert!(!log.search(&at("short"), "by size", 5).unwrap().is_empty());
    assert_eq!(
        log.search(&at("other"), "by size", 5),
        Err(LogError::Missing),
        "only this id"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_needle_in_a_big_tool_result_costs_no_parse() {
    use super::scan::tests::allocs;
    use std::io::Write;
    let root = tmp();
    let dir = projects(&root);
    let log = ClaudeLog::new(root.clone());
    // Two copies of a session that differ in one tool result: the needle is in one and not in the other.
    for (id, filler) in [
        ("withneedle", "zebra quartz "),
        ("without", "filler words "),
    ] {
        std::fs::copy(dir.join("short.jsonl"), dir.join(format!("{id}.jsonl"))).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join(format!("{id}.jsonl")))
            .unwrap();
        let out = filler.repeat(40_000);
        writeln!(f, r#"{{"parentUuid":"a5","type":"user","uuid":"u99","timestamp":"2026-10-31T10:00:10Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_9","content":"{out}"}}]}},"toolUseResult":{{"stdout":"{out}"}}}}"#).unwrap();
    }
    let count = |id: &str| {
        let s = session(id);
        log.search(&s, "zebra quartz", 5).unwrap(); // the first run warms what a run reuses
        let before = allocs();
        let hits = log.search(&s, "zebra quartz", 5).unwrap();
        (allocs() - before, hits)
    };
    let (with, hits) = count("withneedle");
    let (without, _) = count("without");
    assert!(hits.is_empty(), "tool output is never a hit");
    assert_eq!(with, without, "the tool result was parsed");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn only_a_line_with_text_in_it_is_worth_parsing() {
    assert!(may_hold_text(
        br#"{"type":"user","message":{"role":"user","content":"hello"}}"#
    ));
    assert!(may_hold_text(
        br#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#
    ));
    assert!(may_hold_text(
        br#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"hm"}]}}"#
    ));
    assert!(may_hold_text(
        br#"{"type":"user","message":{"content":[{"type":"tool_result","content":"x"},{"type":"text","text":"and a note"}]}}"#
    ));
    assert!(!may_hold_text(
        br#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"out"}]}}"#
    ));
    assert!(!may_hold_text(
        br#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}"#
    ));
    // Tool output that quotes the markers carries them escaped, so they do not count.
    assert!(!may_hold_text(
        br#"{"type":"user","message":{"content":[{"type":"tool_result","content":"{\"type\":\"text\",\"text\":\"x\"}"}]}}"#
    ));
}

#[test]
fn a_file_named_like_a_flag_is_never_listed() {
    let root = tmp();
    let dir = projects(&root);
    std::fs::copy(
        dir.join("short.jsonl"),
        dir.join("--dangerously-skip-permissions.jsonl"),
    )
    .unwrap();
    let log = ClaudeLog::new(root.clone());
    let listing = log.list(None);
    assert!(!listing.files.is_empty());
    assert!(listing.files.iter().all(|f| !f.id.starts_with('-')));
    std::fs::remove_dir_all(root).ok();
}
