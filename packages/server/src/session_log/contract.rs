//! The contract every `SessionLog` adapter must pass (decision 0157), run over that adapter's recorded fixtures
//! (`testdata/session_log/<agent>/`, see its README for the cases). An adapter's test builds `Cases` and calls `run`.
//! Each check panics with the rule it enforces, so a failing adapter names what it broke.
use std::path::{Path, PathBuf};

use super::*;

/// A fixture session and a phrase that is in it.
pub struct Case {
    pub session: SessionRef,
    pub phrase: &'static str,
}

pub struct Cases {
    /// A short conversation. `file` must be a copy the suite may rewrite (the stale-cursor check replaces it).
    pub short: Case,
    pub file: PathBuf,
    /// A rewind or fork: `abandoned` is only on the dropped branch, `active` only on the kept one.
    pub branch: Option<(SessionRef, &'static str, &'static str)>,
    /// The last line is cut: `phrase` is in a whole line, `partial` only in the cut one.
    pub torn: (Case, &'static str),
    pub truncated: SessionRef,
    pub unknown_version: SessionRef,
    /// A tool call whose input and output hold `leaks` (a secret, the command): none may appear anywhere.
    pub tool: (Case, Vec<&'static str>),
    /// A record the adapter cannot map: the window around `turn` must show a `notice` for it.
    pub unmappable: Option<(SessionRef, &'static str)>,
    /// Text that scrubbing hides: `hidden` must not be found or shown, `visible` is in the same session.
    pub secret_text: (Case, &'static str),
}

const ALL: usize = 10_000;

fn all(log: &dyn SessionLog, s: &SessionRef) -> Vec<Entry> {
    log.page(s, None, Direction::Older, ALL)
        .expect("page the whole session")
        .entries
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

pub fn run(log: &dyn SessionLog, c: &Cases) {
    short_conversation(log, &c.short);
    paging(log, &c.short.session);
    if let Some((s, abandoned, active)) = &c.branch {
        assert!(
            log.search(s, abandoned, 10).unwrap().is_empty(),
            "only the active branch is searched"
        );
        assert!(
            !log.search(s, active, 10).unwrap().is_empty(),
            "the active branch is searched"
        );
        assert!(
            all(log, s).iter().all(|e| !e.text.contains(abandoned)),
            "an abandoned branch is not shown"
        );
    }
    torn_line(log, &c.torn.0, c.torn.1);
    for s in [&c.truncated, &c.unknown_version] {
        let off = |r: Result<(), LogError>| matches!(r, Err(LogError::Disabled(_)));
        assert!(
            off(log.search(s, "a", 5).map(drop)),
            "a truncated or unknown-version file turns search off, without a panic"
        );
        assert!(
            off(log.window(s, "t0", 1, 1).map(drop)),
            "window is off too"
        );
        assert!(
            off(log.page(s, None, Direction::Older, 5).map(drop)),
            "page is off too"
        );
    }
    tool_records(log, &c.tool.0, &c.tool.1);
    secret_text(log, &c.secret_text.0, c.secret_text.1);
    if let Some((s, turn)) = &c.unmappable {
        let w = log
            .window(s, turn, 1, 1)
            .expect("window around an unmappable record");
        assert!(
            w.entries
                .iter()
                .any(|e| e.turn == *turn && e.kind == Kind::Notice),
            "an unmappable record becomes a notice, not an error and not nothing"
        );
    }
    // Not the secret session: the scrubber is not idempotent (`[secret]` is itself a keyword), so a second pass
    // would hide more.
    every_entry_is_scrubbed(log, &[&c.short.session, &c.tool.0.session]);
    stale_cursor(log, &c.short.session, &c.file);
}

fn short_conversation(log: &dyn SessionLog, c: &Case) {
    let hits = log.search(&c.session, c.phrase, 10).unwrap();
    assert!(!hits.is_empty(), "a phrase in the conversation is found");
    for h in &hits {
        assert!(
            lower(&h.snippet).contains(&lower(c.phrase)),
            "a snippet holds the match"
        );
        let (a, b) = h.highlight;
        let marked: String = h.snippet.chars().skip(a).take(b - a).collect();
        assert_eq!(
            lower(&marked),
            lower(c.phrase),
            "the highlight covers exactly the match"
        );
    }
    assert_eq!(
        log.search(&c.session, &c.phrase.to_uppercase(), 10)
            .unwrap()
            .len(),
        hits.len(),
        "search ignores case"
    );
    assert!(
        log.search(&c.session, c.phrase, 1).unwrap().len() <= 1,
        "max_hits is honoured"
    );
    assert!(
        log.search(&c.session, "", 10).unwrap().is_empty(),
        "an empty query matches nothing"
    );
    let w = log.window(&c.session, &hits[0].turn, 1, 1).unwrap();
    assert!(
        w.entries.iter().any(|e| e.turn == hits[0].turn),
        "a window holds its anchor turn"
    );
    assert!(
        w.entries.len() <= 3,
        "a window is the anchor and the asked entries around it"
    );
    assert_eq!(w.anchor, hits[0].turn);
    assert_eq!(
        log.window(&c.session, "no-such-turn", 1, 1),
        Err(LogError::NoAnchor)
    );
}

/// Walking the pages from the end back to the start gives the whole session, once, in order.
fn paging(log: &dyn SessionLog, s: &SessionRef) {
    let whole = all(log, s);
    assert!(
        whole.len() >= 2,
        "the short fixture has at least two entries"
    );
    let mut got: Vec<Entry> = Vec::new();
    let mut cursor: Option<Cursor> = None;
    for _ in 0..whole.len() + 1 {
        let p = log.page(s, cursor.as_ref(), Direction::Older, 1).unwrap();
        got.splice(0..0, p.entries);
        match p.next {
            Some(c) => {
                cursor =
                    Some(Cursor::from_token(&c.to_token()).expect("a cursor survives its token"))
            }
            None => break,
        }
    }
    assert_eq!(got, whole, "older pages add up to the session");
    let mut fwd: Vec<Entry> = Vec::new();
    let mut cursor: Option<Cursor> = None;
    for _ in 0..whole.len() + 1 {
        let p = log.page(s, cursor.as_ref(), Direction::Newer, 1).unwrap();
        fwd.extend(p.entries);
        match p.next {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    assert_eq!(fwd, whole, "newer pages add up to the session");
}

fn torn_line(log: &dyn SessionLog, c: &Case, partial: &str) {
    assert!(!log
        .search(&c.session, c.phrase, 10)
        .expect("a torn last line is not an error")
        .is_empty());
    assert!(
        log.search(&c.session, partial, 10).unwrap().is_empty(),
        "a torn last line is dropped, not half shown"
    );
}

fn tool_records(log: &dyn SessionLog, c: &Case, leaks: &[&str]) {
    assert!(!log.search(&c.session, c.phrase, 10).unwrap().is_empty());
    let entries = all(log, &c.session);
    let mut shown = serde_json::to_string(&entries).unwrap();
    for e in &entries {
        shown.push_str(
            &serde_json::to_string(&log.window(&c.session, &e.turn, 2, 2).unwrap().entries)
                .unwrap(),
        );
    }
    for leak in leaks {
        assert!(
            !shown.contains(leak),
            "tool input and output, and secrets, never leave the adapter"
        );
        assert!(
            log.search(&c.session, leak, 10).unwrap().is_empty(),
            "tool input and output are not searched"
        );
    }
}

fn secret_text(log: &dyn SessionLog, c: &Case, hidden: &str) {
    assert!(
        log.search(&c.session, hidden, 10).unwrap().is_empty(),
        "text the scrubber hides cannot be found: a query would confirm a guess at a secret"
    );
    let hits = log.search(&c.session, c.phrase, 10).unwrap();
    assert!(!hits.is_empty());
    assert!(
        hits.iter().all(|h| !h.snippet.contains(hidden)),
        "a snippet is scrubbed"
    );
}

fn every_entry_is_scrubbed(log: &dyn SessionLog, sessions: &[&SessionRef]) {
    for s in sessions {
        for e in all(log, s) {
            assert_eq!(
                sanitize_block(&e.text, MAX_TEXT_CHARS, MAX_TEXT_LINES),
                e.text,
                "entry text leaves the adapter scrubbed"
            );
        }
    }
}

/// A rewritten file (a new inode) invalidates a cursor taken before; growth alone does not.
fn stale_cursor(log: &dyn SessionLog, s: &SessionRef, file: &Path) {
    let cursor = log
        .page(s, None, Direction::Older, 1)
        .unwrap()
        .next
        .expect("a cursor into a longer session");
    assert!(
        log.page(s, Some(&cursor), Direction::Older, 1).is_ok(),
        "an untouched file keeps its cursors"
    );
    let old = std::fs::read(file).unwrap();
    let tmp = file.with_extension("rewrite");
    std::fs::write(&tmp, &old).unwrap();
    std::fs::rename(&tmp, file).unwrap();
    assert_eq!(
        log.page(s, Some(&cursor), Direction::Older, 1),
        Err(LogError::StaleCursor),
        "a changed fingerprint invalidates a cursor"
    );
}
