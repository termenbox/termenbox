//! `--verify-release` is what the installer and the updater call; drive it with a manifest signed by the real
//! `ssh-keygen`, the way CI signs releases.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "termenbox-release-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn verify(dir: &Path, keys: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_termenbox-server"))
        .env("TERMENBOX_RELEASE_KEYS", keys)
        .args(["--verify-release"])
        .arg(dir.join("manifest.json"))
        .arg("--release-sig")
        .arg(dir.join("manifest.json.sig"))
        .args(args)
        .output()
        .unwrap()
}

fn signed_release(dir: &Path, manifest: &str) -> Option<String> {
    let key = dir.join("key");
    if !Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "t", "-f"])
        .arg(&key)
        .status()
        .ok()?
        .success()
    {
        return None;
    }
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    if !Command::new("ssh-keygen")
        .args(["-Y", "sign", "-n", "termenbox-release", "-f"])
        .arg(&key)
        .arg(dir.join("manifest.json"))
        .output()
        .ok()?
        .status
        .success()
    {
        return None;
    }
    let public = std::fs::read_to_string(dir.join("key.pub")).unwrap();
    Some(format!(
        "r1 {}",
        public
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ")
    ))
}

const MANIFEST: &str = r#"{"version":"9.9.9","channel":"stable","freeze":false,"notes":"","binaries":{"x86_64":{"url":"https://github.com/termenbox/termenbox/releases/download/v9.9.9/termenbox-server-x86_64-linux","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}}}"#;

#[test]
fn a_signed_manifest_prints_its_fields_and_a_changed_one_is_refused() {
    let dir = temp();
    let Some(keys) = signed_release(&dir, MANIFEST) else {
        return;
    };
    let ok = verify(&dir, &keys, &["--release-arch", "x86_64"]);
    let out = String::from_utf8_lossy(&ok.stdout);
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    for line in ["kid=r1", "version=9.9.9", "freeze=false", "newer=true", "url=https://github.com/termenbox/termenbox/releases/download/v9.9.9/termenbox-server-x86_64-linux"] {
        assert!(out.contains(line), "missing {line} in {out}");
    }

    std::fs::write(
        dir.join("manifest.json"),
        MANIFEST.replace("9.9.9", "9.9.8"),
    )
    .unwrap();
    let bad = verify(&dir, &keys, &[]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("does not match"));
    assert!(
        bad.stdout.is_empty(),
        "nothing from an unverified manifest may be printed"
    );
}

#[test]
fn an_architecture_without_a_binary_is_an_error() {
    let dir = temp();
    let Some(keys) = signed_release(&dir, MANIFEST) else {
        return;
    };
    let out = verify(&dir, &keys, &["--release-arch", "riscv64"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no riscv64"));
}

#[test]
fn the_embedded_keys_do_not_trust_a_foreign_signer() {
    let dir = temp();
    let Some(_) = signed_release(&dir, MANIFEST) else {
        return;
    };
    // No override: the built-in release key is the only one trusted, and this signer is not it.
    let out = Command::new(env!("CARGO_BIN_EXE_termenbox-server"))
        .env_remove("TERMENBOX_RELEASE_KEYS")
        .arg("--verify-release")
        .arg(dir.join("manifest.json"))
        .arg("--release-sig")
        .arg(dir.join("manifest.json.sig"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a trusted release key"));
}
