//! `termenbox-server --version` prints the version this binary was built as, so an owner can tell what is installed.
use std::process::Command;

#[test]
fn version_flag_prints_the_cargo_version() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_termenbox-server"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("termenbox-server {}", env!("CARGO_PKG_VERSION"))
        );
    }
}
