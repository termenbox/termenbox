//! Termenbox's own tmux server (DECISION-0120), as the real binary sees it: a first start gets the private server
//! and writes its config, workspaces already on the default socket do not make it use that socket (decision 0121),
//! and `termenbox-server tmux` reaches the same server. Every child gets its own TMUX_TMPDIR, so the owner's own tmux
//! and termenbox service are never touched.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn have_tmux() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tm-sock-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("tmp")).unwrap();
    d
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(d: &Path) -> Server {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_termenbox-server"));
    cmd.env("TERMENBOX_CONFIG_DIR", d.join("cfg"))
        .env("TMUX_TMPDIR", d.join("tmp"))
        .env("HOME", d)
        .env("TERMENBOX_HOST", "127.0.0.1")
        .env("TERMENBOX_PORT", "0")
        .env("TERMENBOX_DIRECTORY_URL", "")
        .env("TERMENBOX_WEBRTC", "off")
        .env_remove("TERMENBOX_TMUX_SOCKET")
        .env_remove("TMUX")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Server(cmd.spawn().unwrap())
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The owner's tmux: the default socket in this test's private directory.
fn plain_tmux(d: &Path, args: &[&str]) -> std::process::Output {
    Command::new("tmux")
        .env("TMUX_TMPDIR", d.join("tmp"))
        .env("HOME", d)
        .env_remove("TMUX")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn a_first_start_gets_the_private_server_and_its_config() {
    if !have_tmux() {
        return;
    }
    let d = dir("fresh");
    let _server = start(&d);
    let cfg = d.join("cfg");
    wait_for(&cfg.join("tmux.conf"));
    wait_for(&cfg.join("tmux_socket"));
    assert_eq!(
        std::fs::read_to_string(cfg.join("tmux_socket")).unwrap(),
        "termenbox"
    );
    let conf = std::fs::read_to_string(cfg.join("tmux.conf")).unwrap();
    assert!(
        conf.contains("default-terminal") && conf.contains("tmux.local.conf"),
        "{conf}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn workspaces_already_on_the_default_socket_do_not_make_the_install_use_it() {
    if !have_tmux() {
        return;
    }
    let d = dir("legacy");
    assert!(plain_tmux(&d, &["new-session", "-d", "-s", "mine"])
        .status
        .success());
    let _server = start(&d);
    let cfg = d.join("cfg");
    wait_for(&cfg.join("tmux_socket"));
    assert_eq!(
        std::fs::read_to_string(cfg.join("tmux_socket")).unwrap(),
        "termenbox"
    );
    wait_for(&cfg.join("tmux.conf"));
    let _ = plain_tmux(&d, &["kill-server"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_earlier_saved_default_is_moved_to_the_private_server() {
    if !have_tmux() {
        return;
    }
    let d = dir("saved");
    let cfg = d.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("tmux_socket"), "default").unwrap();
    let _server = start(&d);
    wait_for(&cfg.join("tmux.conf"));
    assert_eq!(
        std::fs::read_to_string(cfg.join("tmux_socket")).unwrap(),
        "termenbox"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_passthrough_reaches_the_private_server_and_not_the_users() {
    if !have_tmux() {
        return;
    }
    let d = dir("pass");
    let cfg = d.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("tmux_socket"), "termenbox").unwrap();
    std::fs::write(cfg.join("tmux.conf"), "set -g history-limit 1234\n").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_termenbox-server"))
            .arg("tmux")
            .args(args)
            .env("TERMENBOX_CONFIG_DIR", &cfg)
            .env("TERMENBOX_RUN", "container")
            .env("TMUX_TMPDIR", d.join("tmp"))
            .env("HOME", &d)
            .env_remove("TMUX")
            .env_remove("TERMENBOX_TMUX_SOCKET")
            .output()
            .unwrap()
    };
    assert!(run(&["new-session", "-d", "-s", "viapass"])
        .status
        .success());
    let ls = run(&["list-sessions", "-F", "#{session_name}"]);
    assert_eq!(String::from_utf8_lossy(&ls.stdout).trim(), "viapass");
    let limit = run(&["show-options", "-gv", "history-limit"]);
    assert_eq!(String::from_utf8_lossy(&limit.stdout).trim(), "1234");
    assert!(
        !plain_tmux(&d, &["list-sessions"]).status.success(),
        "the user's own tmux must not see it"
    );
    let _ = run(&["kill-server"]);
    let _ = std::fs::remove_dir_all(&d);
}
