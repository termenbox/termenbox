//! The tmux server termenbox runs for itself (decision 0120): a private socket and a config file termenbox owns, so
//! the user's own tmux and its config neither break termenbox nor are changed by it.

use std::path::{Path, PathBuf};

/// The socket name (`tmux -L`) of termenbox's own server.
pub const SOCKET: &str = "termenbox";

/// Which tmux server the workspaces live on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Socket {
    /// Termenbox's own server (`-L termenbox`, `-f` the config below).
    Private,
    /// The user's default socket and config. Only an explicit `TERMENBOX_TMUX_SOCKET=default` selects it (decision
    /// 0121): the user's server can be any tmux version, and a client of another version cannot attach to it.
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

/// The private server, unless the owner explicitly asked for the default socket (decision 0121). Neither the saved
/// choice nor workspaces already running on the default socket decide it: the tmux the server runs is pinned, and
/// it can only attach to a server of its own version, which a tmux the user started earlier may not be.
pub fn decide(explicit: Option<Socket>) -> Socket {
    explicit.unwrap_or(Socket::Private)
}

/// The file name of the `BROWSER` shim next to the config.
pub const OPEN_SHIM: &str = "termenbox-open";

/// What a program that wants to open a web page runs as `$BROWSER` inside a session. A server has no screen, so the
/// address goes to the client instead: an OSC 7770 sequence, wrapped for tmux to pass through to the browser, which
/// opens it on the device that shows the terminal. Only `http(s)` addresses without control characters go through.
const OPEN_SHIM_SCRIPT: &str = r#"#!/bin/sh
# Written by termenbox-server on every start. Sends a web address to the browser showing this terminal.
case $1 in
  http://?*|https://?*) ;;
  *) exit 1 ;;
esac
case $1 in
  *[![:graph:]]*) exit 1 ;;
esac
printf '\033Ptmux;\033\033]7770;open;%s\007\033\\' "$1" >"${TERMENBOX_OPEN_TTY:-/dev/tty}"
"#;

/// The default mouse bindings that draw or create something inside the terminal. With the mouse on, a right-click (a
/// Ctrl+click on a Mac) opens tmux's menu, and on tmux 3.8 a Ctrl+press with any movement makes a floating pane one
/// character in size (`new-pane -M`), which a Ctrl/Cmd+click on a link easily is. Alt+drag moves a pane.
const MOUSE_UNBIND: [&str; 13] = [
    "MouseDown3Pane",
    "M-MouseDown3Pane",
    "MouseDown3Status",
    "MouseDown3StatusLeft",
    "M-MouseDown3Status",
    "M-MouseDown3StatusLeft",
    "MouseDown3Empty",
    "M-MouseDown3Empty",
    "C-MouseDrag1Pane",
    "C-MouseDrag1Empty",
    "M-MouseDrag1Pane",
    "M-MouseDrag1Border",
    "C-MouseDown1Status",
];

/// The config of the private server. `term` is the `default-terminal` the host's terminfo supports; `local` is the
/// optional file the owner may add next to it, which termenbox never writes; `shim` is the `BROWSER` program.
pub fn render(term: &str, local: Option<&Path>, shim: Option<&Path>) -> String {
    let mut out = String::from(
        "# Written by termenbox-server on every start; changes here are lost. Put your own in tmux.local.conf.\n",
    );
    out.push_str(&format!("set -g default-terminal \"{term}\"\n"));
    // Copying works across the browser: a drag in tmux copies, and tmux and the programs in it put the text on the
    // clipboard of the machine the browser runs on (OSC 52). Shift+drag still makes the browser's own selection.
    out.push_str("set -g mouse on\nset -g set-clipboard on\nset -gq allow-passthrough on\n");
    // The client draws its own workspace chrome, so tmux's status line only costs a row of the terminal.
    out.push_str("set -g status off\n");
    // The client has its own menus and tabs, so tmux's mouse-made windows only look like a broken screen in the
    // browser. The owner's file can bind any of them again.
    for key in MOUSE_UNBIND {
        out.push_str(&format!("unbind -nq {key}\n"));
    }
    if let Some(shim) = shim {
        out.push_str(&format!(
            "set-environment -g BROWSER \"{}\"\n",
            shim.display()
        ));
    }
    if let Some(local) = local {
        out.push_str(&format!("source-file -q \"{}\"\n", local.display()));
    }
    out
}

/// Writes the config into `dir` and returns its path.
pub fn write_config(dir: &Path, term: &str) -> std::io::Result<PathBuf> {
    let path = dir.join("tmux.conf");
    let shim = dir.join(OPEN_SHIM);
    write_atomic(&shim, OPEN_SHIM_SCRIPT)?;
    std::fs::set_permissions(&shim, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    write_atomic(
        &path,
        &render(term, Some(&dir.join("tmux.local.conf")), Some(&shim)),
    )?;
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
    fn the_private_server_is_the_default_and_only_an_explicit_setting_picks_the_user_s() {
        use Socket::*;
        assert_eq!(decide(None), Private);
        assert_eq!(decide(Some(Private)), Private);
        assert_eq!(decide(Some(Default)), Default);
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
        let c = render(
            "screen-256color",
            Some(Path::new("/c/tmux.local.conf")),
            None,
        );
        assert!(c.contains("set -g default-terminal \"screen-256color\"\n"));
        assert!(c.ends_with("source-file -q \"/c/tmux.local.conf\"\n"));
        // the owner's file comes last, so it can switch any of these off
        assert!(c.find("set -g mouse on").unwrap() < c.find("source-file").unwrap());
        assert!(c.contains("set -g set-clipboard on\n"));
        assert!(c.find("set -g status off").unwrap() < c.find("source-file").unwrap());
        assert!(!render("xterm-256color", None, None).contains("source-file"));
    }

    #[test]
    fn tmux_s_own_mouse_menus_are_off_and_the_owner_can_bring_them_back() {
        let c = render(
            "xterm-256color",
            Some(Path::new("/c/tmux.local.conf")),
            None,
        );
        for key in [
            "MouseDown3Pane",
            "M-MouseDown3Pane",
            "MouseDown3Status",
            "MouseDown3StatusLeft",
        ] {
            let line = format!("unbind -nq {key}\n");
            assert!(c.contains(&line), "{key}");
            assert!(
                c.find(&line).unwrap() < c.find("source-file").unwrap(),
                "{key}"
            );
        }
    }

    #[test]
    fn a_modified_mouse_gesture_never_creates_moves_or_swaps_a_pane() {
        let c = render(
            "xterm-256color",
            Some(Path::new("/c/tmux.local.conf")),
            None,
        );
        // tmux 3.8: Ctrl+drag makes a floating pane (a Ctrl+click on a link is easily one), Alt+drag moves a pane
        for key in [
            "C-MouseDrag1Pane",
            "C-MouseDrag1Empty",
            "M-MouseDrag1Pane",
            "M-MouseDrag1Border",
            "MouseDown3Empty",
            "M-MouseDown3Empty",
            "M-MouseDown3Status",
            "M-MouseDown3StatusLeft",
            "C-MouseDown1Status",
        ] {
            let line = format!("unbind -nq {key}\n");
            assert!(c.contains(&line), "{key}");
            assert!(
                c.find(&line).unwrap() < c.find("source-file").unwrap(),
                "{key}"
            );
        }
    }

    #[test]
    fn programs_in_a_session_open_urls_through_the_shim() {
        let c = render("xterm-256color", None, Some(Path::new("/c/termenbox-open")));
        assert!(c.contains("set-environment -g BROWSER \"/c/termenbox-open\"\n"));
        assert!(!render("xterm-256color", None, None).contains("BROWSER"));
    }

    #[test]
    fn the_shim_passes_a_web_address_to_the_client_and_refuses_anything_else() {
        let dir = std::env::temp_dir().join(format!("tm-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conf = write_config(&dir, "tmux-256color").unwrap();
        assert!(std::fs::read_to_string(conf)
            .unwrap()
            .contains("termenbox-open"));
        let shim = dir.join("termenbox-open");
        let tty = dir.join("tty");
        let run = |arg: &str| {
            std::fs::write(&tty, "").unwrap();
            let ok = std::process::Command::new("sh")
                .arg(&shim)
                .arg(arg)
                .env("TERMENBOX_OPEN_TTY", &tty)
                .status()
                .unwrap()
                .success();
            (ok, std::fs::read_to_string(&tty).unwrap())
        };
        // tmux passes a DCS "tmux;" string on with every ESC inside it doubled
        let (ok, out) = run("https://example.com/login?code=1");
        assert!(ok);
        assert_eq!(
            out,
            "\x1bPtmux;\x1b\x1b]7770;open;https://example.com/login?code=1\x07\x1b\\"
        );
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "example.com",
            "https://a\x1b]52;c;eA==\x07",
        ] {
            assert_eq!(run(bad), (false, String::new()), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
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
