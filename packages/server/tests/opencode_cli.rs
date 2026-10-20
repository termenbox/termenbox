//! `--connect-opencode` / `--disconnect-opencode` as real processes: the preview changes nothing, apply writes
//! exactly the plugin that was shown, and disconnect restores (or removes) the file the connect changed.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tm-oc-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(config: &Path, args: &[&str], env: &[(&str, &Path)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_termenbox-server"));
    cmd.arg("--config-dir").arg(config).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

#[test]
fn preview_changes_nothing_apply_writes_the_shown_plugin_and_disconnect_removes_it() {
    let d = dir("flow");
    let config = d.join("state");
    let plugin = d.join("opencode/plugins/termenbox.js");
    let odir = d.join("opencode");
    let os = odir.to_str().unwrap();
    let preview = run(&config, &["--opencode-dir", os, "--connect-opencode"], &[]);
    assert!(preview.status.success(), "{preview:?}");
    let shown = String::from_utf8_lossy(&preview.stdout);
    assert!(shown.contains("termenbox-opencode-plugin"));
    assert!(shown.contains(plugin.to_str().unwrap()));
    assert!(!plugin.exists(), "a preview must change nothing");

    let apply = run(
        &config,
        &["--opencode-dir", os, "--connect-opencode", "--yes"],
        &[],
    );
    assert!(apply.status.success(), "{apply:?}");
    let written = std::fs::read_to_string(&plugin).unwrap();
    assert!(written.contains("termenbox-opencode-plugin"));
    assert!(written.contains(env!("CARGO_BIN_EXE_termenbox-server")));
    assert!(
        shown.contains(&written),
        "the file holds exactly the preview"
    );

    let disconnect = run(
        &config,
        &["--opencode-dir", os, "--disconnect-opencode"],
        &[],
    );
    assert!(disconnect.status.success(), "{disconnect:?}");
    assert!(!plugin.exists(), "the file did not exist before connect");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn disconnect_follows_the_journal_when_the_config_directory_is_not_named() {
    let d = dir("journal");
    let config = d.join("state");
    let plugin = d.join("opencode/plugins/termenbox.js");
    let odir = d.join("opencode");
    let apply = run(
        &config,
        &[
            "--opencode-dir",
            odir.to_str().unwrap(),
            "--connect-opencode",
            "--yes",
        ],
        &[],
    );
    assert!(apply.status.success(), "{apply:?}");
    assert!(plugin.exists());
    // A different OPENCODE_CONFIG_DIR at disconnect: the journal still names the file to undo.
    let disconnect = Command::new(env!("CARGO_BIN_EXE_termenbox-server"))
        .arg("--config-dir")
        .arg(&config)
        .arg("--disconnect-opencode")
        .env("OPENCODE_CONFIG_DIR", "/nonexistent/other")
        .output()
        .unwrap();
    assert!(disconnect.status.success(), "{disconnect:?}");
    assert!(!plugin.exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_environment_names_the_plugin_directory_when_the_flag_does_not() {
    let d = dir("env");
    let config = d.join("state");
    let envdir = d.join("custom");
    let apply = run(
        &config,
        &["--connect-opencode", "--yes"],
        &[("OPENCODE_CONFIG_DIR", &envdir)],
    );
    assert!(apply.status.success(), "{apply:?}");
    assert!(envdir.join("plugins/termenbox.js").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_plugin_someone_else_wrote_is_neither_previewed_nor_replaced() {
    let d = dir("guard");
    let config = d.join("state");
    let plugin = d.join("opencode/plugins/termenbox.js");
    std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    let mine = "// my own plugin\nexport default {};\n";
    std::fs::write(&plugin, mine).unwrap();
    let odir = d.join("opencode");
    let os = odir.to_str().unwrap();

    let preview = run(&config, &["--opencode-dir", os, "--connect-opencode"], &[]);
    assert!(!preview.status.success());
    let apply = run(
        &config,
        &["--opencode-dir", os, "--connect-opencode", "--yes"],
        &[],
    );
    assert!(!apply.status.success());
    assert_eq!(std::fs::read_to_string(&plugin).unwrap(), mine);

    // Disconnect is not a way around that: a file that is not ours is left alone.
    let disconnect = run(
        &config,
        &["--opencode-dir", os, "--disconnect-opencode"],
        &[],
    );
    assert!(disconnect.status.success(), "{disconnect:?}");
    assert_eq!(std::fs::read_to_string(&plugin).unwrap(), mine);
    let _ = std::fs::remove_dir_all(&d);
}
