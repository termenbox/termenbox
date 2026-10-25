//! Which workspace an agent's event belongs to.
//!
//! A hook or plugin names its workspace through the environment it inherited (`TERMENBOX_SESSION`). That is true for
//! an agent that runs inside the workspace's own panes, and false for one that does not: OpenCode keeps one shared
//! `opencode serve` process for every TUI of the user, so each plugin event carries the workspace of whichever TUI
//! started the service, and one workspace's turn, question and end land on another workspace.
//!
//! The rule here is that an event is attributed only to a workspace the reporter provably belongs to:
//!
//! * the reporter's process ancestry reaches a pane of the workspace it names: delivered there;
//! * it reaches a pane of another workspace: delivered to that one (the environment was inherited or stale);
//! * it reaches no pane (a shared service): the agent's own workspace id is bound to a tmux workspace by evidence (the
//!   TUI was started with `-s <id>`, the prompt it was given is on one screen, or only one TUI exists), and the
//!   event waits for that evidence for a short while; with none it is dropped, never guessed;
//! * a turn is owned by the agent workspace that opened it, so a child or sub-agent workspace cannot close it.
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use crate::backend::{valid_name, Runner};

/// How long a World read is reused for provenance checks. Evidence that needs a screen is read fresh.
const WORLD_TTL_MS: u64 = 1_500;
/// A turn owner that has said nothing for this long no longer blocks another workspace from opening a turn.
const TURN_OWNER_MS: u64 = 10 * 60 * 1000;
/// Bindings kept; the oldest go first.
const MAX_BOUND: usize = 512;

/// What an event says about itself, as far as attribution is concerned.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub adapter: String,
    /// The workspace the reporter's environment named (the id clients see, prefix included).
    pub declared: String,
    /// The agent's own id for the conversation (Claude's `workspace_id`, OpenCode's `sessionID`).
    pub agent_id: Option<String>,
    /// Parent process ids of the reporter, nearest first. Empty when the reporter did not say.
    pub ancestors: Vec<u32>,
    /// `Some(true)` for an event that opens a turn, `Some(false)` for one that ends it.
    pub turn: Option<bool>,
    /// The first line of the prompt this event is about, when it carries one.
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Deliver the event to this workspace.
    Deliver(String),
    /// Not yet known; try again when more is known.
    Pending,
    /// Not attributable (the reason is for the audit log).
    Drop(&'static str),
}

/// One tmux workspace as far as attribution needs it.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceFacts {
    pub id: String,
    pub pane_pids: Vec<u32>,
    /// Command lines of the OpenCode TUIs under its panes.
    pub tuis: Vec<Vec<String>>,
    /// The visible text of its active pane, read only when evidence needs it.
    pub screen: String,
}

#[derive(Debug, Clone, Default)]
pub struct World {
    pub workspaces: Vec<WorkspaceFacts>,
    pub read_at: u64,
    pub has_screens: bool,
}

impl World {
    fn workspace(&self, id: &str) -> Option<&WorkspaceFacts> {
        self.workspaces.iter().find(|s| s.id == id)
    }
    fn owning(&self, ancestors: &[u32]) -> Option<&WorkspaceFacts> {
        self.workspaces
            .iter()
            .find(|s| s.pane_pids.iter().any(|p| ancestors.contains(p)))
    }
}

#[derive(Default)]
struct State {
    /// `adapter|agent_id` -> (workspace, last used ms)
    bound: HashMap<String, (String, u64)>,
    /// workspace -> (agent id that owns the open turn, last event ms)
    turns: HashMap<String, (String, u64)>,
}

fn key(adapter: &str, id: &str) -> String {
    format!("{adapter}|{id}")
}

/// The decision, over a snapshot. Pure: everything it needs is in its arguments.
fn decide(state: &mut State, world: Option<&World>, r: &Report, now: u64) -> Verdict {
    let trusted = match world {
        Some(w) if !r.ancestors.is_empty() => {
            if w.workspace(&r.declared)
                .is_some_and(|s| s.pane_pids.iter().any(|p| r.ancestors.contains(p)))
            {
                Some(r.declared.clone())
            } else {
                w.owning(&r.ancestors).map(|s| s.id.clone())
            }
        }
        // tmux could not be read: what the conversation was bound to earlier is better than the environment's word.
        None if !r.ancestors.is_empty() => r
            .agent_id
            .as_ref()
            .and_then(|id| state.bound.get(&key(&r.adapter, id)))
            .map(|(s, _)| s.clone())
            .or_else(|| Some(r.declared.clone())),
        // No evidence either way (an older hook): the environment's word stands.
        _ => Some(r.declared.clone()),
    };
    let workspace = match trusted {
        Some(s) => s,
        None => match shared_owner(state, world, r, now) {
            Ok(s) => s,
            Err(v) => return v,
        },
    };
    if let Some(id) = &r.agent_id {
        state
            .bound
            .insert(key(&r.adapter, id), (workspace.clone(), now));
        if state.bound.len() > MAX_BOUND {
            if let Some(oldest) = state
                .bound
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(k, _)| k.clone())
            {
                state.bound.remove(&oldest);
            }
        }
    }
    turn_gate(state, &workspace, r, now)
}

/// A reporter outside every pane (a shared service): find the workspace by what the agent's id is bound to or by
/// evidence.
fn shared_owner(
    state: &mut State,
    world: Option<&World>,
    r: &Report,
    now: u64,
) -> Result<String, Verdict> {
    if r.adapter != "opencode" {
        // Only OpenCode has a shared service; any other agent reporting from outside its panes is not ours to place.
        return Err(Verdict::Drop(
            "reporter is in none of the workspace's panes",
        ));
    }
    let Some(id) = &r.agent_id else {
        return sole_tui(world).ok_or(Verdict::Pending);
    };
    if let Some((s, at)) = state.bound.get_mut(&key(&r.adapter, id)) {
        *at = now;
        return Ok(s.clone());
    }
    let Some(w) = world else {
        return Err(Verdict::Pending);
    };
    let tuis: Vec<&WorkspaceFacts> = w.workspaces.iter().filter(|s| !s.tuis.is_empty()).collect();
    if tuis.is_empty() {
        return Err(Verdict::Pending);
    }
    // 1. A TUI started on this conversation (`opencode -s <id>`).
    let asked: Vec<&&WorkspaceFacts> = tuis
        .iter()
        .filter(|s| {
            s.tuis.iter().any(|args| {
                args.windows(2)
                    .any(|p| (p[0] == "-s" || p[0] == "--session") && p[1] == *id)
            })
        })
        .collect();
    if asked.len() == 1 {
        return Ok(asked[0].id.clone());
    }
    // 2. The prompt it was given is on exactly one screen.
    if let Some(p) = r
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| p.chars().count() >= 4)
    {
        if w.has_screens {
            let shown: Vec<&&WorkspaceFacts> =
                tuis.iter().filter(|s| s.screen.contains(p)).collect();
            if shown.len() == 1 {
                return Ok(shown[0].id.clone());
            }
        } else {
            return Err(Verdict::Pending);
        }
    }
    // 3. There is only one TUI, so there is only one place it can be.
    if tuis.len() == 1 {
        return Ok(tuis[0].id.clone());
    }
    Err(Verdict::Pending)
}

fn sole_tui(world: Option<&World>) -> Option<String> {
    let w = world?;
    let mut it = w.workspaces.iter().filter(|s| !s.tuis.is_empty());
    let first = it.next()?;
    it.next().is_none().then(|| first.id.clone())
}

/// A turn belongs to the agent workspace that opened it: another one (a child, a sub-agent) neither opens a second
/// turn on top of it nor ends it.
fn turn_gate(state: &mut State, workspace: &str, r: &Report, now: u64) -> Verdict {
    let (Some(open), Some(id)) = (r.turn, r.agent_id.as_deref()) else {
        if let (Some(id), Some((owner, at))) =
            (r.agent_id.as_deref(), state.turns.get_mut(workspace))
        {
            if owner == id {
                *at = now;
            }
        }
        return Verdict::Deliver(workspace.to_string());
    };
    let owner = state
        .turns
        .get(workspace)
        .filter(|(_, at)| now.saturating_sub(*at) < TURN_OWNER_MS)
        .map(|(o, _)| o.clone());
    match (open, owner) {
        (true, Some(o)) if o != id => Verdict::Drop("another conversation owns the open turn"),
        (true, _) => {
            state
                .turns
                .insert(workspace.to_string(), (id.to_string(), now));
            Verdict::Deliver(workspace.to_string())
        }
        (false, Some(o)) if o != id => {
            Verdict::Drop("a conversation that does not own the turn ended")
        }
        (false, _) => {
            state.turns.remove(workspace);
            Verdict::Deliver(workspace.to_string())
        }
    }
}

/// The resolver the event socket asks. Without a runner (not attached yet, or no tmux) it trusts the environment.
#[derive(Default)]
pub struct Resolver {
    source: OnceLock<(Runner, String)>,
    state: Mutex<State>,
    cache: Mutex<Option<World>>,
}

impl Resolver {
    pub fn attach(&self, runner: Runner, prefix: String) {
        let _ = self.source.set((runner, prefix));
    }

    pub async fn resolve(&self, r: &Report, now: u64) -> Verdict {
        let Some((runner, prefix)) = self.source.get() else {
            return Verdict::Deliver(r.declared.clone());
        };
        // Attribution only concerns this manager's workspaces.
        if !prefix.is_empty() && !r.declared.starts_with(prefix.as_str()) {
            return Verdict::Deliver(r.declared.clone());
        }
        let first = self.world(runner, prefix, now, false, false).await;
        let mut v = decide(&mut self.state.lock().unwrap(), first.as_ref(), r, now);
        // A workspace the cache has not seen yet, or evidence that needs a closer look (TUIs, screens): read again, once.
        if matches!(v, Verdict::Pending | Verdict::Drop(_)) {
            let w = self.world(runner, prefix, now, true, true).await;
            v = decide(&mut self.state.lock().unwrap(), w.as_ref(), r, now);
        }
        v
    }

    async fn world(
        &self,
        runner: &Runner,
        prefix: &str,
        now: u64,
        tuis: bool,
        screens: bool,
    ) -> Option<World> {
        if !tuis && !screens {
            if let Some(w) = self.cache.lock().unwrap().as_ref() {
                if now.saturating_sub(w.read_at) < WORLD_TTL_MS {
                    return Some(w.clone());
                }
            }
        }
        let w = read_world(runner, prefix, now, tuis, screens).await?;
        if !tuis && !screens {
            *self.cache.lock().unwrap() = Some(w.clone());
        }
        Some(w)
    }
}

/// Every pane's workspace and pid, and (when asked) the OpenCode TUIs under them and their screens.
async fn read_world(
    runner: &Runner,
    prefix: &str,
    now: u64,
    tuis: bool,
    screens: bool,
) -> Option<World> {
    let args = ["list-panes", "-a", "-F", "#{session_name}|#{pane_pid}"].map(String::from);
    let out = runner.run("tmux", &args).await.ok()?;
    if !out.status.success() {
        return None;
    }
    let mut by: Vec<WorkspaceFacts> = Vec::new();
    for l in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((name, pid)) = l.rsplit_once('|') else {
            continue;
        };
        let Ok(pid) = pid.trim().parse::<u32>() else {
            continue;
        };
        let id = format!("{prefix}{name}");
        match by.iter_mut().find(|s| s.id == id) {
            Some(s) => s.pane_pids.push(pid),
            None => by.push(WorkspaceFacts {
                id,
                pane_pids: vec![pid],
                ..Default::default()
            }),
        }
    }
    if tuis {
        let table = tokio::task::spawn_blocking(ProcTable::read).await.ok()?;
        for s in &mut by {
            s.tuis = s
                .pane_pids
                .iter()
                .flat_map(|p| table.tuis_under(*p))
                .collect();
        }
    }
    if screens {
        for s in by.iter_mut().filter(|s| !s.tuis.is_empty()) {
            let name = s.id.strip_prefix(prefix).unwrap_or(&s.id).to_string();
            if !valid_name(&name) {
                continue;
            }
            let cap = ["capture-pane", "-p", "-t", &format!("={name}:")].map(String::from);
            if let Ok(o) = runner.run("tmux", &cap).await {
                if o.status.success() {
                    s.screen = String::from_utf8_lossy(&o.stdout).into_owned();
                }
            }
        }
    }
    Some(World {
        workspaces: by,
        read_at: now,
        has_screens: screens,
    })
}

/// The process table, for walking from a pane's process down to the TUIs under it.
#[derive(Default)]
pub struct ProcTable {
    procs: HashMap<u32, (u32, Vec<String>)>,
}

/// OpenCode subcommands that are not the interactive TUI.
const NOT_A_TUI: [&str; 12] = [
    "serve", "run", "acp", "web", "export", "import", "auth", "mcp", "models", "stats", "github",
    "upgrade",
];

impl ProcTable {
    pub fn read() -> Self {
        let mut procs = HashMap::new();
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Self::default();
        };
        for e in dir.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let Some(ppid) = ppid_of(pid) else { continue };
            let args = std::fs::read(e.path().join("cmdline"))
                .map(|b| {
                    b.split(|c| *c == 0)
                        .filter(|a| !a.is_empty())
                        .map(|a| String::from_utf8_lossy(a).into_owned())
                        .collect()
                })
                .unwrap_or_default();
            procs.insert(pid, (ppid, args));
        }
        Self { procs }
    }

    #[cfg(test)]
    fn of(list: &[(u32, u32, &[&str])]) -> Self {
        Self {
            procs: list
                .iter()
                .map(|(p, pp, a)| (*p, (*pp, a.iter().map(|s| s.to_string()).collect())))
                .collect(),
        }
    }

    /// Command lines of the OpenCode TUIs that are `root` or run below it.
    pub fn tuis_under(&self, root: u32) -> Vec<Vec<String>> {
        let mut below: HashSet<u32> = HashSet::from([root]);
        loop {
            let before = below.len();
            for (pid, (ppid, _)) in &self.procs {
                if below.contains(ppid) {
                    below.insert(*pid);
                }
            }
            if below.len() == before {
                break;
            }
        }
        below
            .iter()
            .filter_map(|p| self.procs.get(p))
            .filter(|(_, args)| is_tui(args))
            .map(|(_, args)| args.clone())
            .collect()
    }
}

fn is_tui(args: &[String]) -> bool {
    // `opencode ...`, or an interpreter running it (`node /path/opencode ...`, how a package install starts it).
    let Some(at) = args
        .iter()
        .take(2)
        .position(|a| a.rsplit('/').next() == Some("opencode"))
    else {
        return false;
    };
    !args
        .get(at + 1)
        .is_some_and(|a| NOT_A_TUI.contains(&a.as_str()))
}

/// Parent of `pid`, from `/proc/<pid>/stat` (the name in field 2 may hold spaces and brackets: read after the last `)`).
pub fn ppid_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// The parent chain of this process, nearest first, for a hook to hand to the server.
pub fn own_ancestors() -> Vec<u32> {
    let mut out = Vec::new();
    let mut pid = std::process::id();
    while out.len() < 32 {
        match ppid_of(pid) {
            Some(p) if p > 1 => {
                out.push(p);
                pid = p;
            }
            _ => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(id: &str, panes: &[u32], tuis: &[&[&str]], screen: &str) -> WorkspaceFacts {
        WorkspaceFacts {
            id: id.into(),
            pane_pids: panes.to_vec(),
            tuis: tuis
                .iter()
                .map(|t| t.iter().map(|s| s.to_string()).collect())
                .collect(),
            screen: screen.into(),
        }
    }
    fn world(workspaces: Vec<WorkspaceFacts>) -> World {
        World {
            workspaces,
            read_at: 0,
            has_screens: true,
        }
    }
    fn report(adapter: &str, declared: &str, id: Option<&str>, ancestors: &[u32]) -> Report {
        Report {
            adapter: adapter.into(),
            declared: declared.into(),
            agent_id: id.map(String::from),
            ancestors: ancestors.to_vec(),
            ..Default::default()
        }
    }

    #[test]
    fn a_reporter_inside_the_declared_workspace_is_trusted() {
        let w = world(vec![
            workspace("a", &[100], &[], ""),
            workspace("b", &[200], &[], ""),
        ]);
        let mut st = State::default();
        let r = report("claude", "a", Some("c1"), &[150, 100, 50]);
        assert_eq!(
            decide(&mut st, Some(&w), &r, 1),
            Verdict::Deliver("a".into())
        );
    }

    #[test]
    fn a_reporter_inside_another_workspace_goes_to_that_workspace() {
        // The environment was inherited from a workspace that is not the one the process runs in.
        let w = world(vec![
            workspace("a", &[100], &[], ""),
            workspace("b", &[200], &[], ""),
        ]);
        let r = report("claude", "a", Some("c1"), &[250, 200]);
        assert_eq!(
            decide(&mut State::default(), Some(&w), &r, 1),
            Verdict::Deliver("b".into())
        );
    }

    #[test]
    fn without_ancestors_or_tmux_the_environment_stands() {
        let r = report("claude", "a", None, &[]);
        assert_eq!(
            decide(&mut State::default(), None, &r, 1),
            Verdict::Deliver("a".into())
        );
        let w = world(vec![workspace("b", &[200], &[], "")]);
        assert_eq!(
            decide(&mut State::default(), Some(&w), &r, 1),
            Verdict::Deliver("a".into())
        );
    }

    #[test]
    fn a_non_opencode_reporter_outside_every_pane_is_dropped() {
        let w = world(vec![workspace("a", &[100], &[], "")]);
        let r = report("claude", "a", Some("c1"), &[9, 8]);
        assert!(matches!(
            decide(&mut State::default(), Some(&w), &r, 1),
            Verdict::Drop(_)
        ));
    }

    #[test]
    fn a_shared_opencode_service_is_placed_by_the_tui_started_on_the_conversation() {
        let w = world(vec![
            workspace("workspace-opencode", &[100], &[&["opencode"]], ""),
            workspace(
                "projects-opencode",
                &[200],
                &[&["opencode", "-s", "ses_B"]],
                "",
            ),
        ]);
        // The service runs outside every pane and its environment names the first workspace.
        let r = report("opencode", "workspace-opencode", Some("ses_B"), &[7777]);
        assert_eq!(
            decide(&mut State::default(), Some(&w), &r, 1),
            Verdict::Deliver("projects-opencode".into())
        );
    }

    #[test]
    fn a_shared_opencode_service_is_placed_by_the_prompt_on_one_screen() {
        let w = world(vec![
            workspace("one", &[100], &[&["opencode"]], "> fix the failing test\n"),
            workspace(
                "two",
                &[200],
                &[&["opencode"]],
                "> write the release notes\n",
            ),
        ]);
        let mut r = report("opencode", "one", Some("ses_X"), &[7777]);
        r.prompt = Some("write the release notes".into());
        let mut st = State::default();
        assert_eq!(
            decide(&mut st, Some(&w), &r, 1),
            Verdict::Deliver("two".into())
        );
        // The conversation stays there, without the prompt.
        r.prompt = None;
        r.turn = Some(true);
        assert_eq!(decide(&mut st, None, &r, 2), Verdict::Deliver("two".into()));
    }

    #[test]
    fn with_several_tuis_and_no_evidence_the_event_waits_instead_of_guessing() {
        let w = world(vec![
            workspace("one", &[100], &[&["opencode"]], ""),
            workspace("two", &[200], &[&["opencode"]], ""),
        ]);
        let r = report("opencode", "one", Some("ses_X"), &[7777]);
        assert_eq!(
            decide(&mut State::default(), Some(&w), &r, 1),
            Verdict::Pending
        );
    }

    #[test]
    fn a_single_tui_takes_the_events_of_a_shared_service() {
        let w = world(vec![
            workspace("one", &[100], &[&["opencode"]], ""),
            workspace("sh", &[300], &[], ""),
        ]);
        let r = report("opencode", "sh", Some("ses_X"), &[7777]);
        assert_eq!(
            decide(&mut State::default(), Some(&w), &r, 1),
            Verdict::Deliver("one".into())
        );
    }

    #[test]
    fn a_child_conversation_neither_opens_a_second_turn_nor_ends_the_first() {
        let w = world(vec![workspace("a", &[100], &[], "")]);
        let mut st = State::default();
        let mut open = report("opencode", "a", Some("parent"), &[100]);
        open.turn = Some(true);
        assert_eq!(
            decide(&mut st, Some(&w), &open, 1),
            Verdict::Deliver("a".into())
        );
        let mut child_open = report("opencode", "a", Some("child"), &[100]);
        child_open.turn = Some(true);
        assert!(matches!(
            decide(&mut st, Some(&w), &child_open, 2),
            Verdict::Drop(_)
        ));
        let mut child_end = report("opencode", "a", Some("child"), &[100]);
        child_end.turn = Some(false);
        assert!(
            matches!(decide(&mut st, Some(&w), &child_end, 3), Verdict::Drop(_)),
            "the parent is still working"
        );
        let mut end = report("opencode", "a", Some("parent"), &[100]);
        end.turn = Some(false);
        assert_eq!(
            decide(&mut st, Some(&w), &end, 4),
            Verdict::Deliver("a".into())
        );
        // The turn is over: the next conversation may open one.
        assert_eq!(
            decide(&mut st, Some(&w), &child_open, 5),
            Verdict::Deliver("a".into())
        );
    }

    #[test]
    fn a_turn_owner_that_went_silent_does_not_block_a_new_turn_forever() {
        let w = world(vec![workspace("a", &[100], &[], "")]);
        let mut st = State::default();
        let mut open = report("claude", "a", Some("old"), &[100]);
        open.turn = Some(true);
        decide(&mut st, Some(&w), &open, 1);
        let mut next = report("claude", "a", Some("new"), &[100]);
        next.turn = Some(true);
        assert_eq!(
            decide(&mut st, Some(&w), &next, TURN_OWNER_MS + 2),
            Verdict::Deliver("a".into())
        );
    }

    #[test]
    fn tuis_are_found_below_a_pane_but_the_shared_service_is_not_one() {
        let t = ProcTable::of(&[
            (100, 1, &["bash"]),
            (101, 100, &["/home/u/.opencode/bin/opencode", "-s", "ses_1"]),
            (102, 100, &["opencode", "serve", "--service"]),
            (300, 1, &["bash"]),
            (301, 300, &["opencode"]),
        ]);
        let tuis = t.tuis_under(100);
        assert_eq!(
            tuis,
            vec![vec![
                "/home/u/.opencode/bin/opencode".to_string(),
                "-s".into(),
                "ses_1".into()
            ]]
        );
        assert_eq!(t.tuis_under(300).len(), 1);
    }

    #[test]
    fn the_parent_is_read_after_the_last_paren_of_the_name() {
        assert!(ppid_of(std::process::id()).is_some());
        assert!(!own_ancestors().is_empty() || ppid_of(std::process::id()) == Some(1));
    }

    /// Real tmux, real processes: panes found by pid, an `opencode` TUI found below a pane by its command line.
    #[tokio::test]
    async fn the_resolver_places_events_by_the_processes_of_a_real_tmux() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tm-owner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A program called opencode that does nothing, so the process table has a TUI to find.
        let fake = dir.join("opencode");
        std::fs::write(&fake, "#!/bin/sh\nsleep 600\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let tmpdir = dir.to_string_lossy().into_owned();
        let runner =
            Runner::default().with_env(&[("TMUX_TMPDIR", Some(tmpdir.as_str())), ("TMUX", None)]);
        let run = |args: &[&str]| {
            let (runner, args) = (
                runner.clone(),
                args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
            );
            async move { runner.run("tmux", &args).await.unwrap() }
        };
        run(&["new-session", "-d", "-s", "ra", "sleep 600"]).await;
        run(&["new-session", "-d", "-s", "rb", "sleep 600"]).await;
        let tui = format!("{} -s ses_Z", fake.display());
        run(&["new-session", "-d", "-s", "rc", &tui]).await;
        run(&[
            "new-session",
            "-d",
            "-s",
            "rd",
            &format!("{}", fake.display()),
        ])
        .await;
        let panes = String::from_utf8_lossy(
            &run(&["list-panes", "-a", "-F", "#{session_name} #{pane_pid}"])
                .await
                .stdout,
        )
        .into_owned();
        let pid_of = |name: &str| -> u32 {
            panes
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{name} ")))
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        };
        let resolver = Resolver::default();
        resolver.attach(runner.clone(), String::new());
        let now = 1_000_000;

        // Inside the workspace it names, and inside another one.
        let own = report("claude", "ra", Some("c1"), &[1234, pid_of("ra")]);
        assert_eq!(
            resolver.resolve(&own, now).await,
            Verdict::Deliver("ra".into())
        );
        let other = report("claude", "ra", Some("c1"), &[pid_of("rb")]);
        assert_eq!(
            resolver.resolve(&other, now).await,
            Verdict::Deliver("rb".into())
        );
        // In no pane at all, for an agent that has no shared service.
        let stray = report("claude", "ra", Some("c1"), &[4_000_000]);
        assert!(matches!(
            resolver.resolve(&stray, now).await,
            Verdict::Drop(_)
        ));

        // The shared OpenCode service (outside every pane) reports for the conversation a TUI was started on.
        let shared = report("opencode", "ra", Some("ses_Z"), &[4_000_000]);
        assert_eq!(
            resolver.resolve(&shared, now).await,
            Verdict::Deliver("rc".into())
        );
        // Two TUIs and nothing that tells the conversations apart: it waits.
        let unknown = report("opencode", "ra", Some("ses_Q"), &[4_000_000]);
        assert_eq!(resolver.resolve(&unknown, now).await, Verdict::Pending);

        let _ = runner.run("tmux", &["kill-server".to_string()]).await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
