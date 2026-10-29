//! The server-owned record of workspaces and their tabs (decision 0147). tmux keeps the processes; this module keeps
//! what every device must agree on: which tabs exist, their labels, their order and the tab a new device opens.
//!
//! The record is authoritative for tmux and command workspaces. ACP workspaces stay in the ACP backend's own state and
//! are presented as a legacy workspace with one agent tab; nothing here duplicates their conversation state.
//!
//! Every write is atomic (a temporary file renamed over the old one). A record whose tmux server is gone stays dormant:
//! opening it recreates the session and its tabs as shells, with the same labels and order, rather than deleting what
//! the owner arranged.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use termenbox_core::protocol::{Label, TabInfo, WorkspaceInfo};

use crate::backend::{
    clean_label, tab_label, valid_dir, valid_template, CreatedWorkspace, TabSpec, WorkspaceBackend,
};
use crate::control::WindowState;
use crate::pty::Attachment;

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TabRecord {
    pub id: String,
    pub label: Label,
    /// The template it started from (`shell`, `claude`, ...).
    pub kind: String,
    /// A directory inside the workspace, relative to it; empty means the workspace directory.
    #[serde(default)]
    pub dir: String,
    pub order: u32,
    /// The tmux window it runs in; absent while the workspace is dormant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRecord {
    /// The wire id: a minted `ws-…`, or an older workspace's name, prefixed by the backend in a composite server.
    pub id: String,
    pub label: Label,
    pub dir: String,
    /// Set only when the server runs more than one backend (`tmux`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    pub order: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default)]
    pub tabs: Vec<TabRecord>,
    #[serde(default = "one")]
    pub next_tab: u32,
}

fn one() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct State {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    workspaces: Vec<WorkspaceRecord>,
    /// Every workspace the server knows, including the ones with no record (ACP and command managers). The order is
    /// the owner's order and is the same for every device.
    #[serde(default)]
    order: Vec<String>,
}

pub struct Registry {
    path: PathBuf,
    state: Mutex<State>,
}

impl Registry {
    /// Loads the file; a missing or unreadable one starts empty and is migrated from the backends on the first list.
    pub fn load(path: PathBuf) -> Self {
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<State>(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            state: Mutex::new(state),
        }
    }

    /// A registry that never writes, for tests that only exercise the in-memory rules.
    pub fn memory() -> Self {
        Self {
            path: PathBuf::new(),
            state: Mutex::new(State::default()),
        }
    }

    fn save(&self, state: &State) {
        if self.path.as_os_str().is_empty() {
            return;
        }
        let Ok(text) = serde_json::to_string_pretty(state) else {
            return;
        };
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_err() {
            return;
        }
        let _ = std::fs::rename(&tmp, &self.path);
    }

    /// Lists every workspace with its tabs and brings the registry in line with the backends: a tmux session the owner
    /// made outside termenbox is adopted, a window that is gone loses its window id, a record whose session is gone
    /// stays dormant.
    pub async fn list(&self, backend: &dyn WorkspaceBackend) -> Result<Vec<WorkspaceInfo>> {
        let infos = backend.list().await?;
        let kinds = backend.kinds();
        let mut windows: HashMap<String, Vec<WindowState>> = HashMap::new();
        for info in &infos {
            if kind_of(&kinds, info) != "tmux" {
                continue;
            }
            if let Ok(w) = backend.windows(&info.id).await {
                windows.insert(info.id.clone(), w);
            }
        }

        let mut state = self.state.lock().unwrap();
        if state.version == 0 {
            state.version = VERSION;
        }
        // Adopt or refresh the tmux records.
        for info in &infos {
            if kind_of(&kinds, info) != "tmux" {
                continue;
            }
            let label = info.name.as_str().to_string();
            let Some(observed) = windows.get(&info.id).cloned() else {
                // A transient tmux error must not erase the windows a record already has.
                continue;
            };
            let at = match state.workspaces.iter().position(|r| r.id == info.id) {
                Some(at) => at,
                None => {
                    let record = WorkspaceRecord {
                        id: info.id.clone(),
                        label: label.clone().into(),
                        dir: dir_of(&observed),
                        backend: info.backend.clone(),
                        order: 0,
                        focus: None,
                        tabs: Vec::new(),
                        next_tab: 1,
                    };
                    state.workspaces.push(record);
                    state.workspaces.len() - 1
                }
            };
            reconcile_record(&mut state.workspaces[at], &observed, &label);
        }

        // A record with no backend entry is dormant: its windows are gone with the tmux server.
        let live: HashSet<String> = infos.iter().map(|i| i.id.clone()).collect();
        for record in &mut state.workspaces {
            if !live.contains(&record.id) {
                for tab in &mut record.tabs {
                    tab.window = None;
                }
            }
        }
        let records: HashSet<String> = state.workspaces.iter().map(|r| r.id.clone()).collect();
        state
            .order
            .retain(|id| live.contains(id) || records.contains(id));
        for info in &infos {
            if !state.order.contains(&info.id) {
                state.order.push(info.id.clone());
            }
        }
        let positions: HashMap<String, u32> = state
            .order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i as u32))
            .collect();
        for record in &mut state.workspaces {
            record.order = positions.get(&record.id).copied().unwrap_or(record.order);
        }
        let order = state.order.clone();
        let records = state.workspaces.clone();
        self.save(&state);
        drop(state);

        let mut out: Vec<WorkspaceInfo> = Vec::new();
        let mut seen = HashSet::new();
        for info in &infos {
            seen.insert(info.id.clone());
            if kind_of(&kinds, info) == "tmux" {
                if let Some(record) = records.iter().find(|r| r.id == info.id) {
                    out.push(record_info(record, info.backend.clone(), &order));
                    continue;
                }
            }
            out.push(synthetic_info(info, &order));
        }
        // A dormant record has no backend entry any more; it stays until the owner closes it.
        for record in &records {
            if !seen.contains(&record.id) {
                out.push(record_info(record, record.backend.clone(), &order));
            }
        }
        out.sort_by(|a, b| a.order.cmp(&b.order).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    /// Creates a workspace and its first tab. A tmux workspace is recorded; ACP and command ones are not.
    pub async fn create_workspace(
        &self,
        backend: &dyn WorkspaceBackend,
        name: &str,
        backend_kind: Option<&str>,
        dir: Option<&str>,
        kind: &str,
    ) -> Result<String> {
        let kind = if kind.is_empty() { "shell" } else { kind };
        if !valid_template(kind) {
            return Err(anyhow!("Unknown agent template"));
        }
        let label = clean_label(name)?.ok_or_else(|| anyhow!("A workspace needs a name"))?;
        let root = dir
            .filter(|d| !d.is_empty())
            .map(str::to_string)
            .or_else(|| std::env::var("HOME").ok())
            .unwrap_or_else(|| "/".into());
        if !valid_dir(&root) {
            return Err(anyhow!("The directory must be an absolute path"));
        }
        let first = TabSpec {
            id: "t1".into(),
            label: tab_label(kind),
            kind: kind.into(),
            dir: root.clone(),
        };
        let kinds = backend.kinds();
        let chosen = backend_kind
            .map(str::to_string)
            .or_else(|| {
                // New workspaces are tmux whenever the server runs tmux (decision 0147), whatever the order the
                // owner configured the managers in.
                kinds
                    .iter()
                    .find(|k| *k == "tmux")
                    .cloned()
                    .or_else(|| kinds.first().cloned())
            })
            .unwrap_or_else(|| backend.name().to_string());
        let made: CreatedWorkspace = backend
            .create_workspace(
                &label,
                Some(chosen.as_str()),
                Some(&root),
                Some(kind),
                &first,
            )
            .await?;
        if chosen != "tmux" {
            return Ok(made.id);
        }
        let mut state = self.state.lock().unwrap();
        let record = WorkspaceRecord {
            id: made.id.clone(),
            label: label.into(),
            dir: root,
            backend: if kinds.len() > 1 {
                Some("tmux".into())
            } else {
                None
            },
            order: 0,
            focus: Some(first.id.clone()),
            tabs: vec![TabRecord {
                id: first.id,
                label: first.label.into(),
                kind: first.kind,
                dir: String::new(),
                order: 0,
                window: made.window,
            }],
            next_tab: 2,
        };
        state.workspaces.push(record);
        if !state.order.contains(&made.id) {
            state.order.push(made.id.clone());
        }
        self.save(&state);
        Ok(made.id)
    }

    /// Opens a tab and records it at the end. The first window of a dormant workspace is made by `ensure`.
    pub async fn new_tab(
        &self,
        backend: &dyn WorkspaceBackend,
        workspace: &str,
        kind: &str,
        label: &str,
        dir: &str,
    ) -> Result<TabRecord> {
        let kind = if kind.is_empty() { "shell" } else { kind };
        if !valid_template(kind) {
            return Err(anyhow!("Unknown agent template"));
        }
        let (record, root) = {
            let state = self.state.lock().unwrap();
            let record = state
                .workspaces
                .iter()
                .find(|r| r.id == workspace)
                .cloned()
                .ok_or_else(|| anyhow!("Unknown workspace"))?;
            let root = record.dir.clone();
            (record, root)
        };
        if record.tabs.iter().any(|t| t.window.is_none())
            || backend.windows(&record.id).await.is_err()
        {
            self.ensure_windows(backend, &record).await?;
        }
        let absolute = resolve_tab_dir(&root, dir)?;
        let (tab, window) = self
            .create_window(backend, &record, kind, label, &absolute, dir)
            .await?;
        // A client that creates a tab expects to land in it; an attaching client asks for it explicitly.
        let _ = backend.focus_tab(&record.id, &window).await;
        let mut state = self.state.lock().unwrap();
        let record = state
            .workspaces
            .iter_mut()
            .find(|r| r.id == workspace)
            .ok_or_else(|| anyhow!("Unknown workspace"))?;
        record.tabs.push(tab.clone());
        record.focus = Some(tab.id.clone());
        record.next_tab = record.next_tab.max(tab_number(&tab.id) + 1);
        renumber_tabs(record);
        self.save(&state);
        Ok(tab)
    }

    async fn create_window(
        &self,
        backend: &dyn WorkspaceBackend,
        record: &WorkspaceRecord,
        kind: &str,
        label: &str,
        absolute: &str,
        relative: &str,
    ) -> Result<(TabRecord, String)> {
        let id = self.free_tab_id(record);
        let label = clean_label(label)
            .ok()
            .flatten()
            .unwrap_or_else(|| tab_label(kind));
        let spec = TabSpec {
            id: id.clone(),
            label: unique_label(&label, &record.tabs),
            kind: kind.into(),
            dir: absolute.into(),
        };
        let window = backend.new_tab(&record.id, &spec).await?;
        Ok((
            TabRecord {
                id,
                label: spec.label.into(),
                kind: kind.into(),
                dir: relative.into(),
                order: record.tabs.len() as u32,
                window: Some(window.clone()),
            },
            window,
        ))
    }

    fn free_tab_id(&self, record: &WorkspaceRecord) -> String {
        let mut n = record.next_tab.max(1);
        loop {
            let id = format!("t{n}");
            if !record.tabs.iter().any(|t| t.id == id) {
                return id;
            }
            n += 1;
        }
    }

    pub async fn rename_tab(
        &self,
        backend: &dyn WorkspaceBackend,
        workspace: &str,
        tab: &str,
        label: &str,
    ) -> Result<()> {
        let label = clean_label(label)?.ok_or_else(|| anyhow!("A tab needs a name"))?;
        let (workspace_id, tmux_window) = {
            let state = self.state.lock().unwrap();
            let record = state
                .workspaces
                .iter()
                .find(|r| r.id == workspace)
                .ok_or_else(|| anyhow!("Unknown workspace"))?;
            let tab = record
                .tabs
                .iter()
                .find(|t| t.id == tab)
                .ok_or_else(|| anyhow!("Unknown tab"))?;
            (record.id.clone(), tab.window.clone())
        };
        if let Some(window) = &tmux_window {
            backend.rename_tab(&workspace_id, window, &label).await?;
        }
        let mut state = self.state.lock().unwrap();
        let record = state
            .workspaces
            .iter_mut()
            .find(|r| r.id == workspace)
            .ok_or_else(|| anyhow!("Unknown workspace"))?;
        if let Some(t) = record.tabs.iter_mut().find(|t| t.id == tab) {
            t.label = label.into();
        }
        self.save(&state);
        Ok(())
    }

    /// Deletes a tab. The last tab is replaced by a shell tab, so a workspace is never empty.
    pub async fn close_tab(
        &self,
        backend: &dyn WorkspaceBackend,
        workspace: &str,
        tab: &str,
    ) -> Result<()> {
        let (record, window) = {
            let state = self.state.lock().unwrap();
            let record = state
                .workspaces
                .iter()
                .find(|r| r.id == workspace)
                .cloned()
                .ok_or_else(|| anyhow!("Unknown workspace"))?;
            let window = record
                .tabs
                .iter()
                .find(|t| t.id == tab)
                .and_then(|t| t.window.clone());
            (record, window)
        };
        if let Some(window) = window {
            let _ = backend.close_tab(&record.id, &window).await;
        }
        {
            let mut state = self.state.lock().unwrap();
            if let Some(r) = state.workspaces.iter_mut().find(|r| r.id == workspace) {
                r.tabs.retain(|t| t.id != tab);
                if r.focus.as_deref() == Some(tab) {
                    r.focus = None;
                }
                renumber_tabs(r);
            }
            self.save(&state);
        }
        let left = {
            let state = self.state.lock().unwrap();
            state
                .workspaces
                .iter()
                .find(|r| r.id == workspace)
                .map(|r| r.tabs.len())
                .unwrap_or(0)
        };
        if left == 0 {
            // Closing the last window ended the tmux session; make the replacement shell tab in a fresh one.
            self.new_shell_tab(backend, workspace).await?;
        }
        Ok(())
    }

    async fn new_shell_tab(&self, backend: &dyn WorkspaceBackend, workspace: &str) -> Result<()> {
        let record = {
            let state = self.state.lock().unwrap();
            state
                .workspaces
                .iter()
                .find(|r| r.id == workspace)
                .cloned()
                .ok_or_else(|| anyhow!("Unknown workspace"))?
        };
        // The session is gone; recreate it with the first tab rather than opening a window in nothing.
        let id = self.free_tab_id(&record);
        let label = unique_label("Shell", &record.tabs);
        let spec = TabSpec {
            id: id.clone(),
            label: label.clone(),
            kind: "shell".into(),
            dir: record.dir.clone(),
        };
        let window = backend
            .ensure_session(&record.id, record.label.as_str(), &record.dir, &spec)
            .await?;
        let _ = backend.focus_tab(&record.id, &window).await;
        let mut state = self.state.lock().unwrap();
        let record = state
            .workspaces
            .iter_mut()
            .find(|r| r.id == workspace)
            .ok_or_else(|| anyhow!("Unknown workspace"))?;
        record.tabs.push(TabRecord {
            id,
            label: label.into(),
            kind: "shell".into(),
            dir: String::new(),
            order: 0,
            window: Some(window),
        });
        record.focus = record.tabs.last().map(|t| t.id.clone());
        record.next_tab = record.next_tab.max(record.tabs.len() as u32 + 1);
        renumber_tabs(record);
        self.save(&state);
        Ok(())
    }

    pub async fn move_tab(&self, workspace: &str, tab: &str, index: u32) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let record = state
            .workspaces
            .iter_mut()
            .find(|r| r.id == workspace)
            .ok_or_else(|| anyhow!("Unknown workspace"))?;
        let at = record
            .tabs
            .iter()
            .position(|t| t.id == tab)
            .ok_or_else(|| anyhow!("Unknown tab"))?;
        let moved = record.tabs.remove(at);
        let to = (index as usize).min(record.tabs.len());
        record.tabs.insert(to, moved);
        renumber_tabs(record);
        self.save(&state);
        Ok(())
    }

    pub async fn focus_tab(
        &self,
        backend: &dyn WorkspaceBackend,
        workspace: &str,
        tab: &str,
    ) -> Result<()> {
        let (window, id) = {
            let state = self.state.lock().unwrap();
            let record = state
                .workspaces
                .iter()
                .find(|r| r.id == workspace)
                .ok_or_else(|| anyhow!("Unknown workspace"))?;
            let t = record
                .tabs
                .iter()
                .find(|t| t.id == tab)
                .ok_or_else(|| anyhow!("Unknown tab"))?;
            (t.window.clone(), record.id.clone())
        };
        if let Some(window) = window {
            backend.focus_tab(&id, &window).await?;
        }
        let mut state = self.state.lock().unwrap();
        if let Some(record) = state.workspaces.iter_mut().find(|r| r.id == workspace) {
            record.focus = Some(tab.to_string());
        }
        self.save(&state);
        Ok(())
    }

    pub async fn move_workspace(&self, workspace: &str, index: u32) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let at = state
            .order
            .iter()
            .position(|id| id == workspace)
            .ok_or_else(|| anyhow!("Unknown workspace"))?;
        let moved = state.order.remove(at);
        let to = (index as usize).min(state.order.len());
        state.order.insert(to, moved);
        let positions: HashMap<String, u32> = state
            .order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i as u32))
            .collect();
        for record in &mut state.workspaces {
            record.order = positions.get(&record.id).copied().unwrap_or(record.order);
        }
        self.save(&state);
        Ok(())
    }

    /// Attaches to one tab's window, recreating a dormant session first.
    pub async fn attach(
        &self,
        backend: &dyn WorkspaceBackend,
        workspace: &str,
        tab: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<(String, Attachment)> {
        let (record, tab) = {
            let state = self.state.lock().unwrap();
            let record = state
                .workspaces
                .iter()
                .find(|r| r.id == workspace)
                .cloned()
                .ok_or_else(|| anyhow!("Unknown workspace"))?;
            let id = tab
                .map(str::to_string)
                .or_else(|| record.focus.clone())
                .or_else(|| record.tabs.first().map(|t| t.id.clone()))
                .ok_or_else(|| anyhow!("This workspace has no tabs"))?;
            (record, id)
        };
        self.ensure_windows(backend, &record).await?;
        let record = self
            .state
            .lock()
            .unwrap()
            .workspaces
            .iter()
            .find(|r| r.id == workspace)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown workspace"))?;
        let window = self.window_for_tab(backend, &record, &tab).await?;
        let attachment = backend.attach_tab(&record.id, &window, cols, rows).await?;
        {
            let mut state = self.state.lock().unwrap();
            if let Some(record) = state.workspaces.iter_mut().find(|r| r.id == workspace) {
                record.focus = Some(tab.clone());
            }
            self.save(&state);
        }
        Ok((tab, attachment))
    }

    /// Recreates the tmux session and every tab as a shell when it is dormant (decision 0147).
    async fn ensure_windows(
        &self,
        backend: &dyn WorkspaceBackend,
        record: &WorkspaceRecord,
    ) -> Result<()> {
        if backend.windows(&record.id).await.is_ok() {
            // The session is alive; a tab whose window was closed outside is recreated alone, by window_for_tab.
            return Ok(());
        }
        if record.tabs.is_empty() {
            let id = format!("t{}", record.next_tab.max(1));
            let spec = TabSpec {
                id: id.clone(),
                label: "Shell".into(),
                kind: "shell".into(),
                dir: record.dir.clone(),
            };
            let window = backend
                .ensure_session(&record.id, record.label.as_str(), &record.dir, &spec)
                .await?;
            let mut state = self.state.lock().unwrap();
            if let Some(r) = state.workspaces.iter_mut().find(|r| r.id == record.id) {
                r.tabs.push(TabRecord {
                    id: id.clone(),
                    label: "Shell".into(),
                    kind: "shell".into(),
                    dir: String::new(),
                    order: 0,
                    window: Some(window),
                });
                r.focus = Some(id);
                renumber_tabs(r);
                self.save(&state);
            }
            return Ok(());
        }
        let mut windows = HashMap::new();
        for (i, tab) in record.tabs.iter().enumerate() {
            let spec = TabSpec {
                id: tab.id.clone(),
                label: tab.label.as_str().to_string(),
                kind: "shell".into(),
                dir: resolve_tab_dir(&record.dir, &tab.dir)?,
            };
            let window = if i == 0 {
                backend
                    .ensure_session(&record.id, record.label.as_str(), &record.dir, &spec)
                    .await?
            } else {
                backend.new_tab(&record.id, &spec).await?
            };
            windows.insert(tab.id.clone(), window);
        }
        let mut state = self.state.lock().unwrap();
        if let Some(record) = state.workspaces.iter_mut().find(|r| r.id == record.id) {
            for tab in &mut record.tabs {
                if let Some(window) = windows.get(&tab.id) {
                    tab.window = Some(window.clone());
                }
            }
            self.save(&state);
        }
        Ok(())
    }

    /// The window of a tab, for a peek or a prompt answer.
    pub async fn tab_window(
        &self,
        backend: &dyn WorkspaceBackend,
        workspace: &str,
        tab: Option<&str>,
    ) -> Result<Option<String>> {
        let (record, tab) = {
            let state = self.state.lock().unwrap();
            let record = state
                .workspaces
                .iter()
                .find(|r| r.id == workspace)
                .cloned()
                .ok_or_else(|| anyhow!("Unknown workspace"))?;
            let id = tab
                .map(str::to_string)
                .or_else(|| record.focus.clone())
                .or_else(|| record.tabs.first().map(|t| t.id.clone()));
            (record, id)
        };
        if backend.windows(&record.id).await.is_err() {
            self.ensure_windows(backend, &record).await?;
        }
        let Some(tab) = tab else {
            return Ok(None);
        };
        let record = {
            let state = self.state.lock().unwrap();
            match state.workspaces.iter().find(|r| r.id == workspace) {
                Some(record) => record.clone(),
                None => return Ok(None),
            }
        };
        let Some(known) = record.tabs.iter().find(|t| t.id == tab) else {
            return Ok(None);
        };
        if known.window.is_some() {
            return Ok(known.window.clone());
        }
        Ok(Some(self.window_for_tab(backend, &record, &tab).await?))
    }

    /// A tab whose window was closed outside termenbox is recreated as a shell in its own directory, alone.
    async fn window_for_tab(
        &self,
        backend: &dyn WorkspaceBackend,
        record: &WorkspaceRecord,
        tab: &str,
    ) -> Result<String> {
        let known = record
            .tabs
            .iter()
            .find(|t| t.id == tab)
            .ok_or_else(|| anyhow!("Unknown tab"))?;
        if let Some(window) = &known.window {
            return Ok(window.clone());
        }
        let spec = TabSpec {
            id: known.id.clone(),
            label: known.label.as_str().to_string(),
            kind: "shell".into(),
            dir: resolve_tab_dir(&record.dir, &known.dir)?,
        };
        let window = backend.new_tab(&record.id, &spec).await?;
        let mut state = self.state.lock().unwrap();
        if let Some(r) = state.workspaces.iter_mut().find(|r| r.id == record.id) {
            if let Some(t) = r.tabs.iter_mut().find(|t| t.id == tab) {
                t.window = Some(window.clone());
            }
            self.save(&state);
        }
        Ok(window)
    }

    pub fn tabs(&self, workspace: &str) -> Vec<TabRecord> {
        self.state
            .lock()
            .unwrap()
            .workspaces
            .iter()
            .find(|r| r.id == workspace)
            .map(|r| {
                let mut tabs = r.tabs.clone();
                tabs.sort_by_key(|t| t.order);
                tabs
            })
            .unwrap_or_default()
    }

    pub fn has(&self, workspace: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .workspaces
            .iter()
            .any(|r| r.id == workspace)
    }

    /// Renames a workspace: the record for tmux and command, the backend for ACP.
    pub async fn rename_workspace(
        &self,
        backend: &dyn WorkspaceBackend,
        workspace: &str,
        name: &str,
    ) -> Result<()> {
        let label = clean_label(name)?.ok_or_else(|| anyhow!("A workspace needs a name"))?;
        if self.has(workspace) {
            {
                let mut state = self.state.lock().unwrap();
                if let Some(record) = state.workspaces.iter_mut().find(|r| r.id == workspace) {
                    record.label = label.into();
                }
                self.save(&state);
            }
            // Keep tmux's own label in step for an older client and for tmux itself.
            let _ = backend.rename(workspace, name).await;
            return Ok(());
        }
        backend.rename(workspace, name).await
    }

    /// Closes a workspace: kills every tab and forgets the record. A session that is already gone is not an error.
    pub async fn close_workspace(
        &self,
        backend: &dyn WorkspaceBackend,
        workspace: &str,
    ) -> Result<()> {
        if self.has(workspace) {
            let _ = backend.kill(workspace).await;
            let mut state = self.state.lock().unwrap();
            state.workspaces.retain(|r| r.id != workspace);
            state.order.retain(|id| id != workspace);
            self.save(&state);
            return Ok(());
        }
        backend.kill(workspace).await
    }

    /// The record for one workspace, if it is a tmux one.
    pub fn record(&self, workspace: &str) -> Option<WorkspaceRecord> {
        self.state
            .lock()
            .unwrap()
            .workspaces
            .iter()
            .find(|r| r.id == workspace)
            .cloned()
    }
}

fn kind_of(kinds: &[String], info: &WorkspaceInfo) -> String {
    info.backend
        .clone()
        .or_else(|| kinds.first().cloned())
        .unwrap_or_default()
}

fn record_info(
    record: &WorkspaceRecord,
    backend: Option<String>,
    order: &[String],
) -> WorkspaceInfo {
    let agent = record
        .focus
        .as_deref()
        .and_then(|f| record.tabs.iter().find(|t| t.id == f))
        .filter(|t| t.kind != "shell")
        .map(|t| t.kind.clone())
        .or_else(|| {
            record
                .tabs
                .iter()
                .find(|t| t.kind != "shell")
                .map(|t| t.kind.clone())
        });
    let mut tabs: Vec<TabInfo> = record
        .tabs
        .iter()
        .map(|t| TabInfo {
            id: t.id.clone(),
            label: t.label.clone(),
            kind: t.kind.clone(),
            dir: t.dir.clone(),
            order: t.order,
            window: t.window.clone(),
        })
        .collect();
    tabs.sort_by_key(|t| t.order);
    WorkspaceInfo {
        id: record.id.clone(),
        name: record.label.clone(),
        agent,
        backend,
        dir: record.dir.clone(),
        order: order
            .iter()
            .position(|id| id == &record.id)
            .map(|i| i as u32)
            .unwrap_or(record.order),
        focus: record.focus.clone(),
        tabs,
    }
}

fn synthetic_info(info: &WorkspaceInfo, order: &[String]) -> WorkspaceInfo {
    let kind = info.agent.clone().unwrap_or_else(|| "shell".into());
    let tab = TabInfo {
        id: "t1".into(),
        label: tab_label(&kind).into(),
        kind,
        dir: String::new(),
        order: 0,
        window: None,
    };
    WorkspaceInfo {
        id: info.id.clone(),
        name: info.name.clone(),
        agent: info.agent.clone(),
        backend: info.backend.clone(),
        dir: info.dir.clone(),
        order: order
            .iter()
            .position(|id| id == &info.id)
            .map(|i| i as u32)
            .unwrap_or(info.order),
        focus: Some(tab.id.clone()),
        tabs: vec![tab],
    }
}

fn reconcile_record(record: &mut WorkspaceRecord, windows: &[WindowState], label: &str) {
    if !label.is_empty() {
        record.label = label.into();
    }
    if record.dir.is_empty() {
        record.dir = dir_of(windows);
    }
    let seen: HashSet<String> = windows.iter().map(|w| w.window_id.clone()).collect();
    for tab in &mut record.tabs {
        if tab.window.as_ref().is_some_and(|w| !seen.contains(w)) {
            tab.window = None;
        }
    }
    for window in windows {
        let matched = record.tabs.iter_mut().find(|t| {
            (!window.tab.is_empty() && t.id == window.tab)
                || t.window.as_deref() == Some(window.window_id.as_str())
        });
        if let Some(tab) = matched {
            tab.window = Some(window.window_id.clone());
            if !window.info.name.is_empty() {
                tab.label = window.info.name.clone().into();
            }
            continue;
        }
        let id = format!("t{}", record.next_tab.max(1));
        record.next_tab += 1;
        let kind = window
            .info
            .agent
            .clone()
            .unwrap_or_else(|| "shell".to_string());
        record.tabs.push(TabRecord {
            id,
            label: window.info.name.clone().into(),
            kind,
            dir: relative_to(&record.dir, &window.info.path),
            order: record.tabs.len() as u32,
            window: Some(window.window_id.clone()),
        });
    }
    if record.focus.is_none()
        || !record
            .tabs
            .iter()
            .any(|t| Some(&t.id) == record.focus.as_ref())
    {
        record.focus = record.tabs.first().map(|t| t.id.clone());
    }
    renumber_tabs(record);
    record.next_tab = record.next_tab.max(
        record
            .tabs
            .iter()
            .map(|t| tab_number(&t.id))
            .max()
            .unwrap_or(0)
            + 1,
    );
}

fn renumber_tabs(record: &mut WorkspaceRecord) {
    for (i, tab) in record.tabs.iter_mut().enumerate() {
        tab.order = i as u32;
    }
}

fn tab_number(id: &str) -> u32 {
    id.strip_prefix('t')
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn unique_label(base: &str, tabs: &[TabRecord]) -> String {
    if !tabs.iter().any(|t| t.label.as_str() == base) {
        return base.to_string();
    }
    for n in 2..1000 {
        let candidate = format!("{base} {n}");
        if !tabs.iter().any(|t| t.label.as_str() == candidate) {
            return candidate;
        }
    }
    base.to_string()
}

fn dir_of(windows: &[WindowState]) -> String {
    windows
        .iter()
        .find(|w| w.info.active)
        .or_else(|| windows.first())
        .map(|w| w.info.path.clone())
        .filter(|p| valid_dir(p))
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

fn relative_to(root: &str, path: &str) -> String {
    let Ok(root) = Path::new(root).canonicalize() else {
        return String::new();
    };
    let Ok(path) = Path::new(path).canonicalize() else {
        return String::new();
    };
    path.strip_prefix(&root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Resolves a tab's directory inside its workspace: a relative path with no `..`, canonicalised so a symlink cannot
/// leave the workspace. An empty path is the workspace directory.
pub fn resolve_tab_dir(root: &str, relative: &str) -> Result<String> {
    let root_path = Path::new(root)
        .canonicalize()
        .map_err(|_| anyhow!("The workspace directory does not exist"))?;
    if relative.trim().is_empty() {
        return Ok(root_path.to_string_lossy().to_string());
    }
    let rel = Path::new(relative);
    if rel.is_absolute()
        || rel.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!("Use a directory inside the workspace"));
    }
    let path = root_path
        .join(rel)
        .canonicalize()
        .map_err(|_| anyhow!("That directory does not exist"))?;
    if !path.starts_with(&root_path) || !path.is_dir() {
        return Err(anyhow!("That directory is outside the workspace"));
    }
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests;
