//! The served uninstaller must undo every host change Termenbox can make: Claude Code's settings and OpenCode's
//! plugin. The user-scope branch is exercised in a temporary HOME with stubbed host tools, so nothing real is
//! touched.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_exe(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tm-uninstall-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn uninstall_removes_the_opencode_plugin_too() {
    let d = dir("opencode");
    let home = d.join("home");
    let stubs = d.join("stubs");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&stubs).unwrap();
    write_exe(
        &home.join(".local/bin/termenbox-server"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/calls.log\"\nexit 0\n",
    );
    write_exe(&stubs.join("systemctl"), "#!/bin/sh\nexit 0\n");
    // The script's user branch is the one that owns the file; a stub keeps the test independent of the uid it
    // runs under.
    write_exe(
        &stubs.join("id"),
        "#!/bin/sh\ncase \"$1\" in -u) echo 1000 ;; -un) echo tester ;; *) exec /usr/bin/id \"$@\" ;; esac\n",
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/public/uninstall.sh");
    let out = Command::new("sh")
        .arg(&script)
        .arg("--yes")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = std::fs::read_to_string(home.join("calls.log")).unwrap_or_default();
    assert!(calls.contains("--disconnect-claude"), "calls: {calls}");
    assert!(
        calls.contains("--disconnect-opencode"),
        "the uninstall left the OpenCode plugin behind; calls: {calls}"
    );
    let _ = std::fs::remove_dir_all(&d);
}
