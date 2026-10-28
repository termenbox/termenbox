use super::*;
use crate::backend::{Runner, TmuxBackend};

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
        }],
        next_tab: 2,
    };
    let id = "t2".to_string();
    record.next_tab = 3;
    record.tabs.push(TabRecord {
        id,
        label: unique_label("Shell", &record.tabs),
        kind: "shell".into(),
        dir: String::new(),
        order: 1,
        window: Some("@2".into()),
    });
    assert_eq!(record.tabs[1].label, "Shell 2");
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
    registry.move_tab(&id, &second.id, 0).await.unwrap();
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
