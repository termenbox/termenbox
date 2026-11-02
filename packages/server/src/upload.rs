//! File upload to a tab's working folder (decision 0194). This part decides where a file may go and what it may be
//! called; the writer follows. The client never names a folder: the server reads the tab's live one, canonicalises it,
//! and refuses unless it is `$HOME` or below.

#![allow(dead_code)] // the writer of step 2.3 of the upload plan uses these; drop this then

use std::path::{Path, PathBuf};

use crate::control::WindowState;
use crate::listdir::{confine, Confinement};

/// `control::parse_windows` cuts a path to this many characters; one that reached it may be cut, so it is refused.
const PATH_CAP: usize = 256;
/// The longest file name, in bytes.
const NAME_MAX: usize = 255;

/// Why an upload cannot start; `reason` is a short sentence for the user.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    NoWindow,
    TooLong,
    Outside,
    Missing,
    NotDir,
    HomeUnknown,
    Name(&'static str),
}

impl Refusal {
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::NoWindow => "That tab has no running terminal to upload into",
            Refusal::TooLong => "The tab's folder path is too long to use",
            Refusal::Outside => "The tab's folder is outside your home folder",
            Refusal::Missing => "The tab's folder no longer exists",
            Refusal::NotDir => "The tab's folder is not a folder",
            Refusal::HomeUnknown => "The home folder is unknown",
            Refusal::Name(r) => r,
        }
    }
}

/// The live working folder of the window with id `window_id`, as tmux reports it. A tab with no live window has none:
/// the registry directory is where the tab began, not where the user is (fact 0076).
pub fn live_folder<'a>(windows: &'a [WindowState], window_id: &str) -> Result<&'a str, Refusal> {
    let w = windows
        .iter()
        .find(|w| !window_id.is_empty() && w.window_id == window_id)
        .ok_or(Refusal::NoWindow)?;
    if w.info.path.chars().count() >= PATH_CAP {
        return Err(Refusal::TooLong);
    }
    Ok(&w.info.path)
}

/// The canonical folder an upload is pinned to: `live` (a tab's current folder) canonicalised, inside `home`.
/// Blocking: call it from `spawn_blocking`.
pub fn resolve_destination(home: &Path, live: &str) -> Result<PathBuf, Refusal> {
    let home = std::fs::canonicalize(home).map_err(|_| Refusal::HomeUnknown)?;
    if !Path::new(live).is_absolute() {
        return Err(Refusal::Missing);
    }
    confine(&home, Path::new(live)).map_err(|c| match c {
        Confinement::Missing => Refusal::Missing,
        Confinement::Outside => Refusal::Outside,
        Confinement::NotDir => Refusal::NotDir,
    })
}

/// The rules for a file name (decision 0194). The server never rewrites a name; the client cleans it before asking.
pub fn check_name(name: &str) -> Result<(), Refusal> {
    let bad = |r| Err(Refusal::Name(r));
    if name.is_empty() {
        return bad("The file name is empty");
    }
    if name.len() > NAME_MAX {
        return bad("The file name is too long");
    }
    if name == "." || name == ".." || name.starts_with('.') {
        return bad("The file name cannot start with a dot");
    }
    if name
        .chars()
        .any(|c| c == '/' || c == '\\' || c.is_control())
    {
        return bad("The file name has a character that is not allowed");
    }
    if name.trim() != name {
        return bad("The file name cannot start or end with a space");
    }
    Ok(())
}

#[cfg(test)]
mod upload_destination {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use termenbox_core::protocol::WindowInfo;

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("tb-upload-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            for d in ["home/code/app", "home/.config/proj", "outside/secret"] {
                fs::create_dir_all(root.join(d)).unwrap();
            }
            let root = fs::canonicalize(root).unwrap();
            let home = root.join("home");
            Fixture { root, home }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn window(id: &str, path: &str) -> WindowState {
        WindowState {
            info: WindowInfo {
                index: 0,
                name: "t".into(),
                command: "bash".into(),
                path: path.into(),
                active: true,
                agent: None,
            },
            activity: 0,
            window_id: id.into(),
            tab: String::new(),
        }
    }

    #[test]
    fn a_folder_under_home_is_pinned_in_canonical_form() {
        let f = Fixture::new("under");
        let p = format!("{}/code/app/../app", f.home.display());
        assert_eq!(
            resolve_destination(&f.home, &p),
            Ok(f.home.join("code/app"))
        );
        assert_eq!(
            resolve_destination(&f.home, f.home.to_str().unwrap()),
            Ok(f.home.clone())
        );
    }

    #[test]
    fn a_folder_with_a_dot_component_is_accepted() {
        let f = Fixture::new("dot");
        let p = f.home.join(".config/proj");
        assert_eq!(resolve_destination(&f.home, p.to_str().unwrap()), Ok(p));
    }

    #[test]
    fn a_folder_outside_home_is_refused() {
        let f = Fixture::new("outside");
        let p = f.root.join("outside/secret");
        assert_eq!(
            resolve_destination(&f.home, p.to_str().unwrap()),
            Err(Refusal::Outside)
        );
        let up = format!("{}/code/../../outside", f.home.display());
        assert_eq!(resolve_destination(&f.home, &up), Err(Refusal::Outside));
    }

    #[test]
    fn a_symlink_leaving_home_is_refused() {
        let f = Fixture::new("link");
        symlink(f.root.join("outside/secret"), f.home.join("code/out")).unwrap();
        let p = f.home.join("code/out");
        assert_eq!(
            resolve_destination(&f.home, p.to_str().unwrap()),
            Err(Refusal::Outside)
        );
    }

    #[test]
    fn a_deleted_or_relative_or_file_folder_is_refused() {
        let f = Fixture::new("gone");
        let p = f.home.join("code/app");
        fs::remove_dir(&p).unwrap();
        assert_eq!(
            resolve_destination(&f.home, p.to_str().unwrap()),
            Err(Refusal::Missing)
        );
        assert_eq!(resolve_destination(&f.home, "code"), Err(Refusal::Missing));
        fs::write(f.home.join("file"), "x").unwrap();
        let file = f.home.join("file");
        assert_eq!(
            resolve_destination(&f.home, file.to_str().unwrap()),
            Err(Refusal::NotDir)
        );
    }

    #[test]
    fn the_live_folder_is_that_of_the_window_and_a_cut_path_is_refused() {
        let long = format!("/{}", "a".repeat(255));
        let ws = [window("@1", "/home/u/a"), window("@2", &long)];
        assert_eq!(live_folder(&ws, "@1"), Ok("/home/u/a"));
        assert_eq!(live_folder(&ws, "@2"), Err(Refusal::TooLong));
        assert_eq!(live_folder(&ws, "@9"), Err(Refusal::NoWindow));
        assert_eq!(live_folder(&ws, ""), Err(Refusal::NoWindow));
    }

    #[test]
    fn every_bad_name_class_is_refused_and_a_good_name_is_not() {
        for good in [
            "a.png",
            "image (1).png",
            "ünï.txt",
            "no-extension",
            "a b",
            &"x".repeat(255),
        ] {
            assert_eq!(check_name(good), Ok(()), "{good}");
        }
        let long = "x".repeat(256);
        for bad in [
            "", ".", "..", ".hidden", "a/b", "a\\b", "a\0b", "a\nb", "a\u{7f}b", " a", "a ", "\ta",
            &long,
        ] {
            assert!(matches!(check_name(bad), Err(Refusal::Name(_))), "{bad:?}");
        }
    }
}
