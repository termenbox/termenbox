//! The served installer ends every run with the pairing block (decision 0126), whatever the secret was and whether the
//! server is already owned. The container branch runs against stubbed host tools in a temporary directory, so nothing
//! real is touched; the stubbed `termenbox-server` prints what the real one would.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const LINK: &str = "https://app.test/#/pair/PAYLOAD?claim=TOKEN";
const OWNED_LINK: &str = "https://app.test/#/pair/PAYLOAD";

fn write_exe(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tm-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs the container install with `secret` and a stub server whose `--register` prints `register`; returns stdout and
/// the log of `docker exec` calls.
fn run(name: &str, secret: &str, register: &str) -> (String, String) {
    let d = dir(name);
    let stubs = d.join("stubs");
    std::fs::write(d.join("register.txt"), register).unwrap();
    write_exe(
        &stubs.join("docker"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  exec) shift 2; echo \"$*\" >> {log}\n    case \"$*\" in *--register*) cat {reg} ;; esac ;;\n  *) exit 0 ;;\nesac\n",
            log = d.join("calls.log").display(),
            reg = d.join("register.txt").display()
        ),
    );
    for tool in ["chown", "systemctl", "ufw", "sleep"] {
        write_exe(&stubs.join(tool), "#!/bin/sh\nexit 0\n");
    }
    write_exe(&stubs.join("curl"), "#!/bin/sh\nexit 22\n");
    write_exe(
        &stubs.join("id"),
        "#!/bin/sh\ncase \"$1\" in -u) echo 0 ;; -un) echo root ;; *) exec /usr/bin/id \"$@\" ;; esac\n",
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/public/install.sh");
    let out = Command::new("sh")
        .arg(&script)
        .args([secret, "https://app.test", "container"])
        .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
        .env("TERMENBOX_DATA", d.join("data"))
        .env("TERMENBOX_UDP_PORT", "4443")
        .env("TERMENBOX_SKIP_FIREWALL", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = std::fs::read_to_string(d.join("calls.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&d);
    (stdout, calls)
}

#[test]
fn the_block_is_shown_after_a_secret_install() {
    let secret = "A".repeat(43);
    let (out, calls) = run("secret", &secret, &format!("QRART\n{LINK}\n"));
    assert!(out.contains(LINK) && out.contains("QRART"), "{out}");
    // The shortcut still runs first.
    let claim = calls.find("--claim").expect("--claim ran");
    assert!(claim < calls.find("--register").expect("--register ran"));
}

#[test]
fn the_block_is_shown_after_a_dash_install() {
    let (out, calls) = run("dash", "-", &format!("QRART\n{LINK}\n"));
    assert!(out.contains(LINK) && out.contains("QRART"), "{out}");
    assert!(!calls.contains("--claim"));
}

#[test]
fn a_rerun_on_an_owned_server_shows_the_readd_block() {
    let (out, _) = run(
        "owned",
        "-",
        &format!("This server is registered to @me.\nQRART\n{OWNED_LINK}\n"),
    );
    assert!(
        out.contains("registered to @me") && out.contains(OWNED_LINK),
        "{out}"
    );
    assert!(!out.contains("claim="));
}
