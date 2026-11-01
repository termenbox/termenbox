use super::*;
use crate::backend::{CreatedWorkspace, Runner, TmuxBackend, WorkspaceBackend};
use termenbox_core::protocol::WindowInfo;

fn temp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("termenbox-registry-{name}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn tmux(dir: &Path) -> TmuxBackend {
    let tmp = dir.to_string_lossy().into_owned();
    TmuxBackend::new(
        Runner::default().with_env(&[("TMUX_TMPDIR", Some(tmp.as_str())), ("TMUX", None)]),
    )
}

async fn stop(dir: &Path) {
    let tmp = dir.to_string_lossy().into_owned();
    let runner = Runner::default().with_env(&[("TMUX_TMPDIR", Some(tmp.as_str())), ("TMUX", None)]);
    let _ = runner.run("tmux", &["kill-server".to_string()]).await;
}

#[test]
fn a_relative_directory_inside_the_workspace_resolves_and_an_escape_is_refused() {
    let dir = temp("dirs");
    let sub = dir.join("api");
    std::fs::create_dir_all(&sub).unwrap();
    let root = dir.to_string_lossy().to_string();

    assert_eq!(
        resolve_tab_dir(&root, "api").unwrap(),
        sub.canonicalize().unwrap().to_string_lossy()
    );
    assert_eq!(
        resolve_tab_dir(&root, "").unwrap(),
        dir.canonicalize().unwrap().to_string_lossy()
    );
    assert!(resolve_tab_dir(&root, "../").is_err());
    assert!(resolve_tab_dir(&root, "/etc").is_err());
    assert!(resolve_tab_dir(&root, "missing").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_tab_id_is_unique_and_labels_do_not_collide() {
    let mut record = WorkspaceRecord {
        id: "ws-1".into(),
        label: "Work".into(),
        dir: "/".into(),
        backend: None,
        order: 0,
        focus: None,
        tabs: vec![TabRecord {
            id: "t1".into(),
            label: "Shell".into(),
            kind: "shell".into(),
            dir: String::new(),
            order: 0,
            window: Some("@1".into()),
            agent_session: None,
        }],
        next_tab: 2,
    };
    let id = "t2".to_string();
    record.next_tab = 3;
    record.tabs.push(TabRecord {
        id,
        label: unique_label("Shell", &record.tabs).into(),
        kind: "shell".into(),
        dir: String::new(),
        order: 1,
        window: Some("@2".into()),
        agent_session: None,
    });
    assert_eq!(record.tabs[1].label.as_str(), "Shell 2");
    assert_eq!(record.tabs[0].id, "t1");
    assert_eq!(record.tabs[1].id, "t2");
}

#[tokio::test]
async fn a_workspace_gets_a_first_tab_and_tabs_can_be_added_renamed_moved_and_closed() {
    let tmp = temp("life");
    let ws = tmp.join("app");
    std::fs::create_dir_all(&ws).unwrap();
    let backend = tmux(&tmp).await;
    let registry = Registry::memory();
    let root = ws.to_string_lossy().to_string();

    let id = registry
        .create_workspace(&backend, "Work", None, Some(&root), "shell")
        .await
        .unwrap();
    let listed = registry.list(&backend).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].name.as_str(), "Work");
    assert_eq!(listed[0].tabs.len(), 1);
    assert!(listed[0].tabs[0].window.is_some());

    let second = registry
        .new_tab(&backend, &id, "shell", "Logs", "")
        .await
        .unwrap();
    assert_eq!(second.order, 1);
    let listed = registry.list(&backend).await.unwrap();
    assert_eq!(listed[0].tabs.len(), 2);

    registry
        .rename_tab(&backend, &id, &second.id, "Build")
        .await
        .unwrap();
    registry
        .move_tab(&backend, &id, &second.id, 0)
        .await
        .unwrap();
    let listed = registry.list(&backend).await.unwrap();
    assert_eq!(listed[0].tabs[0].label.as_str(), "Build");
    assert_eq!(listed[0].tabs[0].id, second.id);

    registry
        .close_tab(&backend, &id, &listed[0].tabs[0].id)
        .await
        .unwrap();
    let listed = registry.list(&backend).await.unwrap();
    assert_eq!(listed[0].tabs.len(), 1);

    // The last tab leaves a shell behind, so the workspace is never empty.
    let last = listed[0].tabs[0].id.clone();
    registry.close_tab(&backend, &id, &last).await.unwrap();
    let listed = registry.list(&backend).await.unwrap();
    assert_eq!(listed[0].tabs.len(), 1);
    assert_ne!(listed[0].tabs[0].id, last);

    registry.close_workspace(&backend, &id).await.unwrap();
    assert!(registry.list(&backend).await.unwrap().is_empty());
    stop(&tmp).await;
    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn a_migrated_tmux_session_keeps_its_windows_as_tabs_and_comes_back_as_shells_when_dormant() {
    let tmp = temp("migrate");
    let ws = tmp.join("legacy");
    std::fs::create_dir_all(&ws).unwrap();
    let root = ws.to_string_lossy().to_string();
    let backend = tmux(&tmp).await;
    let socket = tmp.to_string_lossy().into_owned();
    let runner =
        Runner::default().with_env(&[("TMUX_TMPDIR", Some(socket.as_str())), ("TMUX", None)]);
    for args in [
        vec!["new-session", "-d", "-s", "legacy", "-c", &root],
        vec!["rename-window", "-t", "=legacy:", "main"],
        vec!["new-window", "-t", "=legacy:", "-n", "logs"],
    ] {
        assert!(runner
            .run(
                "tmux",
                &args.into_iter().map(String::from).collect::<Vec<_>>()
            )
            .await
            .unwrap()
            .status
            .success());
    }
    let registry = Registry::memory();
    let listed = registry.list(&backend).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "legacy");
    assert_eq!(listed[0].tabs.len(), 2);
    assert_eq!(listed[0].tabs[0].label.as_str(), "main");
    assert_eq!(listed[0].tabs[1].label.as_str(), "logs");

    // A tmux server that died leaves the record dormant; opening a tab recreates the session and its tabs.
    stop(&tmp).await;
    let listed = registry.list(&backend).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].tabs.iter().all(|t| t.window.is_none()));
    let first = listed[0].tabs[0].id.clone();
    let window = registry
        .tab_window(&backend, "legacy", Some(&first))
        .await
        .unwrap();
    assert!(window.is_some());
    assert_eq!(backend.windows("legacy").await.unwrap().len(), 2);

    stop(&tmp).await;
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A backend that keeps window ids in memory, so the cap rules can be tested without spawning agents in tmux.
struct Fake {
    windows: Mutex<HashMap<String, Vec<String>>>,
    next: Mutex<u32>,
    /// Every tab the registry asked for, in order: the template and the resume command each was started with.
    specs: Mutex<Vec<(String, Option<String>)>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            next: Mutex::new(0),
            specs: Mutex::new(Vec::new()),
        }
    }

    fn mint(&self, workspace: &str) -> String {
        let mut next = self.next.lock().unwrap();
        *next += 1;
        let window = format!("@{next}");
        self.windows
            .lock()
            .unwrap()
            .entry(workspace.to_string())
            .or_default()
            .push(window.clone());
        window
    }
}

#[async_trait::async_trait]
impl WorkspaceBackend for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn kinds(&self) -> Vec<String> {
        vec!["tmux".into()]
    }

    async fn list(&self) -> Result<Vec<WorkspaceInfo>> {
        Ok(Vec::new())
    }

    async fn create(&self, _: &str) -> Result<()> {
        Ok(())
    }

    fn attach(&self, _: &str, _: u16, _: u16) -> Result<Attachment> {
        Err(anyhow!("no terminal"))
    }

    async fn create_workspace(
        &self,
        name: &str,
        _: Option<&str>,
        _: Option<&str>,
        _: Option<&str>,
        _: &TabSpec,
    ) -> Result<CreatedWorkspace> {
        let id = format!("ws-{name}");
        let window = self.mint(&id);
        Ok(CreatedWorkspace {
            id,
            window: Some(window),
        })
    }

    async fn windows(&self, id: &str) -> Result<Vec<WindowState>> {
        let windows = self
            .windows
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("no session"))?;
        Ok(windows
            .into_iter()
            .enumerate()
            .map(|(i, window_id)| WindowState {
                info: WindowInfo {
                    index: i as u32,
                    name: String::new(),
                    command: String::new(),
                    path: String::new(),
                    active: i == 0,
                    agent: None,
                },
                activity: 0,
                window_id,
                tab: String::new(),
            })
            .collect())
    }

    async fn new_tab(&self, id: &str, spec: &TabSpec) -> Result<String> {
        self.specs
            .lock()
            .unwrap()
            .push((spec.kind.clone(), spec.resume.clone()));
        if !self.windows.lock().unwrap().contains_key(id) {
            return Err(anyhow!("no session"));
        }
        Ok(self.mint(id))
    }

    async fn ensure_session(&self, id: &str, _: &str, _: &str, spec: &TabSpec) -> Result<String> {
        self.specs
            .lock()
            .unwrap()
            .push((spec.kind.clone(), spec.resume.clone()));
        self.windows
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default();
        Ok(self.mint(id))
    }

    async fn focus_tab(&self, _: &str, _: &str) -> Result<()> {
        Ok(())
    }

    async fn close_tab(&self, id: &str, window: &str) -> Result<()> {
        if let Some(windows) = self.windows.lock().unwrap().get_mut(id) {
            windows.retain(|w| w != window);
        }
        Ok(())
    }

    async fn kill(&self, id: &str) -> Result<()> {
        self.windows.lock().unwrap().remove(id);
        Ok(())
    }
}

async fn fake_workspace(
    registry: &Registry,
    backend: &Fake,
    name: &str,
    dir: &Path,
    kind: &str,
) -> String {
    let root = dir.to_string_lossy().into_owned();
    registry
        .create_workspace(backend, name, None, Some(&root), kind)
        .await
        .unwrap()
}

#[tokio::test]
async fn tmux_current_window_becomes_the_focus_but_only_for_a_known_tab() {
    let dir = temp("observe-focus");
    let backend = Fake::new();
    let registry = Registry::memory();
    let work = fake_workspace(&registry, &backend, "Work", &dir, "claude").await;
    let second = registry
        .new_tab(&backend, &work, "shell", "Logs", "")
        .await
        .unwrap();
    let focus = || registry.record(&work).unwrap().focus;
    let first = registry.record(&work).unwrap().tabs[0].id.clone();

    assert_eq!(focus().as_deref(), Some(second.id.as_str()));
    // tmux moved back to its first window: the focus follows the window, not the clock.
    assert!(registry.observe_focus(&work, &first));
    assert_eq!(focus().as_deref(), Some(first.as_str()));
    // The same window again, an unknown tab and an unknown workspace change nothing.
    assert!(!registry.observe_focus(&work, &first));
    assert!(!registry.observe_focus(&work, "t99"));
    assert!(!registry.observe_focus("gone", "t1"));
    assert!(!registry.observe_focus(&work, ""));
    assert_eq!(focus().as_deref(), Some(first.as_str()));
}

#[tokio::test]
async fn a_light_list_keeps_the_records_and_their_focus_without_asking_for_windows() {
    let dir = temp("light-list");
    let backend = Fake::new();
    let registry = Registry::memory();
    let work = fake_workspace(&registry, &backend, "Work", &dir, "claude").await;
    registry
        .new_tab(&backend, &work, "shell", "Logs", "")
        .await
        .unwrap();

    let full = registry.list(&backend).await.unwrap();
    let light = registry.list_light(&backend).await.unwrap();
    assert_eq!(light.len(), full.len());
    assert_eq!(light[0].id, full[0].id);
    assert_eq!(light[0].focus, full[0].focus);
    assert_eq!(light[0].tabs.len(), full[0].tabs.len());
    assert_eq!(light[0].tabs[1].label, full[0].tabs[1].label);
}

#[tokio::test]
async fn agent_tabs_are_capped_per_workspace_and_closing_one_frees_a_slot() {
    let dir = temp("cap");
    let backend = Fake::new();
    let registry = Registry::memory();
    let work = fake_workspace(&registry, &backend, "Work", &dir, "claude").await;

    // The first agent tab came with the workspace; five more reach the default cap of six.
    for _ in 0..(MAX_AGENT_TABS - 1) {
        registry
            .new_tab(&backend, &work, "codex", "", "")
            .await
            .unwrap();
    }
    assert_eq!(registry.tabs(&work).len(), MAX_AGENT_TABS);

    let refused = registry
        .new_tab(&backend, &work, "claude", "", "")
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        refused,
        "This workspace already has 6 agent tabs. Close one before opening another."
    );

    // The cap is per workspace: another workspace still accepts an agent tab.
    let other = fake_workspace(&registry, &backend, "Other", &dir, "shell").await;
    registry
        .new_tab(&backend, &other, "claude", "", "")
        .await
        .unwrap();
    assert_eq!(agent_tabs(&registry.state.lock().unwrap().workspaces[1]), 1);

    // Closing an agent tab frees exactly one slot.
    let agent = registry
        .tabs(&work)
        .into_iter()
        .find(|t| t.kind != "shell")
        .unwrap();
    registry
        .close_tab(&backend, &work, &agent.id)
        .await
        .unwrap();
    registry
        .new_tab(&backend, &work, "claude", "", "")
        .await
        .unwrap();
    assert_eq!(
        agent_tabs(&registry.state.lock().unwrap().workspaces[0]),
        MAX_AGENT_TABS
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn shell_tabs_still_open_at_the_cap() {
    let dir = temp("cap-shell");
    let backend = Fake::new();
    let registry = Registry::memory();
    let work = fake_workspace(&registry, &backend, "Work", &dir, "claude").await;
    for _ in 0..(MAX_AGENT_TABS - 1) {
        registry
            .new_tab(&backend, &work, "claude", "", "")
            .await
            .unwrap();
    }

    registry
        .new_tab(&backend, &work, "shell", "Logs", "")
        .await
        .unwrap();
    assert_eq!(registry.tabs(&work).len(), MAX_AGENT_TABS + 1);
    assert!(registry
        .new_tab(&backend, &work, "claude", "", "")
        .await
        .is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_dormant_agent_tab_still_counts_toward_the_cap() {
    let dir = temp("cap-dormant");
    let backend = Fake::new();
    let registry = Registry::memory();
    let work = fake_workspace(&registry, &backend, "Work", &dir, "claude").await;
    for _ in 0..(MAX_AGENT_TABS - 1) {
        registry
            .new_tab(&backend, &work, "claude", "", "")
            .await
            .unwrap();
    }
    // A tmux server that died leaves the records behind with no window id.
    for tab in &mut registry.state.lock().unwrap().workspaces[0].tabs {
        tab.window = None;
    }

    let refused = registry
        .new_tab(&backend, &work, "claude", "", "")
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("already has 6 agent tabs"));
    assert_eq!(registry.tabs(&work).len(), MAX_AGENT_TABS);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn two_concurrent_agent_creates_cannot_both_take_the_last_slot() {
    let dir = temp("cap-race");
    let backend = Fake::new();
    let registry = Registry::memory();
    let work = fake_workspace(&registry, &backend, "Work", &dir, "claude").await;
    // Five agent tabs: one slot is left for the two creators to race over.
    for _ in 0..(MAX_AGENT_TABS - 2) {
        registry
            .new_tab(&backend, &work, "claude", "", "")
            .await
            .unwrap();
    }

    let (first, second) = tokio::join!(
        registry.new_tab(&backend, &work, "claude", "", ""),
        registry.new_tab(&backend, &work, "codex", "", "")
    );
    let created = [first.is_ok(), second.is_ok()]
        .iter()
        .filter(|ok| **ok)
        .count();
    assert_eq!(created, 1, "exactly one create takes the last slot");
    assert_eq!(registry.tabs(&work).len(), MAX_AGENT_TABS);
    let freed = registry.state.lock().unwrap().reserved.clone();
    assert!(freed.is_empty(), "every reservation is released");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_registry_file_keeps_the_order_and_the_labels_across_a_reload() {
    let tmp = temp("persist");
    let ws = tmp.join("app");
    std::fs::create_dir_all(&ws).unwrap();
    let root = ws.to_string_lossy().to_string();
    let path = tmp.join("workspaces.json");
    let backend = tmux(&tmp).await;
    let id = {
        let registry = Registry::load(path.clone());
        registry
            .create_workspace(&backend, "Release notes", None, Some(&root), "shell")
            .await
            .unwrap()
    };
    let registry = Registry::load(path);
    let listed = registry.list(&backend).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].name.as_str(), "Release notes");
    assert_eq!(listed[0].tabs.len(), 1);
    stop(&tmp).await;
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Real tmux: the windows are listed as `(index, window id, active)` in index order.
async fn tmux_windows(dir: &Path, session: &str) -> Vec<(u32, String, bool)> {
    let socket = dir.to_string_lossy().into_owned();
    let runner =
        Runner::default().with_env(&[("TMUX_TMPDIR", Some(socket.as_str())), ("TMUX", None)]);
    let args = [
        "list-windows",
        "-t",
        &format!("={session}"),
        "-F",
        "#{window_index} #{window_id} #{window_active}",
    ]
    .map(String::from);
    let out = runner.run("tmux", &args).await.unwrap();
    let mut windows: Vec<(u32, String, bool)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f[0].parse().unwrap(), f[1].to_string(), f[2] == "1")
        })
        .collect();
    windows.sort();
    windows
}

#[tokio::test]
async fn moving_a_tab_reorders_the_tmux_windows_and_closing_one_packs_the_numbers() {
    let tmp = temp("reorder");
    let ws = tmp.join("app");
    std::fs::create_dir_all(&ws).unwrap();
    let backend = tmux(&tmp).await;
    let registry = Registry::memory();
    let root = ws.to_string_lossy().to_string();
    let id = registry
        .create_workspace(&backend, "Work", None, Some(&root), "shell")
        .await
        .unwrap();
    for label in ["Logs", "Build", "Test"] {
        registry
            .new_tab(&backend, &id, "shell", label, "")
            .await
            .unwrap();
    }
    let tabs = registry.record(&id).unwrap().tabs;
    let window = |i: usize| tabs[i].window.clone().unwrap();
    registry
        .focus_tab(&backend, &id, &tabs[2].id)
        .await
        .unwrap();

    // The second tab moves to the first place: tmux lists the same windows in the new order, with the same ids.
    registry
        .move_tab(&backend, &id, &tabs[1].id, 0)
        .await
        .unwrap();
    let after = tmux_windows(&tmp, &id).await;
    assert_eq!(
        after.iter().map(|w| (w.0, w.1.clone())).collect::<Vec<_>>(),
        vec![
            (0, window(1)),
            (1, window(0)),
            (2, window(2)),
            (3, window(3))
        ]
    );
    // The tab that was current is still the current one, and the registry shows the same order.
    assert_eq!(
        after.iter().find(|w| w.2).map(|w| w.1.clone()),
        Some(window(2))
    );
    let order: Vec<String> = registry
        .record(&id)
        .unwrap()
        .tabs
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(
        order,
        vec![
            tabs[1].id.clone(),
            tabs[0].id.clone(),
            tabs[2].id.clone(),
            tabs[3].id.clone()
        ]
    );

    // Closing a tab leaves no gap: Ctrl+B <n> still selects position n.
    registry
        .close_tab(&backend, &id, &tabs[0].id)
        .await
        .unwrap();
    let after = tmux_windows(&tmp, &id).await;
    assert_eq!(
        after.iter().map(|w| (w.0, w.1.clone())).collect::<Vec<_>>(),
        vec![(0, window(1)), (1, window(2)), (2, window(3))]
    );
    stop(&tmp).await;
    let _ = std::fs::remove_dir_all(&tmp);
}

fn tab(id: &str, kind: &str, dir: &str, window: Option<&str>) -> TabRecord {
    TabRecord {
        id: id.into(),
        label: id.into(),
        kind: kind.into(),
        dir: dir.into(),
        order: 0,
        window: window.map(Into::into),
        agent_session: None,
    }
}

fn workspace(id: &str, dir: &str, tabs: Vec<TabRecord>) -> WorkspaceRecord {
    WorkspaceRecord {
        id: id.into(),
        label: id.into(),
        dir: dir.into(),
        backend: None,
        order: 0,
        focus: None,
        tabs,
        next_tab: 9,
    }
}

#[test]
fn known_sessions_list_an_open_and_a_dormant_workspace_and_only_tabs_with_a_session() {
    let registry = Registry::memory();
    registry.state.lock().unwrap().workspaces = vec![
        workspace(
            "ws-up",
            "/home/o/app",
            vec![
                tab("t1", "shell", "", Some("@1")),
                tab("t2", "claude", "", Some("@2")),
                tab("t3", "codex", "api", Some("@3")),
                tab("t4", "claude", "", Some("@4")),
            ],
        ),
        workspace("ws-down", "/home/o/lib", vec![tab("t1", "codex", "", None)]),
    ];
    assert!(registry.set_agent_session("ws-up", "t2", "c-1"));
    assert!(registry.set_agent_session("ws-up", "t3", "x-1"));
    assert!(registry.set_agent_session("ws-down", "t1", "x-2"));
    assert!(
        !registry.set_agent_session("ws-up", "t1", "sh-1"),
        "a shell has no agent session"
    );
    assert!(
        !registry.set_agent_session("ws-up", "t2", "c-1"),
        "an unchanged id is not a write"
    );
    assert!(!registry.set_agent_session("ws-nope", "t1", "z"));
    let got: Vec<(String, String, String, bool)> = registry
        .known_sessions()
        .into_iter()
        .map(|k| {
            (
                k.agent,
                k.session_id,
                k.dir.to_string_lossy().into_owned(),
                k.up,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("claude".into(), "c-1".into(), "/home/o/app".into(), true),
            ("codex".into(), "x-1".into(), "/home/o/app/api".into(), true),
            ("codex".into(), "x-2".into(), "/home/o/lib".into(), false),
        ]
    );
    // A new session in the same tab replaces the old one.
    assert!(registry.set_agent_session("ws-up", "t2", "c-2"));
    assert_eq!(registry.known_sessions()[0].session_id, "c-2");
}

#[test]
fn a_session_id_survives_a_reload_and_is_not_in_debug_output() {
    let dir = temp("sessions");
    let path = dir.join("workspaces.json");
    let registry = Registry::load(path.clone());
    registry.state.lock().unwrap().workspaces = vec![workspace(
        "ws-1",
        "/w",
        vec![tab("t1", "claude", "", Some("@1"))],
    )];
    registry.set_agent_session("ws-1", "t1", "secret-id-123");
    let again = Registry::load(path);
    let known = again.known_sessions();
    assert_eq!(known[0].session_id, "secret-id-123");
    assert!(!format!("{known:?}").contains("secret-id-123"));
}

#[tokio::test]
async fn reopening_a_dormant_workspace_resumes_the_agents_that_can_and_says_so_per_tab() {
    let tmp = temp("reopen");
    std::fs::create_dir_all(&tmp).unwrap();
    let root = tmp.to_string_lossy().into_owned();
    let registry = Registry::memory();
    registry.state.lock().unwrap().workspaces = vec![workspace(
        "ws-down",
        &root,
        vec![
            tab("t1", "shell", "", None),
            tab("t2", "claude", "", None),
            tab("t3", "codex", "", None),
            tab("t4", "opencode", "", None),
            tab("t5", "gemini", "", None),
            tab("t6", "claude", "", None),
        ],
    )];
    assert!(registry.set_agent_session("ws-down", "t2", "c-1"));
    assert!(registry.set_agent_session("ws-down", "t3", "x-1"));
    assert!(registry.set_agent_session("ws-down", "t5", "g-1"));
    // A token that is not plain is never put on a command line.
    assert!(registry.set_agent_session("ws-down", "t4", "a b; rm -rf /"));
    let backend = Fake::new();
    let record = registry.record("ws-down").unwrap();
    let said = registry
        .ensure_windows(&backend, &record, true)
        .await
        .unwrap();
    let got: Vec<(&str, &str, Option<&str>)> = said
        .iter()
        .map(|r| (r.tab.as_str(), r.outcome.as_str(), r.reason.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("t2", "resumed", None),
            ("t3", "resumed", None),
            (
                "t4",
                "shell",
                Some("this agent cannot resume a conversation")
            ),
            (
                "t5",
                "shell",
                Some("this agent cannot resume a conversation")
            ),
            (
                "t6",
                "shell",
                Some("no conversation was recorded for this tab")
            ),
        ]
    );
    let specs = backend.specs.lock().unwrap().clone();
    assert_eq!(
        specs,
        [
            ("shell".to_string(), None),
            (
                "claude".to_string(),
                Some("claude --resume c-1".to_string())
            ),
            ("codex".to_string(), Some("codex resume x-1".to_string())),
            ("shell".to_string(), None),
            ("shell".to_string(), None),
            ("shell".to_string(), None),
        ]
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn a_peek_or_a_new_tab_never_resumes_an_agent() {
    let tmp = temp("noreopen");
    std::fs::create_dir_all(&tmp).unwrap();
    let root = tmp.to_string_lossy().into_owned();
    let registry = Registry::memory();
    registry.state.lock().unwrap().workspaces = vec![workspace(
        "ws-down",
        &root,
        vec![tab("t1", "claude", "", None)],
    )];
    assert!(registry.set_agent_session("ws-down", "t1", "c-1"));
    let backend = Fake::new();
    let record = registry.record("ws-down").unwrap();
    let said = registry
        .ensure_windows(&backend, &record, false)
        .await
        .unwrap();
    assert!(said.is_empty());
    assert_eq!(
        backend.specs.lock().unwrap().clone(),
        [("shell".to_string(), None)]
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
