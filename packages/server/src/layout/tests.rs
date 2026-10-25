use super::*;
use crate::control::WindowState;
use crate::pty::Attachment;
use std::sync::Mutex;
use termenbox_core::protocol::{WindowInfo, WorkspaceInfo};

fn terminal(id: &str) -> Window {
    Window {
        id: id.into(),
        kind: WindowKind::Terminal,
        host: "tmux".into(),
        name: id.into(),
        session_id: None,
    }
}

#[test]
fn a_layout_without_windows_saved_is_one_agent_window_with_the_focus() {
    let l = Layout::default();
    assert_eq!(l.windows.len(), 1);
    assert_eq!(l.focused(), Some(AGENT_WINDOW));
    let e = l.entries(Some("acp-1"));
    assert_eq!(e[0].session_id.as_deref(), Some("acp-1"));
    assert!(e[0].focused);
    // State an older server wrote has no layout field at all.
    let old: Layout = serde_json::from_str(r#"{"windows":[]}"#).unwrap();
    assert!(old.focused().is_none());
}

#[test]
fn the_focus_moves_falls_back_and_refuses_a_stranger() {
    let mut l = Layout::default();
    l.add(terminal("t1")).unwrap();
    assert!(l.add(terminal("t1")).is_err());
    l.set_focus("t1").unwrap();
    assert_eq!(l.focused(), Some("t1"));
    assert!(l.set_focus("nope").is_err());
    l.remove("t1").unwrap();
    assert_eq!(l.focused(), Some(AGENT_WINDOW));
    assert!(l.remove("t1").is_err());
    // A pointer at a window that is gone reads as the base agent window.
    l.focus = Some("gone".into());
    assert_eq!(l.focused(), Some(AGENT_WINDOW));
}

#[test]
fn a_layout_round_trips_through_json() {
    let mut l = Layout::default();
    l.add(terminal("t1")).unwrap();
    l.set_focus("t1").unwrap();
    let back: Layout = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
    assert_eq!(back, l);
}

/// A workspace manager with windows, in memory.
#[derive(Default)]
struct FakeManager {
    windows: Mutex<Vec<(u32, String, Option<String>, bool)>>,
}

#[async_trait]
impl WorkspaceBackend for FakeManager {
    fn name(&self) -> &str {
        "fake"
    }
    async fn list(&self) -> Result<Vec<WorkspaceInfo>> {
        Ok(vec![])
    }
    async fn create(&self, _name: &str) -> Result<()> {
        Ok(())
    }
    fn attach(&self, _id: &str, _cols: u16, _rows: u16) -> Result<Attachment> {
        Err(anyhow!("no"))
    }
    async fn windows(&self, _id: &str) -> Result<Vec<WindowState>> {
        Ok(self
            .windows
            .lock()
            .unwrap()
            .iter()
            .map(|(index, name, agent, active)| WindowState {
                info: WindowInfo {
                    index: *index,
                    name: name.clone(),
                    agent: agent.clone(),
                    command: String::new(),
                    path: String::new(),
                    active: *active,
                },
                activity: 0,
            })
            .collect())
    }
    async fn select_window(&self, _id: &str, index: u32) -> Result<()> {
        for w in self.windows.lock().unwrap().iter_mut() {
            w.3 = w.0 == index;
        }
        Ok(())
    }
    async fn new_window(&self, _id: &str) -> Result<()> {
        let mut w = self.windows.lock().unwrap();
        let next = w.iter().map(|x| x.0).max().map_or(0, |m| m + 1);
        for x in w.iter_mut() {
            x.3 = false;
        }
        w.push((next, "bash".into(), None, true));
        Ok(())
    }
    async fn kill_window(&self, _id: &str, index: u32) -> Result<()> {
        self.windows.lock().unwrap().retain(|w| w.0 != index);
        Ok(())
    }
}

fn spec(kind: WindowKind) -> WindowSpec {
    WindowSpec {
        kind,
        name: "w".into(),
        dir: None,
        command: None,
    }
}

#[tokio::test]
async fn a_tmux_window_is_addressed_by_its_id() {
    let m = Arc::new(FakeManager::default());
    m.windows
        .lock()
        .unwrap()
        .push((0, "claude".into(), Some("claude".into()), true));
    let host = TmuxHost::new(m);
    let first = host.list("ws").await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].kind, WindowKind::Agent);

    let made = host.create("ws", spec(WindowKind::Terminal)).await.unwrap();
    assert_eq!(made.kind, WindowKind::Terminal);
    assert!(made.focused);
    host.focus("ws", &first[0].id).await.unwrap();
    let now = host.list("ws").await.unwrap();
    assert!(now.iter().find(|w| w.id == first[0].id).unwrap().focused);
    assert!(!now.iter().find(|w| w.id == made.id).unwrap().focused);
    host.resize("ws", &made.id, 80, 24).await.unwrap();
    host.close("ws", &made.id).await.unwrap();
    assert_eq!(host.list("ws").await.unwrap().len(), 1);

    for bad in ["9", "x", ""] {
        assert!(host.focus("ws", bad).await.is_err(), "{bad:?}");
        assert!(host.close("ws", bad).await.is_err());
        assert!(host.resize("ws", bad, 1, 1).await.is_err());
    }
    assert!(host.create("ws", spec(WindowKind::Agent)).await.is_err());
}
