//! The window model of a workspace (decision 0138, step 3.1): a workspace is a sandbox of windows, each an `agent`
//! or a `terminal`, with a focus pointer; the base agent window is the default focus. The layout is what a native
//! ACP workspace persists next to its saved ACP state; a tmux workspace keeps its windows in tmux itself.
//!
//! Two `WindowHost`s put the model behind the port of `ports.rs`: `TmuxHost` over any workspace manager that has
//! windows, `AcpHost` over the native ACP backend. A window is addressed by its id on both.
// The hosts are used by tests only until the control channel carries window create/close/focus (step 3.3).
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::acp::AcpBackend;
use crate::backend::WorkspaceBackend;
use crate::ports::{WindowEntry, WindowHost, WindowKind, WindowSpec};

/// The id of the base agent window of an ACP workspace.
pub const AGENT_WINDOW: &str = "agent";

/// A window of a workspace. `host` names what runs it (`acp` for the agent child, `tmux` for a terminal).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub id: String,
    pub kind: WindowKind,
    pub host: String,
    #[serde(default)]
    pub name: String,
    /// The ACP `sessionId` of an agent window, once it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// The windows of one workspace and which one has the focus. `None` focus means the base agent window.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub windows: Vec<Window>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
}

impl Default for Layout {
    /// What a workspace saved before windows existed loads as: one agent window.
    fn default() -> Self {
        Self {
            windows: vec![Window {
                id: AGENT_WINDOW.into(),
                kind: WindowKind::Agent,
                host: "acp".into(),
                name: "agent".into(),
                session_id: None,
            }],
            focus: None,
        }
    }
}

impl Layout {
    pub fn window(&self, id: &str) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// The focused window's id: the pointer if it names a window, else the base agent window, else the first.
    pub fn focused(&self) -> Option<&str> {
        self.focus
            .as_deref()
            .filter(|f| self.window(f).is_some())
            .or_else(|| {
                self.windows
                    .iter()
                    .find(|w| w.kind == WindowKind::Agent)
                    .or(self.windows.first())
                    .map(|w| w.id.as_str())
            })
    }

    pub fn add(&mut self, w: Window) -> Result<()> {
        if self.window(&w.id).is_some() {
            return Err(anyhow!("A window with this id exists"));
        }
        self.windows.push(w);
        Ok(())
    }

    pub fn set_focus(&mut self, id: &str) -> Result<()> {
        self.window(id).ok_or_else(|| anyhow!("Unknown window"))?;
        self.focus = Some(id.to_string());
        Ok(())
    }

    /// Removes a window; the focus falls back to the base agent window if it was on this one.
    pub fn remove(&mut self, id: &str) -> Result<Window> {
        let at = self
            .windows
            .iter()
            .position(|w| w.id == id)
            .ok_or_else(|| anyhow!("Unknown window"))?;
        let w = self.windows.remove(at);
        if self.focus.as_deref() == Some(id) {
            self.focus = None;
        }
        Ok(w)
    }

    /// The layout as the host reports it: the agent window carries the live `sessionId`.
    pub fn entries(&self, session_id: Option<&str>) -> Vec<WindowEntry> {
        let focused = self.focused();
        self.windows
            .iter()
            .map(|w| WindowEntry {
                id: w.id.clone(),
                kind: w.kind,
                name: w.name.clone(),
                focused: focused == Some(w.id.as_str()),
                session_id: match w.kind {
                    WindowKind::Agent => w
                        .session_id
                        .clone()
                        .or_else(|| session_id.filter(|s| !s.is_empty()).map(String::from)),
                    WindowKind::Terminal => None,
                },
            })
            .collect()
    }
}

/// Windows of a workspace manager that has them (tmux). The id is the window index. tmux keeps the windows and the
/// current one itself, so nothing is persisted here.
pub struct TmuxHost {
    backend: Arc<dyn WorkspaceBackend>,
}

impl TmuxHost {
    pub fn new(backend: Arc<dyn WorkspaceBackend>) -> Self {
        Self { backend }
    }

    async fn entries(&self, workspace: &str) -> Result<Vec<WindowEntry>> {
        Ok(self
            .backend
            .windows(workspace)
            .await?
            .into_iter()
            .map(|w| WindowEntry {
                id: w.info.index.to_string(),
                kind: if w.info.agent.is_some() {
                    WindowKind::Agent
                } else {
                    WindowKind::Terminal
                },
                name: w.info.name,
                focused: w.info.active,
                session_id: None,
            })
            .collect())
    }

    fn index(window: &str) -> Result<u32> {
        window.parse().map_err(|_| anyhow!("Unknown window"))
    }

    async fn known(&self, workspace: &str, window: &str) -> Result<u32> {
        let index = Self::index(window)?;
        let all = self.entries(workspace).await?;
        all.iter()
            .any(|w| w.id == window)
            .then_some(index)
            .ok_or_else(|| anyhow!("Unknown window"))
    }
}

#[async_trait]
impl WindowHost for TmuxHost {
    async fn create(&self, workspace: &str, spec: WindowSpec) -> Result<WindowEntry> {
        if spec.kind == WindowKind::Agent {
            return Err(anyhow!(
                "An agent in a tmux window starts from its terminal (adapters, step 4.2)"
            ));
        }
        let before = self.entries(workspace).await?;
        self.backend.new_window(workspace).await?;
        // The new window is the current one and the one that was not there before.
        let after = self.entries(workspace).await?;
        after
            .into_iter()
            .find(|w| !before.iter().any(|b| b.id == w.id))
            .ok_or_else(|| anyhow!("The window was not created"))
    }

    async fn list(&self, workspace: &str) -> Result<Vec<WindowEntry>> {
        self.entries(workspace).await
    }

    async fn focus(&self, workspace: &str, window: &str) -> Result<()> {
        let index = self.known(workspace, window).await?;
        self.backend.select_window(workspace, index).await
    }

    async fn close(&self, workspace: &str, window: &str) -> Result<()> {
        let index = self.known(workspace, window).await?;
        self.backend.kill_window(workspace, index).await
    }

    /// A tmux window takes the size of the client attached to it; there is nothing to set from here.
    async fn resize(&self, workspace: &str, window: &str, _cols: u16, _rows: u16) -> Result<()> {
        self.known(workspace, window).await.map(|_| ())
    }
}

/// Windows of a native ACP workspace: the base agent window is the child process (no pane); terminal windows are
/// made by `terminals` (tmux) and recorded in the persisted layout.
pub struct AcpHost {
    backend: Arc<AcpBackend>,
    terminals: Option<Arc<dyn WindowHost>>,
}

impl AcpHost {
    pub fn new(backend: Arc<AcpBackend>) -> Self {
        Self {
            backend,
            terminals: None,
        }
    }

    pub fn with_terminals(mut self, terminals: Arc<dyn WindowHost>) -> Self {
        self.terminals = Some(terminals);
        self
    }

    fn terminals(&self) -> Result<&Arc<dyn WindowHost>> {
        self.terminals
            .as_ref()
            .ok_or_else(|| anyhow!("This server has no terminal windows for ACP workspaces"))
    }
}

#[async_trait]
impl WindowHost for AcpHost {
    async fn create(&self, workspace: &str, spec: WindowSpec) -> Result<WindowEntry> {
        if spec.kind == WindowKind::Agent {
            return Err(anyhow!(
                "A workspace has one agent window until several are supported (step 6.1)"
            ));
        }
        let name = spec.name.clone();
        let made = self.terminals()?.create(workspace, spec).await?;
        let window = Window {
            id: made.id.clone(),
            kind: WindowKind::Terminal,
            host: "tmux".into(),
            name,
            session_id: None,
        };
        if let Err(e) = self.backend.edit_layout(workspace, |l| l.add(window)) {
            let _ = self.terminals()?.close(workspace, &made.id).await;
            return Err(e);
        }
        Ok(made)
    }

    async fn list(&self, workspace: &str) -> Result<Vec<WindowEntry>> {
        let (layout, session) = self.backend.layout(workspace)?;
        Ok(layout.entries(session.as_deref()))
    }

    async fn focus(&self, workspace: &str, window: &str) -> Result<()> {
        self.backend.edit_layout(workspace, |l| l.set_focus(window))
    }

    async fn close(&self, workspace: &str, window: &str) -> Result<()> {
        let (layout, _) = self.backend.layout(workspace)?;
        match layout.window(window).map(|w| w.kind) {
            None => Err(anyhow!("Unknown window")),
            Some(WindowKind::Agent) => Err(anyhow!("Close the workspace to end its agent")),
            Some(WindowKind::Terminal) => {
                self.terminals()?.close(workspace, window).await?;
                self.backend
                    .edit_layout(workspace, |l| l.remove(window).map(|_| ()))
            }
        }
    }

    async fn resize(&self, workspace: &str, window: &str, cols: u16, rows: u16) -> Result<()> {
        let (layout, _) = self.backend.layout(workspace)?;
        match layout.window(window).map(|w| w.kind) {
            None => Err(anyhow!("Unknown window")),
            // The agent child has no pane.
            Some(WindowKind::Agent) => Ok(()),
            Some(WindowKind::Terminal) => {
                self.terminals()?
                    .resize(workspace, window, cols, rows)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests;
