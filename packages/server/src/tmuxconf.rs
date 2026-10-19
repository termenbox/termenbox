//! The tmux server termenbox runs for itself (decision 0120): a private socket and a config file termenbox owns, so
//! the user's own tmux and its config neither break termenbox nor are changed by it.

use std::path::{Path, PathBuf};

/// The socket name (`tmux -L`) of termenbox's own server.
pub const SOCKET: &str = "termenbox";

/// Which tmux server the sessions live on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Socket {
    /// Termenbox's own server (`-L termenbox`, `-f` the config below).
    Private,
    /// The user's default socket and config, as before 0120; kept for installs that already have sessions there.
    Default,
}

impl Socket {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "termenbox",
            Self::Default => "default",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "termenbox" | "private" => Some(Self::Private),
            "default" => Some(Self::Default),
            _ => None,
        }
    }
}

/// The choice saved by an earlier start; absent or unreadable means "not decided yet".
pub fn saved(dir: &Path) -> Option<Socket> {
    Socket::parse(&std::fs::read_to_string(dir.join("tmux_socket")).ok()?)
}

pub fn save(dir: &Path, socket: Socket) -> std::io::Result<()> {
    write_atomic(&dir.join("tmux_socket"), socket.as_str())
}

/// An explicit setting wins, then the saved choice. With neither, an install that already has sessions on the
/// default socket keeps them there, and anything else gets the private server.
pub fn decide(
    explicit: Option<Socket>,
    saved: Option<Socket>,
    default_has_sessions: bool,
) -> Socket {
    explicit.or(saved).unwrap_or(if default_has_sessions {
        Socket::Default
    } else {
        Socket::Private
    })
}

/// The config of the private server. `term` is the `default-terminal` the host's terminfo supports; `local` is the
/// optional file the owner may add next to it, which termenbox never writes.
pub fn render(term: &str, local: Option<&Path>) -> String {
    let mut out = String::from(
        "# Written by termenbox-server on every start; changes here are lost. Put your own in tmux.local.conf.\n",
    );
    out.push_str(&format!("set -g default-terminal \"{term}\"\n"));
    if let Some(local) = local {
        out.push_str(&format!("source-file -q \"{}\"\n", local.display()));
    }
    out
}

/// Writes the config into `dir` and returns its path.
pub fn write_config(dir: &Path, term: &str) -> std::io::Result<PathBuf> {
    let path = dir.join("tmux.conf");
    write_atomic(&path, &render(term, Some(&dir.join("tmux.local.conf"))))?;
    Ok(path)
}

/// Writes to a temporary file and renames it over `path`, so a tmux starting meanwhile never reads half a file.
fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, content)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_choice_beats_the_saved_one_and_sessions_only_decide_when_nothing_is_set() {
        use Socket::*;
        assert_eq!(decide(Some(Private), Some(Default), true), Private);
        assert_eq!(decide(None, Some(Default), false), Default);
        assert_eq!(decide(None, Some(Private), true), Private);
        assert_eq!(decide(None, None, true), Default);
        assert_eq!(decide(None, None, false), Private);
    }

    #[test]
    fn the_choice_survives_a_restart_and_a_bad_file_means_undecided() {
        let dir = std::env::temp_dir().join(format!("tm-sock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(saved(&dir), None);
        save(&dir, Socket::Default).unwrap();
        assert_eq!(saved(&dir), Some(Socket::Default));
        save(&dir, Socket::Private).unwrap();
        assert_eq!(saved(&dir), Some(Socket::Private));
        std::fs::write(dir.join("tmux_socket"), "nonsense").unwrap();
        assert_eq!(saved(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_config_names_the_terminal_and_ends_with_the_optional_local_file() {
        let c = render("screen-256color", Some(Path::new("/c/tmux.local.conf")));
        assert!(c.contains("set -g default-terminal \"screen-256color\"\n"));
        assert!(c.ends_with("source-file -q \"/c/tmux.local.conf\"\n"));
        assert!(!render("xterm-256color", None).contains("source-file"));
    }

    #[test]
    fn the_config_is_rewritten_on_every_start() {
        let dir = std::env::temp_dir().join(format!("tm-conf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tmux.conf"), "stale").unwrap();
        let path = write_config(&dir, "tmux-256color").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("tmux-256color") && !text.contains("stale"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
