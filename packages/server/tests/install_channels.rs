//! The installer's release channels (decisions 0130-0132): `--channel` and `--auto-update` are written to `update.toml`,
//! the updater units point at the channel's manifest, a prerelease version parses, every asset is downloaded from the
//! address the signed manifest names (never from beside the manifest), and a manifest of another channel, or one signed
//! by a key bound to another channel, installs nothing. It runs the no-root install (`host-me`) in a temporary HOME
//! against a `file://` release, with a stub `systemctl` and a stub server, so nothing real is touched.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const STUB_SERVER: &str = "#!/bin/sh\ncase \"$*\" in *--register*) echo 'https://app.test/#/pair/PAYLOAD' ;; esac\nexit 0\n";
const STUB_TMUX: &str = "#!/bin/sh\nif [ \"$1\" = \"-V\" ]; then echo 'tmux 3.8'; fi\nexit 0\n";
const NOTICES: [&str; 3] = [
    "tmux-COPYING",
    "tmux-libevent-LICENSE",
    "tmux-ncurses-COPYING",
];

fn write_exe(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-V")
        .output()
        .map(|_| true)
        .unwrap_or(false)
}

struct Case<'a> {
    /// What the manifest says it is, and the version it carries.
    manifest_channel: &'a str,
    version: &'a str,
    /// `channel=...` on the signing key's line; None is a key without the field (a stable key).
    key_channel: Option<&'a str>,
    /// Replaces the download address of the server binary in the manifest.
    binary_url: Option<&'a str>,
    /// The installer's arguments.
    args: &'a [&'a str],
}

struct Run {
    out: Output,
    home: PathBuf,
    release: PathBuf,
}

impl Run {
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.out.stdout).into_owned()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.out.stderr).into_owned()
    }
    fn state(&self, name: &str) -> String {
        std::fs::read_to_string(self.home.join(".local/state/termenbox").join(name))
            .unwrap_or_default()
    }
    fn unit(&self, name: &str) -> String {
        std::fs::read_to_string(self.home.join(".config/systemd/user").join(name))
            .unwrap_or_default()
    }
    fn installed(&self) -> bool {
        self.home.join(".local/bin/termenbox-server").exists()
    }
    fn release_url(&self) -> String {
        format!("file://{}", self.release.display())
    }
}

/// None when a tool the installer itself needs is missing (the test is then skipped, as the updater's are).
fn run(name: &str, case: &Case) -> Option<Run> {
    if !have("ssh-keygen") || Command::new("curl").arg("--version").output().is_err() {
        eprintln!("skipped {name}: ssh-keygen or curl is missing");
        return None;
    }
    let d = std::env::temp_dir().join(format!("tm-channels-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (home, release, assets, stubs) = (
        d.join("home"),
        d.join("rel"),
        d.join("rel/assets"),
        d.join("stubs"),
    );
    for dir in [&home, &assets, &stubs, &d.join("run")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let url = format!("file://{}", release.display());
    // Assets live under assets/, not beside the manifest: only the manifest's own addresses can find them.
    let mut binaries = serde_json::Map::new();
    let mut tmux = serde_json::Map::new();
    let sha = |bytes: &[u8]| {
        use sha2::{Digest, Sha256};
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    for arch in ["x86_64", "aarch64"] {
        let (server, t) = (
            format!("termenbox-server-{arch}-linux"),
            format!("tmux-{arch}-linux"),
        );
        write_exe(&assets.join(&server), STUB_SERVER);
        write_exe(&assets.join(&t), STUB_TMUX);
        let server_url = case
            .binary_url
            .map_or_else(|| format!("{url}/assets/{server}"), str::to_string);
        binaries.insert(
            arch.into(),
            serde_json::json!({"url": server_url, "sha256": sha(STUB_SERVER.as_bytes())}),
        );
        tmux.insert(arch.into(), serde_json::json!({"url": format!("{url}/assets/{t}"), "sha256": sha(STUB_TMUX.as_bytes())}));
    }
    let mut notices = serde_json::Map::new();
    for n in NOTICES {
        std::fs::write(assets.join(n), n).unwrap();
        notices.insert(
            n.into(),
            serde_json::json!({"url": format!("{url}/assets/{n}"), "sha256": sha(n.as_bytes())}),
        );
    }
    let manifest = serde_json::json!({
        "version": case.version, "channel": case.manifest_channel, "freeze": false, "notes": "",
        "binaries": binaries, "tmux": tmux, "tmux_notices": notices,
    });
    std::fs::write(
        release.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap() + "\n",
    )
    .unwrap();

    let key = d.join("key");
    assert!(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "t", "-f"])
        .arg(&key)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ssh-keygen")
        .args(["-Y", "sign", "-n", "termenbox-release", "-f"])
        .arg(&key)
        .arg(release.join("manifest.json"))
        .output()
        .unwrap()
        .status
        .success());
    let public = std::fs::read_to_string(d.join("key.pub")).unwrap();
    let public: Vec<&str> = public.split_whitespace().take(2).collect();
    let keys = format!(
        "k1 {} {}",
        public.join(" "),
        case.key_channel
            .map_or(String::new(), |c| format!("channel={c}"))
    );

    write_exe(&stubs.join("systemctl"), "#!/bin/sh\nexit 0\n");
    // The health check of the stub server is answered; everything else (the file:// release) is real curl.
    write_exe(
        &stubs.join("curl"),
        "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in http://127.0.0.1*) exit 0 ;; esac; done\nexec /usr/bin/curl \"$@\"\n",
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/installer/install.sh");
    let out = Command::new("sh")
        .arg(&script)
        .args(case.args)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", d.join("run"))
        .env("TERMENBOX_RELEASE_URL", &url)
        .env("TERMENBOX_RELEASE_KEYS", keys)
        .env("TERMENBOX_CLAIM", "-")
        .env("TERMENBOX_CLIENT_URL", "https://app.test")
        .env("TERMENBOX_SKIP_LINGER", "1")
        .env("TERMENBOX_SKIP_FIREWALL", "1")
        .output()
        .unwrap();
    Some(Run { out, home, release })
}

fn beta<'a>(args: &'a [&'a str]) -> Case<'a> {
    Case {
        manifest_channel: "beta",
        version: "0.2.0-rc.1",
        key_channel: Some("beta"),
        binary_url: None,
        args,
    }
}

#[test]
fn channel_beta_writes_the_beta_config_and_units_and_parses_a_prerelease() {
    // The option may follow the mode word.
    let Some(r) = run("beta", &beta(&["host-me", "--channel", "beta"])) else {
        return;
    };
    assert!(r.out.status.success(), "{}\n{}", r.stdout(), r.stderr());
    assert!(
        r.stdout()
            .contains("Termenbox 0.2.0-rc.1 will be installed"),
        "{}",
        r.stdout()
    );
    let config = r.state("update.toml");
    assert!(
        config.contains("channel = \"beta\"") && config.contains("mode = \"auto\""),
        "{config}"
    );
    let line = format!("Environment=TERMENBOX_RELEASE_URL={}", r.release_url());
    for unit in ["termenbox-update.service", "termenbox-request.service"] {
        assert!(r.unit(unit).contains(&line), "{unit}:\n{}", r.unit(unit));
        assert!(
            r.unit(unit).contains(&format!(
                "--requests-dir {}/.config/termenbox/requests",
                r.home.display()
            )),
            "{unit}:\n{}",
            r.unit(unit)
        );
    }
    // The server comes back from any exit, and ten failed starts in five minutes end in a visible failed state.
    let server = r.unit("termenbox.service");
    for line in [
        "Restart=always",
        "RestartSec=2",
        "StartLimitIntervalSec=300",
        "StartLimitBurst=10",
    ] {
        assert!(server.lines().any(|l| l == line), "{line}:\n{server}");
    }
    // downloaded from the manifest's address (assets/), which release_url + name would not find
    assert!(r.installed());
    assert!(!r.release.join("termenbox-server-x86_64-linux").exists());
}

#[test]
fn an_existing_user_install_migrates_its_timer_without_losing_custom_arguments_and_is_idempotent() {
    let Some(r) = run("migrate-requests", &beta(&["host-me", "--channel", "beta"])) else {
        return;
    };
    assert!(r.out.status.success(), "{}", r.stderr());
    let unit = r.home.join(".config/systemd/user/termenbox-update.service");
    let requests = format!(
        " --requests-dir {}/.config/termenbox/requests",
        r.home.display()
    );
    let original = r
        .unit("termenbox-update.service")
        .replace(&requests, "")
        .replace("--user-helper", "--user-helper --custom-preserved");
    std::fs::write(&unit, &original).unwrap();
    // The stub server records journal requests, so the real installer's migration must journal before replacing.
    let log = r.home.join("journal-calls");
    write_exe(
        &r.home.join(".local/bin/termenbox-server"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$*\" in *--register*) echo 'https://app.test/#/pair/PAYLOAD' ;; esac\n", log.display()),
    );
    let root = r.home.parent().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/installer/install.sh");
    let rerun = || {
        Command::new("sh")
            .arg(&script)
            .arg("host-me")
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.join("stubs").display()),
            )
            .env("HOME", &r.home)
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("TERMENBOX_CLAIM", "-")
            .env("TERMENBOX_CLIENT_URL", "https://app.test")
            .env("TERMENBOX_SKIP_LINGER", "1")
            .env("TERMENBOX_SKIP_FIREWALL", "1")
            .output()
            .unwrap()
    };
    let out = rerun();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let migrated = r.unit("termenbox-update.service");
    assert!(migrated.contains("--custom-preserved"));
    assert_eq!(migrated.matches("--requests-dir").count(), 1);
    assert!(migrated.contains(&requests));
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(
        logged.contains("--journal-record file_replaced"),
        "{logged}"
    );
    let backups = r.home.join(".local/state/termenbox/backups");
    assert!(std::fs::read_dir(&backups)
        .unwrap()
        .flatten()
        .any(|p| std::fs::read_to_string(p.path()).ok().as_deref() == Some(original.as_str())));
    let out = rerun();
    assert!(out.status.success());
    assert_eq!(r.unit("termenbox-update.service"), migrated);
    let after = std::fs::read_to_string(log).unwrap();
    assert_eq!(
        after.matches("--journal-record file_replaced").count(),
        logged.matches("--journal-record file_replaced").count()
    );
}

#[test]
fn the_update_timer_checks_at_boot_and_every_six_hours_and_an_old_daily_timer_is_replaced_once() {
    let Some(r) = run("timer-cadence", &beta(&["host-me", "--channel", "beta"])) else {
        return;
    };
    assert!(r.out.status.success(), "{}", r.stderr());
    let fresh = r.unit("termenbox-update.timer");
    assert!(fresh.contains("OnBootSec=2min"), "{fresh}");
    assert!(fresh.contains("OnUnitActiveSec=6h"), "{fresh}");
    assert!(!fresh.contains("OnCalendar=daily"), "{fresh}");
    // An install from before decision 0153 has the daily timer; the next run replaces it, with a backup.
    let timer = r.home.join(".config/systemd/user/termenbox-update.timer");
    let daily = "[Unit]\nDescription=Check for a new Termenbox release every day\n\n[Timer]\nOnCalendar=daily\nRandomizedDelaySec=3600\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n";
    std::fs::write(&timer, daily).unwrap();
    let log = r.home.join("journal-calls");
    write_exe(
        &r.home.join(".local/bin/termenbox-server"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$*\" in *--register*) echo 'https://app.test/#/pair/PAYLOAD' ;; esac\n", log.display()),
    );
    let root = r.home.parent().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/installer/install.sh");
    let rerun = || {
        Command::new("sh")
            .arg(&script)
            .arg("host-me")
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.join("stubs").display()),
            )
            .env("HOME", &r.home)
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("TERMENBOX_CLAIM", "-")
            .env("TERMENBOX_CLIENT_URL", "https://app.test")
            .env("TERMENBOX_SKIP_LINGER", "1")
            .env("TERMENBOX_SKIP_FIREWALL", "1")
            .output()
            .unwrap()
    };
    let out = rerun();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let migrated = r.unit("termenbox-update.timer");
    assert_eq!(migrated, fresh);
    let replaced = |l: &str| l.matches("--journal-record file_replaced").count();
    let logged = std::fs::read_to_string(&log).unwrap();
    assert_eq!(replaced(&logged), 1, "{logged}");
    let backups = r.home.join(".local/state/termenbox/backups");
    assert!(std::fs::read_dir(&backups)
        .unwrap()
        .flatten()
        .any(|p| std::fs::read_to_string(p.path()).ok().as_deref() == Some(daily)));
    assert!(rerun().status.success());
    assert_eq!(r.unit("termenbox-update.timer"), migrated);
    assert_eq!(replaced(&std::fs::read_to_string(log).unwrap()), 1);
}

#[test]
fn a_nightly_install_only_notifies_unless_auto_update_is_asked_for() {
    let nightly = |args| Case {
        manifest_channel: "nightly",
        version: "0.1.36-nightly.20261022",
        key_channel: Some("nightly"),
        binary_url: None,
        args,
    };
    let Some(r) = run("nightly", &nightly(&["--channel=nightly", "host-me"])) else {
        return;
    };
    assert!(r.out.status.success(), "{}\n{}", r.stdout(), r.stderr());
    let config = r.state("update.toml");
    assert!(
        config.contains("channel = \"nightly\"") && config.contains("mode = \"notify\""),
        "{config}"
    );
    assert!(r.stdout().contains("notify only"), "{}", r.stdout());

    let Some(r) = run(
        "nightly-auto",
        &nightly(&["--channel", "nightly", "--auto-update"]),
    ) else {
        return;
    };
    assert!(r.out.status.success(), "{}\n{}", r.stdout(), r.stderr());
    assert!(
        r.state("update.toml").contains("mode = \"auto\""),
        "{}",
        r.state("update.toml")
    );
}

#[test]
fn the_default_is_stable_and_automatic() {
    let stable = Case {
        manifest_channel: "stable",
        version: "0.1.37",
        key_channel: None,
        binary_url: None,
        args: &[],
    };
    let Some(r) = run("stable", &stable) else {
        return;
    };
    assert!(r.out.status.success(), "{}\n{}", r.stdout(), r.stderr());
    let config = r.state("update.toml");
    assert!(
        config.contains("channel = \"stable\"") && config.contains("mode = \"auto\""),
        "{config}"
    );
    assert!(r
        .unit("termenbox-update.service")
        .contains("TERMENBOX_RELEASE_URL="));
}

#[test]
fn a_release_of_another_channel_or_a_key_bound_to_another_channel_installs_nothing() {
    // a beta server is not offered a nightly
    let nightly_manifest = Case {
        manifest_channel: "nightly",
        version: "0.1.36-nightly.20261022",
        key_channel: Some("nightly"),
        binary_url: None,
        args: &["--channel", "beta"],
    };
    let Some(r) = run("wrong-channel", &nightly_manifest) else {
        return;
    };
    assert!(!r.out.status.success());
    assert!(r.stderr().contains("does not follow"), "{}", r.stderr());
    assert!(!r.installed() && r.state("update.toml").is_empty());

    // a stable server is not offered a beta
    let Some(r) = run("stable-vs-beta", &beta(&[])) else {
        return;
    };
    assert!(!r.out.status.success() && !r.installed());

    // a nightly manifest signed by a key that is not bound to nightly
    let unbound = Case {
        key_channel: None,
        ..Case {
            manifest_channel: "nightly",
            version: "0.1.36-nightly.20261022",
            key_channel: None,
            binary_url: None,
            args: &["--channel", "nightly"],
        }
    };
    let Some(r) = run("unbound-key", &unbound) else {
        return;
    };
    assert!(!r.out.status.success());
    assert!(
        r.stderr().contains("signature is not valid"),
        "{}",
        r.stderr()
    );
    assert!(!r.installed());
}

#[test]
fn an_address_in_the_manifest_outside_the_project_releases_is_refused() {
    for bad in [
        "https://evil.example/termenbox-server-x86_64-linux",
        "file:///etc/../tmp/x",
    ] {
        let case = Case {
            binary_url: Some(bad),
            ..beta(&["--channel", "beta"])
        };
        let Some(r) = run("bad-url", &case) else {
            return;
        };
        assert!(!r.out.status.success(), "{bad}");
        assert!(r.stderr().contains("address"), "{bad}: {}", r.stderr());
        assert!(!r.installed());
    }
}

#[test]
fn an_unknown_channel_or_option_is_refused_before_anything_happens() {
    for args in [
        &["--channel", "edge"][..],
        &["--channel"][..],
        &["--frobnicate"][..],
        &["host-me", "container"][..],
    ] {
        let Some(r) = run("bad-args", &beta(args)) else {
            return;
        };
        assert!(!r.out.status.success(), "{args:?}");
        assert!(!r.installed());
    }
}
