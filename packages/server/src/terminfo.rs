//! The TERM the bundled tmux gives the panes (decision 0119). The bundled tmux carries these terminfo entries
//! compiled in, so it runs on a host with no terminfo database; the programs inside the panes read the host's
//! database though, so a pane's TERM has to be one the host knows.

use std::path::{Path, PathBuf};

/// The entries compiled into the bundled tmux (`FALLBACKS` in `.github/scripts/build-tmux.sh`).
pub const COMPILED_IN: [&str; 5] = [
    "xterm-256color",
    "screen-256color",
    "tmux-256color",
    "xterm",
    "linux",
];

/// Best first: tmux's own entry, then the screen one old hosts have, then the one every host has.
const PANE_PREFERENCE: [&str; 3] = ["tmux-256color", "screen-256color", "xterm-256color"];

/// The first of the preferred entries that `host_has`, or `xterm-256color` (a pane then at least gets the entry
/// the attaching terminal itself uses) when the host knows none of them.
pub fn pane_term(host_has: &dyn Fn(&str) -> bool) -> &'static str {
    PANE_PREFERENCE
        .into_iter()
        .find(|t| host_has(t))
        .unwrap_or("xterm-256color")
}

/// Whether a terminfo database under `dirs` has `name` (`<dir>/<first letter>/<name>`, or `<dir>/<hex>/<name>` on
/// case-insensitive file systems).
pub fn in_dirs(dirs: &[PathBuf], name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return false;
    };
    dirs.iter().any(|d| {
        d.join(first.to_string()).join(name).is_file()
            || d.join(format!("{:x}", first as u32)).join(name).is_file()
    })
}

/// The directories ncurses searches: `$TERMINFO`, `~/.terminfo`, `$TERMINFO_DIRS`, then the system ones.
pub fn search_dirs(
    terminfo: Option<&str>,
    terminfo_dirs: Option<&str>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![];
    dirs.extend(terminfo.filter(|t| !t.is_empty()).map(PathBuf::from));
    dirs.extend(home.map(|h| h.join(".terminfo")));
    dirs.extend(
        terminfo_dirs
            .unwrap_or("")
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    for d in [
        "/etc/terminfo",
        "/lib/terminfo",
        "/usr/lib/terminfo",
        "/usr/share/terminfo",
    ] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

/// The TERM for panes on this machine.
pub fn host_pane_term() -> &'static str {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs = search_dirs(
        std::env::var("TERMINFO").ok().as_deref(),
        std::env::var("TERMINFO_DIRS").ok().as_deref(),
        home.as_deref(),
    );
    pane_term(&|t| in_dirs(&dirs, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_term_prefers_tmux_then_screen_then_xterm() {
        assert_eq!(pane_term(&|_| true), "tmux-256color");
        assert_eq!(pane_term(&|t| t != "tmux-256color"), "screen-256color");
        assert_eq!(pane_term(&|t| t == "xterm-256color"), "xterm-256color");
        assert_eq!(pane_term(&|_| false), "xterm-256color");
    }

    #[test]
    fn every_pane_term_is_compiled_into_the_bundled_tmux() {
        for t in PANE_PREFERENCE {
            assert!(COMPILED_IN.contains(&t), "{t}");
        }
    }

    #[test]
    fn the_compiled_in_set_matches_the_build_script() {
        let script = include_str!("../../../.github/scripts/build-tmux.sh");
        let line = script
            .lines()
            .find_map(|l| l.strip_prefix("FALLBACKS="))
            .unwrap();
        assert_eq!(line, COMPILED_IN.join(","));
    }

    #[test]
    fn a_terminfo_directory_is_searched_by_first_letter_and_by_hex() {
        let dir = std::env::temp_dir().join(format!("tm-terminfo-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("t")).unwrap();
        std::fs::create_dir_all(dir.join("78")).unwrap();
        std::fs::write(dir.join("t/tmux-256color"), b"x").unwrap();
        std::fs::write(dir.join("78/xterm"), b"x").unwrap();
        let dirs = vec![dir.clone()];
        assert!(in_dirs(&dirs, "tmux-256color"));
        assert!(in_dirs(&dirs, "xterm"));
        assert!(!in_dirs(&dirs, "screen-256color"));
        assert!(!in_dirs(&dirs, ""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_search_order_is_terminfo_home_terminfo_dirs_then_the_system() {
        let d = search_dirs(Some("/a"), Some("/b:/c"), Some(Path::new("/h")));
        assert_eq!(
            d[..4],
            [
                PathBuf::from("/a"),
                PathBuf::from("/h/.terminfo"),
                PathBuf::from("/b"),
                PathBuf::from("/c")
            ]
        );
        assert!(d.contains(&PathBuf::from("/usr/share/terminfo")));
        assert_eq!(
            search_dirs(None, None, None)[0],
            PathBuf::from("/etc/terminfo")
        );
    }
}
