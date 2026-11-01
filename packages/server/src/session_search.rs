//! Cross-session search (decisions 0156 and 0167): runs the `SessionLog` adapters over every session file in the
//! agents' own session roots, and answers the `window` request. Blocking; the connection runs it on a blocking thread.
//!
//! The corpus is what the adapters list, not what the registry knows: a closed tab, a replaced session and a session
//! run in a plain terminal are found too. The registry's sessions only say which hit a running tab holds (`open`) and
//! which workspace holds it; every other hit is `resume`.
//!
//! The query and the snippets are the user's conversations. Nothing here logs, formats into an error or keeps one:
//! the only outputs are the `emit` callback and the returned summary, which holds fixed strings.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use crate::registry::KnownSession;
use crate::session_log::scan::{Cancel, Cut, Limits};
use crate::session_log::{Hit, LogError, SearchRun, SessionLogs, SessionRef, Window, MAX_SIDE};

/// How a search ended: the cap that cut it, if any, and the agents whose search is off.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub cut: Option<&'static str>,
    /// Sessions the `since` bound kept from being read in full.
    pub older: usize,
    pub off: Vec<(String, &'static str)>,
}

pub fn cut_name(cut: Cut) -> &'static str {
    match cut {
        Cut::Hits => "hits",
        Cut::Bytes => "bytes",
        Cut::Time => "time",
        Cut::Cancelled => "cancelled",
    }
}

/// The cut reason when the time bound is the only thing that kept the search from looking everywhere.
pub const RANGE: &str = "range";

/// A conversation a hit belongs to. `workspace` and `tab` name the tab that holds it, and are empty when no tab does.
#[derive(Clone, PartialEq, Eq)]
pub struct Found {
    pub agent: String,
    pub session_id: String,
    pub dir: PathBuf,
    pub workspace: String,
    pub tab: String,
    /// A running tab holds the session; otherwise the hit offers Resume.
    pub open: bool,
}

impl std::fmt::Debug for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Found({})", self.agent) // the id and the directory are not logged
    }
}

fn session_ref(s: &KnownSession) -> SessionRef {
    SessionRef {
        agent: s.agent.clone(),
        id: s.session_id.clone(),
        dir: s.dir.clone(),
    }
}

/// One entry per conversation: the same session can sit in two tabs after a resume. Open workspaces come first, so
/// the hit the user can switch to is the one reported.
pub fn distinct(mut sessions: Vec<KnownSession>) -> Vec<KnownSession> {
    sessions.sort_by_key(|s| !s.up);
    let mut seen = HashSet::new();
    sessions.retain(|s| seen.insert((s.agent.clone(), s.session_id.clone())));
    sessions
}

/// Searches every session file the adapters list for `query` under `limits`, handing each hit to `emit` as it is
/// found. `known` marks the sessions a tab holds. `emit` returns false when nobody listens any more, which ends the
/// search like a cancel.
pub fn run(
    logs: &SessionLogs,
    known: Vec<KnownSession>,
    query: &str,
    limits: Limits,
    cancel: Cancel,
    mut emit: impl FnMut(&Found, Hit) -> bool,
) -> Summary {
    let mut run = SearchRun::new(limits, cancel.clone());
    let mut summary = Summary::default();
    let since = run
        .budget
        .limits()
        .since_ms
        .map(|ms| UNIX_EPOCH + Duration::from_millis(ms));
    let (listing, off) = logs.list(since);
    for (agent, why) in off {
        note_off(&mut summary, &agent, why);
    }
    let held: HashMap<(String, String), KnownSession> = distinct(known)
        .into_iter()
        .map(|k| ((k.agent.clone(), k.session_id.clone()), k))
        .collect();
    // An agent a tab runs that has no adapter is reported off, as before.
    for (agent, _) in held.keys() {
        if let Err(LogError::Disabled(why)) = logs.get(agent) {
            note_off(&mut summary, agent, why);
        }
    }
    let mut dropped = false;
    for f in listing.files {
        if run.budget.check().is_err() {
            break;
        }
        let adapter = match logs.get(&f.agent) {
            Ok(a) => a,
            Err(LogError::Disabled(why)) => {
                note_off(&mut summary, &f.agent, why);
                continue;
            }
            Err(_) => continue,
        };
        let found = match held.get(&(f.agent.clone(), f.id.clone())) {
            Some(k) => Found {
                agent: f.agent.clone(),
                session_id: f.id.clone(),
                dir: f.dir.clone(),
                workspace: k.workspace.clone(),
                tab: k.tab.clone(),
                open: k.up,
            },
            None => Found {
                agent: f.agent.clone(),
                session_id: f.id.clone(),
                dir: f.dir.clone(),
                workspace: String::new(),
                tab: String::new(),
                open: false,
            },
        };
        let session = SessionRef {
            agent: f.agent,
            id: f.id,
            dir: f.dir,
        };
        match adapter.search_in(&session, query, &mut run) {
            Ok(hits) => {
                for hit in hits {
                    if !emit(&found, hit) {
                        dropped = true;
                        break;
                    }
                }
            }
            Err(e) => {
                logs.note(&found.agent, &e);
                if let LogError::Disabled(why) = e {
                    note_off(&mut summary, &found.agent, why);
                }
            }
        }
        if dropped {
            cancel.cancel();
            let _ = run.budget.check();
            break;
        }
    }
    summary.older = run.budget.older() + listing.older;
    summary.cut = run
        .budget
        .cut()
        .map(cut_name)
        .or((summary.older > 0).then_some(RANGE));
    summary
}

fn note_off(summary: &mut Summary, agent: &str, why: &'static str) {
    if !summary.off.iter().any(|(a, _)| a == agent) {
        summary.off.push((agent.to_string(), why));
    }
}

/// The directory the session file of (`agent`, `id`) says it ran in, if the agent's root holds such a file. The client
/// names a conversation, never a path, so only a file the adapter lists can be resumed.
pub fn locate(logs: &SessionLogs, agent: &str, id: &str) -> Option<PathBuf> {
    logs.list(None)
        .0
        .files
        .into_iter()
        .find(|f| f.agent == agent && f.id == id)
        .map(|f| f.dir)
}

/// The turns around `turn` of the session of `workspace`'s `tab`. Only a session the server knows can be read, so a
/// client cannot name a file.
pub fn window(
    logs: &SessionLogs,
    sessions: &[KnownSession],
    workspace: &str,
    tab: &str,
    turn: &str,
    before: usize,
    after: usize,
) -> Result<Window, LogError> {
    let s = sessions
        .iter()
        .find(|s| s.workspace == workspace && s.tab == tab)
        .ok_or(LogError::Missing)?;
    let result = logs.get(&s.agent)?.window(
        &session_ref(s),
        turn,
        before.min(MAX_SIDE),
        after.min(MAX_SIDE),
    );
    if let Err(e) = &result {
        logs.note(&s.agent, e);
    }
    result
}

/// The Claude fixtures as the adapter finds them, for tests here and in the connection handler.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::PathBuf;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/session_log/claude");
    pub const DIR: &str = "/home/fixture/project";

    pub fn projects() -> PathBuf {
        let root = std::env::temp_dir().join(format!("tm-search-{}", uuid::Uuid::new_v4()));
        let dir = root.join("-home-fixture-project");
        std::fs::create_dir_all(&dir).unwrap();
        for f in std::fs::read_dir(FIXTURES).unwrap() {
            let f = f.unwrap().path();
            if f.extension().is_some_and(|e| e == "jsonl") {
                std::fs::copy(&f, dir.join(f.file_name().unwrap())).unwrap();
            }
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::fixtures::{projects, DIR};
    use super::*;
    use crate::session_log::claude::ClaudeLog;
    use crate::session_log::{Cursor, WINDOW_BYTES};

    fn logs(root: &Path) -> SessionLogs {
        let mut logs = SessionLogs::default();
        logs.register(Arc::new(ClaudeLog::new(root.to_path_buf())));
        logs
    }

    fn known(id: &str, workspace: &str, tab: &str, up: bool) -> KnownSession {
        KnownSession {
            agent: "claude".into(),
            session_id: id.into(),
            dir: PathBuf::from(DIR),
            workspace: workspace.into(),
            tab: tab.into(),
            up,
        }
    }

    #[test]
    fn a_search_finds_scrubbed_hits_in_open_and_dormant_sessions() {
        let root = projects();
        let sessions = vec![
            known("short", "ws-down", "t1", false),
            known("rewind", "ws-up", "t1", true),
        ];
        let mut got = Vec::new();
        let summary = run(
            &logs(&root),
            sessions,
            "newest files",
            Limits::default(),
            Cancel::default(),
            |s, h| {
                got.push((s.workspace.clone(), h));
                true
            },
        );
        assert_eq!(summary.cut, None);
        assert!(got.iter().any(|(w, _)| w == "ws-up"), "{got:?}");
        assert!(got.iter().all(|(_, h)| !h.snippet.contains("sk-FAKE")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_hit_cap_cuts_the_search_and_says_so() {
        let root = projects();
        let limits = Limits {
            max_hits: 1,
            ..Limits::default()
        };
        let mut n = 0;
        let summary = run(
            &logs(&root),
            vec![
                known("short", "a", "t1", true),
                known("rewind", "b", "t1", true),
            ],
            "the",
            limits,
            Cancel::default(),
            |_, _| {
                n += 1;
                true
            },
        );
        assert_eq!(n, 1);
        assert_eq!(summary.cut, Some("hits"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_cancelled_search_says_so_and_one_session_is_one_entry() {
        let root = projects();
        let cancel = Cancel::default();
        cancel.cancel();
        let summary = run(
            &logs(&root),
            vec![known("short", "a", "t1", true)],
            "the",
            Limits::default(),
            cancel,
            |_, _| true,
        );
        assert_eq!(summary.cut, Some("cancelled"));
        let twice = distinct(vec![
            known("short", "dormant", "t1", false),
            known("short", "open", "t1", true),
        ]);
        assert_eq!(twice.len(), 1);
        assert_eq!(twice[0].workspace, "open");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_agent_with_no_adapter_is_reported_off_and_the_others_still_search() {
        let root = projects();
        let mut other = known("x", "w", "t1", true);
        other.agent = "opencode".into();
        let mut n = 0;
        let summary = run(
            &logs(&root),
            vec![other, known("short", "a", "t1", true)],
            "by size",
            Limits::default(),
            Cancel::default(),
            |_, _| {
                n += 1;
                true
            },
        );
        assert!(n > 0);
        assert_eq!(summary.off.len(), 1);
        assert_eq!(summary.off[0].0, "opencode");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_session_last_written_before_the_bound_is_not_opened() {
        let root = projects();
        // Every session file of the root was last written long ago.
        for f in std::fs::read_dir(root.join("-home-fixture-project")).unwrap() {
            let file = std::fs::File::options()
                .write(true)
                .open(f.unwrap().path())
                .unwrap();
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000))
                .unwrap();
        }
        let limits = Limits {
            since_ms: Some(2_000_000 * 1000),
            ..Limits::default()
        };
        let mut n = 0;
        let summary = run(
            &logs(&root),
            vec![known("short", "ws", "t1", true)],
            "the",
            limits,
            Cancel::default(),
            |_, _| {
                n += 1;
                true
            },
        );
        assert_eq!(n, 0);
        assert_eq!(summary.cut, Some("range"));
        assert!(summary.older >= 1);
        // With no bound the same session is searched.
        let summary = run(
            &logs(&root),
            vec![known("short", "ws", "t1", true)],
            "the",
            Limits::default(),
            Cancel::default(),
            |_, _| true,
        );
        assert_eq!((summary.older, summary.cut), (0, None));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_window_around_a_hit_fits_the_cap_and_only_known_sessions_are_read() {
        let root = projects();
        let logs = logs(&root);
        let sessions = vec![known("short", "ws", "t1", true)];
        let mut hit = None;
        run(
            &logs,
            sessions.clone(),
            "by size",
            Limits::default(),
            Cancel::default(),
            |_, h| {
                hit = Some(h);
                true
            },
        );
        let hit = hit.expect("a hit");
        let w = window(&logs, &sessions, "ws", "t1", &hit.turn, 5, 5).unwrap();
        assert!(w.entries.iter().any(|e| e.turn == hit.turn));
        let bytes: usize = w.entries.iter().map(|e| e.text.len()).sum();
        assert!(bytes <= WINDOW_BYTES);
        assert!(Cursor {
            fingerprint: w.fingerprint,
            pos: 0
        }
        .to_token()
        .contains('.'));
        // A tab the server has no session for is not read.
        assert_eq!(
            window(&logs, &sessions, "ws", "other", &hit.turn, 1, 1).unwrap_err(),
            LogError::Missing
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn hits_of(root: &Path, known: Vec<KnownSession>, query: &str) -> Vec<(Found, Hit)> {
        let mut got = Vec::new();
        run(
            &logs(root),
            known,
            query,
            Limits::default(),
            Cancel::default(),
            |f, h| {
                got.push((f.clone(), h));
                true
            },
        );
        got
    }

    #[test]
    fn sessions_no_tab_holds_are_found_and_a_held_one_names_its_tab() {
        let root = projects();
        // The registry knows nothing (a closed tab, a closed workspace, a session started in a plain terminal).
        let got = hits_of(&root, Vec::new(), "by size");
        assert!(!got.is_empty());
        assert!(got.iter().all(|(f, _)| !f.open && f.workspace.is_empty()));
        assert!(got.iter().all(|(f, _)| f.dir == PathBuf::from(DIR)));
        // A replaced session is no longer the tab's, yet its file is still searched: only `short` is held.
        let got = hits_of(&root, vec![known("short", "ws-up", "t1", true)], "by size");
        let held: Vec<_> = got
            .iter()
            .filter(|(f, _)| f.session_id == "short")
            .collect();
        assert!(!held.is_empty() && held.iter().all(|(f, _)| f.open && f.workspace == "ws-up"));
        assert!(got.iter().any(|(f, _)| f.session_id == "rewind" && !f.open));
        // A dormant workspace holds a session without running it: named, but `resume`.
        let got = hits_of(
            &root,
            vec![known("short", "ws-down", "t1", false)],
            "by size",
        );
        assert!(got
            .iter()
            .any(|(f, _)| f.session_id == "short" && f.workspace == "ws-down" && !f.open));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_symlink_out_of_the_root_is_never_read_and_an_unknown_format_is_skipped() {
        let root = projects();
        let outside = std::env::temp_dir().join(format!("tm-outside-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&outside).unwrap();
        let line = |t: &str| {
            format!(
                "{{\"type\":\"user\",\"uuid\":\"o1\",\"parentUuid\":null,\"cwd\":\"/home/fixture/project\",\"version\":\"2.1.0\",\"message\":{{\"role\":\"user\",\"content\":\"{t}\"}}}}\n"
            )
        };
        std::fs::write(outside.join("out.jsonl"), line("zebrafinch outside")).unwrap();
        let project = root.join("-home-fixture-project");
        std::os::unix::fs::symlink(outside.join("out.jsonl"), project.join("link.jsonl")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("-linked-folder")).unwrap();
        std::fs::write(
            project.join("junk.jsonl"),
            "not json at all\nzebrafinch junk\n",
        )
        .unwrap();
        assert!(hits_of(&root, Vec::new(), "zebrafinch").is_empty());
        // The same file, really inside the root, is found: the link was the only reason it was not.
        std::fs::copy(outside.join("out.jsonl"), project.join("real.jsonl")).unwrap();
        let got = hits_of(&root, Vec::new(), "zebrafinch");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.session_id, "real");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn a_found_session_prints_the_agent_only() {
        let f = known("secret-id", "w", "t", true);
        let found = Found {
            agent: f.agent,
            session_id: f.session_id,
            dir: PathBuf::from("/private/dir"),
            workspace: f.workspace,
            tab: f.tab,
            open: true,
        };
        let text = format!("{found:?}");
        assert!(
            !text.contains("secret-id") && !text.contains("private"),
            "{text}"
        );
    }
}
