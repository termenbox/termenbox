use std::path::{Path, PathBuf};

use super::contract::{self, Case, Cases};
use super::stub::Stub;
use super::*;

const SECRET: &str = "sk-FAKE-0123456789abcdef";

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termenbox-session-log-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn session(id: &str) -> SessionRef {
    SessionRef {
        agent: "stub".into(),
        id: id.into(),
        dir: PathBuf::from("/work"),
    }
}

fn case(id: &str, phrase: &'static str) -> Case {
    Case {
        session: session(id),
        phrase,
    }
}

fn write(root: &Path, id: &str, body: &str) {
    std::fs::write(root.join(format!("{id}.jsonl")), body).unwrap();
}

fn user(turn: &str, text: &str) -> String {
    format!("{{\"turn\":\"{turn}\",\"kind\":\"user\",\"text\":\"{text}\"}}\n")
}

fn message(turn: &str, text: &str) -> String {
    format!("{{\"turn\":\"{turn}\",\"kind\":\"message\",\"text\":\"{text}\"}}\n")
}

/// The six fixture cases of `testdata/session_log/README.md`, plus a record to map and a scrubbed one, in the
/// stub's format.
fn fixtures(root: &Path) -> Cases {
    let head = "{\"stub\":1}\n";
    let short = format!(
        "{head}{}{}{}{}",
        user("t1", "How do I list files by size?"),
        message("t2", "Use ls -lS to sort by size."),
        user("t3", "And only the five biggest?"),
        message("t4", "Pipe it to head.")
    );
    write(root, "short", &short);
    write(
        root,
        "branch",
        &format!(
            "{head}{}{}{{\"turn\":\"t3\",\"kind\":\"user\",\"text\":\"zebra dropped branch\",\"abandoned\":true}}\n{}",
            user("t1", "Question one"),
            message("t2", "Answer one"),
            message("t4", "quokka kept branch")
        ),
    );
    let torn_line = user("t9", "cut off while written");
    write(
        root,
        "torn",
        &format!(
            "{head}{}{}{}",
            user("t1", "whole line here"),
            message("t2", "another whole line"),
            &torn_line[..torn_line.len() - 20]
        ),
    );
    write(
        root,
        "truncated",
        &format!("\"stub\":1}}\n{}", user("t1", "orphan")),
    );
    write(root, "unknown", "{\"stub\":9}\n");
    write(
        root,
        "tool",
        &format!(
            "{head}{}{{\"turn\":\"t2\",\"kind\":\"tool\",\"text\":\"Bash\",\"input\":\"echo LEAK-INPUT-5521\",\"output\":\"RESULT-LINE-7731 {SECRET}\"}}\n{}",
            user("t1", "Print the deploy listing"),
            message("t3", "Done.")
        ),
    );
    write(
        root,
        "weird",
        &format!(
            "{head}{}{{\"turn\":\"w1\",\"kind\":\"hologram\"}}\n{}",
            user("t1", "before"),
            message("t3", "after")
        ),
    );
    write(
        root,
        "secret",
        &format!(
            "{head}{}{}",
            user("t1", "the password hunter2xyz is set"),
            message("t2", "visible reply")
        ),
    );
    Cases {
        short: case("short", "by size"),
        file: root.join("short.jsonl"),
        branch: Some((session("branch"), "zebra", "quokka")),
        torn: (case("torn", "whole line"), "written"),
        truncated: session("truncated"),
        unknown_version: session("unknown"),
        tool: (
            case("tool", "deploy listing"),
            vec!["RESULT-LINE-7731", "LEAK-INPUT-5521", SECRET],
        ),
        unmappable: Some((session("weird"), "w1")),
        secret_text: (case("secret", "visible reply"), "hunter2xyz"),
    }
}

#[test]
fn a_correct_adapter_passes_the_contract_suite() {
    let root = tmp();
    let cases = fixtures(&root);
    let stub = Stub {
        name: "stub",
        root: root.clone(),
        copy_tool_output: false,
    };
    contract::run(&stub, &cases);
    std::fs::remove_dir_all(root).ok();
}

#[test]
#[should_panic(expected = "never leave the adapter")]
fn the_suite_fails_an_adapter_that_copies_tool_output() {
    let root = tmp();
    let cases = fixtures(&root);
    let stub = Stub {
        name: "stub",
        root: root.clone(),
        copy_tool_output: true,
    };
    contract::run(&stub, &cases);
}

#[test]
fn a_snippet_marks_the_match_and_shortens_long_text() {
    let long = format!("{}needle {}", "alpha ".repeat(60), "beta ".repeat(60));
    let (text, (a, b)) = snippet(&long, "NEEDLE").unwrap();
    assert_eq!(
        text.chars().skip(a).take(b - a).collect::<String>(),
        "needle"
    );
    assert!(text.starts_with('…') && text.ends_with('…'));
    assert!(text.chars().count() <= 2 * SNIPPET_CONTEXT + 6 + 2);
    assert_eq!(snippet("no match", "zzz"), None);
    assert_eq!(snippet("anything", ""), None);
}

#[test]
fn a_query_cannot_match_text_the_scrubber_hides() {
    assert_eq!(
        snippet("the password hunter2xyz is set", "hunter2xyz"),
        None
    );
    assert!(snippet("the password hunter2xyz is set", "the password").is_some());
}

#[test]
fn a_cursor_holds_while_the_file_only_grows() {
    let at = Fingerprint {
        inode: 7,
        size: 100,
        mtime_ns: 5,
    };
    assert!(at.still_holds(&at));
    assert!(
        at.still_holds(&Fingerprint {
            size: 150,
            mtime_ns: 9,
            ..at
        }),
        "appended"
    );
    assert!(!at.still_holds(&Fingerprint { inode: 8, ..at }), "replaced");
    assert!(!at.still_holds(&Fingerprint { size: 90, ..at }), "shrunk");
    assert!(
        !at.still_holds(&Fingerprint { mtime_ns: 6, ..at }),
        "rewritten in place"
    );
}

#[test]
fn a_cursor_token_round_trips_and_rejects_junk() {
    let c = Cursor {
        fingerprint: Fingerprint {
            inode: 0xabc,
            size: 4096,
            mtime_ns: 1_700_000_000_123_456_789,
        },
        pos: 12,
    };
    assert_eq!(Cursor::from_token(&c.to_token()), Some(c));
    for junk in ["", "1.2.3", "1.2.3.4.5", "x.2.3.4", "1.2.3.-4"] {
        assert_eq!(Cursor::from_token(junk), None, "{junk}");
    }
}

#[test]
fn one_agent_going_off_leaves_the_others_searchable() {
    let root = tmp();
    fixtures(&root);
    let mut logs = SessionLogs::default();
    logs.register(Arc::new(Stub {
        name: "a",
        root: root.clone(),
        copy_tool_output: false,
    }));
    logs.register(Arc::new(Stub {
        name: "b",
        root: root.clone(),
        copy_tool_output: false,
    }));
    let on = |agent: &str, id: &str| SessionRef {
        agent: agent.into(),
        ..session(id)
    };

    assert!(matches!(
        logs.search(&on("a", "unknown"), "x", 5),
        Err(LogError::Disabled("unknown version"))
    ));
    assert!(
        matches!(
            logs.get("a").map(|_| ()),
            Err(LogError::Disabled("unknown version"))
        ),
        "later calls skip it"
    );
    assert_eq!(
        logs.search(&on("b", "short"), "by size", 5).unwrap().len(),
        2
    );
    assert_eq!(
        logs.status(),
        vec![
            ("a".to_string(), Some("unknown version")),
            ("b".to_string(), None)
        ]
    );
    assert!(
        matches!(
            logs.search(&on("nope", "short"), "x", 5),
            Err(LogError::Disabled(_))
        ),
        "no adapter is off, not a panic"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_missing_file_is_not_a_reason_to_turn_the_agent_off() {
    let root = tmp();
    fixtures(&root);
    let mut logs = SessionLogs::default();
    logs.register(Arc::new(Stub {
        name: "stub",
        root: root.clone(),
        copy_tool_output: false,
    }));
    assert_eq!(
        logs.search(&session("gone"), "x", 5),
        Err(LogError::Missing)
    );
    assert!(logs.search(&session("short"), "by size", 5).is_ok());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn errors_and_debug_output_carry_no_id_query_or_path() {
    let s = SessionRef {
        agent: "claude".into(),
        id: "secret-session-id".into(),
        dir: PathBuf::from("/home/me/private"),
    };
    let shown = format!("{s:?}");
    assert!(
        !shown.contains("secret-session-id") && !shown.contains("private"),
        "{shown}"
    );
    for e in [
        LogError::Disabled("unknown version"),
        LogError::Missing,
        LogError::StaleCursor,
        LogError::NoAnchor,
    ] {
        assert!(!e.to_string().contains("secret-session-id"));
    }
}
