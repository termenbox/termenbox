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
use crate::session_log::scan::{Budget, Cancel, Cut, Limits};
use crate::session_log::{
    plain_id, Cursor, Hit, Listing, LogError, SearchRun, SessionFile, SessionLogs, SessionRef,
    Window, MAX_SIDE,
};

/// How a search ended: the cap that cut it, if any, and the agents whose search is off.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub cut: Option<&'static str>,
    /// Sessions the `since` bound kept from being read in full.
    pub older: usize,
    pub off: Vec<(String, &'static str)>,
    /// Where to continue when the search stopped before it had looked everywhere (opaque to the client).
    pub next: Option<String>,
    /// The cursor's file was rewritten since the last page, so it was read again from its newest line.
    pub restarted: bool,
}

/// Most hits of the one conversation a per-session search returns.
pub const SESSION_CAP: usize = 200;

/// Which part of the corpus a search covers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    /// Only this conversation, with all its matches up to `SESSION_CAP`.
    pub session: Option<String>,
    /// The agent of `session`; none searches every agent that has the id.
    pub agent: Option<String>,
    /// A cursor from an earlier `Summary::next`: continue after it.
    pub after: Option<String>,
}

/// Where a page stopped: the last conversation it reached, and how far into it. The listing is newest first, so
/// "after" means older; the server keeps nothing between pages.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pos {
    agent: String,
    id: String,
    mtime_ns: u128,
    /// How far into the file the scan got, or none when the conversation was finished.
    at: Option<Cursor>,
}

impl Pos {
    fn token(&self) -> String {
        let at = self.at.map_or("-".to_string(), Cursor::to_token);
        format!("{}:{}:{:x}:{}", self.agent, self.id, self.mtime_ns, at)
    }

    fn parse(token: &str) -> Option<Pos> {
        let mut p = token.splitn(4, ':');
        let (agent, id) = (p.next()?, p.next()?);
        let mtime_ns = u128::from_str_radix(p.next()?, 16).ok()?;
        let at = match p.next()? {
            "-" => None,
            t => Some(Cursor::from_token(t)?),
        };
        (plain_id(agent) && plain_id(id)).then(|| Pos {
            agent: agent.into(),
            id: id.into(),
            mtime_ns,
            at,
        })
    }

    fn of(f: &SessionFile, at: Option<Cursor>) -> Pos {
        Pos {
            agent: f.agent.clone(),
            id: f.id.clone(),
            mtime_ns: mtime_ns(f),
            at,
        }
    }
}

fn mtime_ns(f: &SessionFile) -> u128 {
    f.mtime
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// The listing order: newest first, ties by name so a page boundary is the same on every walk.
fn order(a: &SessionFile, b: &SessionFile) -> std::cmp::Ordering {
    mtime_ns(b)
        .cmp(&mtime_ns(a))
        .then_with(|| (&a.agent, &a.id).cmp(&(&b.agent, &b.id)))
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
    emit: impl FnMut(&Found, Hit) -> bool,
) -> Summary {
    run_page(logs, known, query, limits, cancel, &Page::default(), emit)
}

/// `run` for one page: a single conversation (`page.session`), or the corpus after the cursor (`page.after`).
pub fn run_page(
    logs: &SessionLogs,
    known: Vec<KnownSession>,
    query: &str,
    mut limits: Limits,
    cancel: Cancel,
    page: &Page,
    mut emit: impl FnMut(&Found, Hit) -> bool,
) -> Summary {
    if page.session.is_some() {
        // All of one conversation, whatever its age.
        limits.since_ms = None;
        limits.per_session = SESSION_CAP;
        limits.max_hits = SESSION_CAP;
    }
    let mut run = SearchRun::new(limits, cancel.clone());
    let mut summary = Summary::default();
    let since = run
        .budget
        .limits()
        .since_ms
        .map(|ms| UNIX_EPOCH + Duration::from_millis(ms));
    // The walk of the roots has its own half of the time and bytes, so a big corpus cannot spend all of them before a
    // file is searched; what it reads is charged to the search. One conversation is found by name, not by a walk.
    let (mut listing, off) = match &page.session {
        Some(id) => {
            let mut files = logs.find(id, &cancel);
            files.retain(|f| page.agent.as_ref().is_none_or(|a| *a == f.agent));
            (
                Listing {
                    files,
                    ..Listing::default()
                },
                Vec::new(),
            )
        }
        None => {
            let l = run.budget.limits();
            let walk = Limits {
                max_time: l.max_time / 2,
                max_bytes: l.max_bytes / 2,
                ..*l
            };
            let mut walker = Budget::new(walk, cancel.clone());
            let out = logs.list(since, &mut walker);
            run.budget.add_bytes(walker.bytes_read());
            out
        }
    };
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
    let mut files = std::mem::take(&mut listing.files);
    files.sort_by(order);
    // A cursor that does not parse starts from the top: it holds no more than a position.
    let after = page.after.as_deref().and_then(Pos::parse);
    if let Some(c) = &after {
        let this = |f: &SessionFile| f.agent == c.agent && f.id == c.id;
        let key = |f: &SessionFile| {
            (
                std::cmp::Reverse(mtime_ns(f)),
                f.agent.clone(),
                f.id.clone(),
            )
        };
        let at = (std::cmp::Reverse(c.mtime_ns), c.agent.clone(), c.id.clone());
        // The conversation the page stopped in goes first, wherever it sorts now; the rest are those that sort
        // after it, which a conversation that grew since is not any more.
        let mine = files.iter().position(this).filter(|_| c.at.is_some());
        let head = mine.map(|i| files.remove(i));
        files.retain(|f| !this(f) && key(f) > at);
        if let Some(h) = head {
            files.insert(0, h);
        }
    }
    // A search cancelled during the walk says so, though no file was reached.
    let _ = run.budget.check();
    let per_session = run.budget.limits().per_session;
    let max_hits = run.budget.limits().max_hits;
    let mut last: Option<Pos> = None;
    let mut dropped = false;
    for f in files {
        if run.budget.check().is_err() {
            break;
        }
        run.resume = after
            .as_ref()
            .filter(|c| c.agent == f.agent && c.id == f.id)
            .and_then(|c| c.at);
        run.stopped = None;
        run.seen = None;
        run.restarted = false;
        let pos = Pos::of(&f, None);
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
        let mut shown = 0;
        match adapter.search_file(&f, query, &mut run) {
            Ok(hits) => {
                shown = hits.len();
                for hit in hits {
                    if !emit(&found, hit) {
                        dropped = true;
                        break;
                    }
                }
            }
            // One file that cannot be read is left out; the agent stays on.
            Err(LogError::Unreadable | LogError::Missing) => {}
            Err(e) => {
                logs.note(&found.agent, &e);
                if let LogError::Disabled(why) = e {
                    note_off(&mut summary, &found.agent, why);
                }
            }
        }
        summary.restarted |= run.restarted;
        // The per-session cap ends a conversation for this page (the sheet asks for the rest of it on its own); any
        // other stop leaves the rest of the file for the next page.
        let capped = shown >= per_session && per_session < max_hits;
        let at = match (run.stopped, run.seen) {
            (Some(pos), Some(fp)) if !capped => Some(Cursor {
                fingerprint: fp,
                pos,
            }),
            _ => None,
        };
        last = Some(Pos { at, ..pos });
        if dropped {
            cancel.cancel();
            let _ = run.budget.check();
            break;
        }
    }
    summary.older = run.budget.older() + listing.older;
    let cut = run.budget.cut().or(listing.cut);
    summary.cut = cut.map(cut_name).or((summary.older > 0).then_some(RANGE));
    let stopped_early = matches!(cut, Some(Cut::Hits | Cut::Bytes | Cut::Time));
    let mid_file = last.as_ref().is_some_and(|p| p.at.is_some());
    if !dropped && cut != Some(Cut::Cancelled) && (stopped_early || mid_file) {
        summary.next = last.or(after).map(|p| p.token());
    }
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
    logs.find(id, &Cancel::default())
        .into_iter()
        .find(|f| f.agent == agent)
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

    /// A Claude session of `n` user records, each holding "quokka", chained so all are on the active branch.
    fn write_quokka(root: &Path, id: &str, from: usize, n: usize, append: bool) {
        use std::io::Write;
        let path = root
            .join("-home-fixture-project")
            .join(format!("{id}.jsonl"));
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path)
            .unwrap();
        for i in from..from + n {
            let parent = if i == 0 {
                "null".to_string()
            } else {
                format!("\"{id}-{}\"", i - 1)
            };
            writeln!(
                f,
                "{{\"parentUuid\":{parent},\"type\":\"user\",\"uuid\":\"{id}-{i}\",\"cwd\":\"{DIR}\",\"version\":\"2.1.0\",\"message\":{{\"role\":\"user\",\"content\":\"quokka number {i}\"}}}}"
            )
            .unwrap();
        }
    }

    fn page(root: &Path, limits: Limits, page: &Page, got: &mut Vec<(String, String)>) -> Summary {
        run_page(
            &logs(root),
            Vec::new(),
            "quokka",
            limits,
            Cancel::default(),
            page,
            |f, h| {
                got.push((f.session_id.clone(), h.turn));
                true
            },
        )
    }

    fn paged_limits() -> Limits {
        Limits {
            max_hits: 20,
            per_session: 100,
            ..Limits::default()
        }
    }

    /// `n` small sessions, each with one "quokka" record and its own mtime a second apart (newest is `many-0`).
    fn write_many(root: &Path, n: usize) {
        for i in 0..n {
            write_quokka(root, &format!("many-{i}"), 0, 1, false);
            let path = root
                .join("-home-fixture-project")
                .join(format!("many-{i}.jsonl"));
            let t = std::time::SystemTime::now() - Duration::from_secs(i as u64 + 1);
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
    }

    #[test]
    fn a_big_corpus_under_a_low_byte_cap_gives_hits_and_a_cursor_never_nothing() {
        let root = projects();
        write_many(&root, 1000);
        let limits = Limits {
            max_bytes: 64 * 1024,
            max_hits: 1000,
            per_session: 10,
            ..Limits::default()
        };
        // One registry for every page, as the server keeps one: the heads read for a page cost a `stat` on the next.
        let logs = logs(&root);
        let mut seen: HashSet<String> = HashSet::new();
        let mut after: Option<String> = None;
        for n in 0..100 {
            let mut got = Vec::new();
            let summary = run_page(
                &logs,
                Vec::new(),
                "quokka",
                limits,
                Cancel::default(),
                &Page {
                    after: after.take(),
                    ..Page::default()
                },
                |f, h| {
                    got.push((f.session_id.clone(), h.turn));
                    true
                },
            );
            assert!(!got.is_empty(), "page {n} found nothing");
            if n == 0 {
                assert_eq!(summary.cut, Some("bytes"));
            }
            seen.extend(got.into_iter().map(|(s, _)| s));
            after = summary.next;
            if after.is_none() {
                break;
            }
        }
        assert!(after.is_none(), "the pages did not end");
        assert_eq!(seen.len(), 1000);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn listing_stops_when_the_search_is_cancelled() {
        let root = projects();
        write_many(&root, 50);
        let cancel = Cancel::default();
        cancel.cancel();
        let mut budget = Budget::new(Limits::default(), cancel);
        let (listing, _) = logs(&root).list(None, &mut budget);
        assert!(listing.files.len() <= 1, "{} listed", listing.files.len());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_link_that_carries_a_real_sessions_id_is_never_read() {
        let root = projects();
        let outside = std::env::temp_dir().join(format!("tm-outside-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&outside).unwrap();
        write_quokka(&root, "real-id", 0, 1, false);
        let real = root.join("-home-fixture-project").join("real-id.jsonl");
        // A second project folder holds a link named for the same id, pointing at a file outside the root.
        let other = root.join("-home-other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::copy(&real, outside.join("x.jsonl")).unwrap();
        std::os::unix::fs::symlink(outside.join("x.jsonl"), other.join("real-id.jsonl")).unwrap();
        let mut got = Vec::new();
        page(&root, Limits::default(), &Page::default(), &mut got);
        assert_eq!(got.len(), 1);
        // Found by name (Resume) the link is not taken either.
        let found = logs(&root).find("real-id", &Cancel::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, real);
        // And a file swapped for a link after the walk is refused at the open.
        let listed = found[0].clone();
        std::fs::remove_file(&real).unwrap();
        std::os::unix::fs::symlink(outside.join("x.jsonl"), &real).unwrap();
        let mut run = SearchRun::single(10);
        let adapter = logs(&root).get("claude").unwrap();
        assert!(adapter.search_file(&listed, "quokka", &mut run).is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn one_unreadable_file_is_skipped_and_the_agent_stays_on() {
        use std::os::unix::fs::PermissionsExt;
        let root = projects();
        write_quokka(&root, "good", 0, 1, false);
        write_quokka(&root, "locked", 0, 1, false);
        let locked = root.join("-home-fixture-project").join("locked.jsonl");
        let logs = logs(&root);
        // Listed while readable, then locked: the search meets the error at the open.
        let listing = logs.list(None, &mut Budget::new(Limits::default(), Cancel::default()));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&locked).is_ok() {
            return; // running as root: the mode is not enforced
        }
        let mut run = SearchRun::single(10);
        let adapter = logs.get("claude").unwrap();
        let file = listing.0.files.iter().find(|f| f.id == "locked").unwrap();
        assert_eq!(
            adapter.search_file(file, "quokka", &mut run).unwrap_err(),
            LogError::Unreadable
        );
        let mut got = Vec::new();
        let summary = run_page(
            &logs,
            Vec::new(),
            "quokka",
            Limits::default(),
            Cancel::default(),
            &Page::default(),
            |f, h| {
                got.push((f.session_id.clone(), h.turn));
                true
            },
        );
        assert!(got.iter().any(|(s, _)| s == "good"));
        assert!(summary.off.is_empty());
        assert!(logs.get("claude").is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn paging_past_the_cap_sees_each_hit_exactly_once() {
        let root = projects();
        write_quokka(&root, "paged-a", 0, 70, false);
        write_quokka(&root, "paged-b", 0, 30, false);
        let mut whole = Vec::new();
        let all = Limits {
            max_hits: 1000,
            per_session: 1000,
            ..Limits::default()
        };
        assert_eq!(page(&root, all, &Page::default(), &mut whole).next, None);
        assert_eq!(whole.len(), 100);

        let mut got = Vec::new();
        let mut after = None;
        let mut pages = 0;
        loop {
            let before = got.len();
            let s = page(
                &root,
                paged_limits(),
                &Page {
                    session: None,
                    agent: None,
                    after,
                },
                &mut got,
            );
            assert!(got.len() - before >= 20 || s.next.is_none(), "{pages}");
            pages += 1;
            assert!(pages < 20, "the cursor never ends");
            match s.next {
                Some(n) => after = Some(n),
                None => break,
            }
        }
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), got.len(), "a hit came twice");
        let mut want = whole;
        want.sort();
        assert_eq!(sorted, want, "a hit was skipped");
        assert!(pages >= 5);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_appended_between_pages_repeats_and_skips_nothing() {
        let root = projects();
        write_quokka(&root, "paged-a", 0, 70, false);
        write_quokka(&root, "paged-b", 0, 30, false);
        let mut first = Vec::new();
        let s = page(&root, paged_limits(), &Page::default(), &mut first);
        let (id, _) = first.last().unwrap().clone();
        // The conversation the page stopped in grows. (One not reached yet that grows moves to the front of the
        // listing, so a later page leaves it to a fresh search rather than risk showing a hit twice.)
        let len = if id == "paged-a" { 70 } else { 30 };
        write_quokka(&root, &id, len, 5, true);
        let mut got = first.clone();
        let mut after = s.next;
        while let Some(n) = after.take() {
            let s = page(
                &root,
                paged_limits(),
                &Page {
                    session: None,
                    agent: None,
                    after: Some(n),
                },
                &mut got,
            );
            assert!(!s.restarted, "an append is not a rewrite");
            after = s.next;
        }
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), got.len(), "a hit came twice");
        // Every hit that was there before the append is seen once; the appended ones are the newest, past the cursor.
        let mut before: Vec<_> = got.iter().filter(|(s, _)| *s == id).collect();
        before.sort();
        assert_eq!(before.len(), if id == "paged-a" { 70 } else { 30 });
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_rewritten_file_restarts_and_says_so() {
        let root = projects();
        write_quokka(&root, "paged-a", 0, 70, false);
        let mut first = Vec::new();
        let s = page(&root, paged_limits(), &Page::default(), &mut first);
        // Compacted: another inode, fewer bytes.
        std::fs::remove_file(root.join("-home-fixture-project/paged-a.jsonl")).unwrap();
        write_quokka(&root, "paged-a", 0, 10, false);
        let mut got = Vec::new();
        let s = page(
            &root,
            paged_limits(),
            &Page {
                session: None,
                agent: None,
                after: s.next,
            },
            &mut got,
        );
        assert!(s.restarted);
        assert_eq!(got.len(), 10);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn one_conversation_returns_all_its_matches_whatever_the_overall_cap() {
        let root = projects();
        write_quokka(&root, "paged-a", 0, 70, false);
        write_quokka(&root, "paged-b", 0, 30, false);
        let mut got = Vec::new();
        let s = page(
            &root,
            Limits::default(),
            &Page {
                session: Some("paged-b".into()),
                agent: None,
                after: None,
            },
            &mut got,
        );
        assert_eq!(got.len(), 30);
        assert!(got.iter().all(|(id, _)| id == "paged-b"));
        assert_eq!(s.next, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn one_conversation_page_keeps_only_the_agent_it_names() {
        let root = projects();
        write_quokka(&root, "paged-b", 0, 30, false);
        let ask = |agent: &str| {
            let mut got = Vec::new();
            page(
                &root,
                Limits::default(),
                &Page {
                    session: Some("paged-b".into()),
                    agent: Some(agent.into()),
                    after: None,
                },
                &mut got,
            );
            got.len()
        };
        assert_eq!(ask("claude"), 30);
        assert_eq!(ask("codex"), 0, "another agent's id is not this one");
        let _ = std::fs::remove_dir_all(root);
    }
}
