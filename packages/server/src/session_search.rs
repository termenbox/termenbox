//! Cross-session search (decision 0156): runs the `SessionLog` adapters over the sessions the server has a record of,
//! and answers the `window` request. Blocking; the connection runs it on a blocking thread.
//!
//! The query and the snippets are the user's conversations. Nothing here logs, formats into an error or keeps one:
//! the only outputs are the `emit` callback and the returned summary, which holds fixed strings.
use std::collections::HashSet;

use crate::registry::KnownSession;
use crate::session_log::scan::{Cancel, Cut, Limits};
use crate::session_log::{Hit, LogError, SearchRun, SessionLogs, SessionRef, Window, MAX_SIDE};

/// How a search ended: the cap that cut it, if any, and the agents whose search is off.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub cut: Option<&'static str>,
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

/// Searches `sessions` for `query` under `limits`, handing each hit to `emit` as it is found. `emit` returns false
/// when nobody listens any more, which ends the search like a cancel.
pub fn run(
    logs: &SessionLogs,
    sessions: Vec<KnownSession>,
    query: &str,
    limits: Limits,
    cancel: Cancel,
    mut emit: impl FnMut(&KnownSession, Hit) -> bool,
) -> Summary {
    let mut run = SearchRun::new(limits, cancel.clone());
    let mut summary = Summary::default();
    let mut dropped = false;
    for s in distinct(sessions) {
        if run.budget.check().is_err() {
            break;
        }
        let adapter = match logs.get(&s.agent) {
            Ok(a) => a,
            Err(LogError::Disabled(why)) => {
                note_off(&mut summary, &s.agent, why);
                continue;
            }
            Err(_) => continue,
        };
        match adapter.search_in(&session_ref(&s), query, &mut run) {
            Ok(hits) => {
                for hit in hits {
                    if !emit(&s, hit) {
                        dropped = true;
                        break;
                    }
                }
            }
            Err(e) => {
                logs.note(&s.agent, &e);
                if let LogError::Disabled(why) = e {
                    note_off(&mut summary, &s.agent, why);
                }
            }
        }
        if dropped {
            cancel.cancel();
            let _ = run.budget.check();
            break;
        }
    }
    summary.cut = run.budget.cut().map(cut_name);
    summary
}

fn note_off(summary: &mut Summary, agent: &str, why: &'static str) {
    if !summary.off.iter().any(|(a, _)| a == agent) {
        summary.off.push((agent.to_string(), why));
    }
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
}
