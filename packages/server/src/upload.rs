//! File upload to a tab's working folder (decision 0194). This part decides where a file may go and what it may be
//! called; the writer follows. The client never names a folder: the server reads the tab's live one, canonicalises it,
//! and refuses unless it is `$HOME` or below.

use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use termenbox_core::protocol::{
    ServerMessage, UploadCollision, UploadFrame, UPLOAD_FREE_DISK_FLOOR, UPLOAD_MAX_BATCH,
    UPLOAD_RESUME_GRACE_SECS, UPLOAD_STARTS_PER_MINUTE, UPLOAD_WINDOW_BYTES,
};
use tracing::info;

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

/// Who asks: the signed-in account (provider and id), and whether it is the owner (decision 0094: a guest is not).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Who {
    pub provider: String,
    pub account: String,
    pub owner: bool,
}

impl Who {
    fn key(&self) -> String {
        format!("{}:{}", self.provider, self.account)
    }
}

/// One upload that has started and not finished: the pinned folder, the hidden partial file and how far it got. It
/// outlives its connection for the grace period so a dropped link can resume.
struct Entry {
    id: u32,
    owner: String,
    /// The connection driving it; `None` while detached.
    conn: Option<u64>,
    detached: Option<Instant>,
    dir: PathBuf,
    name: String,
    part: PathBuf,
    file: File,
    size: u64,
    received: u64,
    acked: u64,
    collision: UploadCollision,
}

struct State {
    entries: HashMap<(String, u32), Entry>,
    /// When each recent `upload_start` came, for the per-minute limit.
    starts: VecDeque<Instant>,
}

/// The uploads of this server (decision 0194). All methods block on the disk: call them from `spawn_blocking`.
pub struct Uploads {
    home: Option<PathBuf>,
    floor: u64,
    grace: Duration,
    state: Mutex<State>,
    /// The audit lines, as written, kept for tests.
    #[cfg(test)]
    audited: Mutex<Vec<String>>,
}

fn denied(id: u32, reason: &str) -> ServerMessage {
    ServerMessage::UploadDenied {
        id,
        reason: reason.into(),
    }
}

fn failed(id: u32, reason: &str) -> ServerMessage {
    ServerMessage::UploadFailed {
        id,
        reason: reason.into(),
    }
}

/// Free bytes for an unprivileged writer on the filesystem holding `dir`.
fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut v: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `v` a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut v) } != 0 {
        return None;
    }
    Some((v.f_bavail as u64).saturating_mul(v.f_frsize as u64))
}

/// `name`, then `stem (1).ext`, `stem (2).ext`, ...
fn numbered(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

impl Uploads {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self::with_limits(
            home,
            UPLOAD_FREE_DISK_FLOOR,
            Duration::from_secs(UPLOAD_RESUME_GRACE_SECS),
        )
    }

    pub fn with_limits(home: Option<PathBuf>, floor: u64, grace: Duration) -> Self {
        Uploads {
            home,
            floor,
            grace,
            state: Mutex::new(State {
                entries: HashMap::new(),
                starts: VecDeque::new(),
            }),
            #[cfg(test)]
            audited: Mutex::default(),
        }
    }

    pub fn grace(&self) -> Duration {
        self.grace
    }

    /// One audit line: time (the logger's), account, a hash of the folder, a hash of the name and the size. Never the
    /// name, the path or any content (`audit-logging-no-secrets`).
    fn audit(&self, event: &str, who: &Who, dir: &Path, name: &str, size: u64) {
        let folder = crate::agentwatch::hex8(&dir.to_string_lossy());
        let file = crate::agentwatch::hex8(name);
        info!(target: "audit", event, provider = %who.provider, account_id = %who.account, folder_hash = %folder, name_hash = %file, size);
        #[cfg(test)]
        self.audited.lock().unwrap().push(format!(
            "{event} {} {} {folder} {file} {size}",
            who.provider, who.account
        ));
    }

    fn refuse(&self, who: &Who, id: u32, why: &str, name: &str, size: u64) -> ServerMessage {
        self.audit("upload_refused", who, Path::new(""), name, size);
        denied(id, why)
    }

    /// Handles `upload_start`. `live` is the tab's current folder as the caller read it, or why it could not.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &self,
        who: &Who,
        conn: u64,
        id: u32,
        live: Result<String, Refusal>,
        name: &str,
        size: u64,
        collision: UploadCollision,
        now: Instant,
    ) -> ServerMessage {
        if !who.owner {
            return self.refuse(who, id, "Only the owner can upload files", name, size);
        }
        let mut st = self.state.lock().unwrap();
        while st
            .starts
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
        {
            st.starts.pop_front();
        }
        if st.starts.len() >= UPLOAD_STARTS_PER_MINUTE as usize {
            drop(st);
            return self.refuse(
                who,
                id,
                "Too many uploads started; wait a minute",
                name,
                size,
            );
        }
        st.starts.push_back(now);
        let key = who.key();
        if st.entries.values().filter(|e| e.owner == key).count() >= UPLOAD_MAX_BATCH {
            drop(st);
            return self.refuse(who, id, "Too many uploads are waiting", name, size);
        }
        if st.entries.values().any(|e| e.conn == Some(conn))
            || st.entries.contains_key(&(key.clone(), id))
        {
            drop(st);
            return self.refuse(who, id, "Another upload is still running", name, size);
        }
        if let Err(r) = check_name(name) {
            drop(st);
            return self.refuse(who, id, r.reason(), name, size);
        }
        let Some(home) = self.home.as_deref() else {
            drop(st);
            return self.refuse(who, id, Refusal::HomeUnknown.reason(), name, size);
        };
        let dir = match live.and_then(|l| resolve_destination(home, &l)) {
            Ok(d) => d,
            Err(r) => {
                drop(st);
                return self.refuse(who, id, r.reason(), name, size);
            }
        };
        if collision == UploadCollision::Skip && std::fs::symlink_metadata(dir.join(name)).is_ok() {
            drop(st);
            return self.refuse(
                who,
                id,
                "A file with that name is already there; skipped",
                name,
                size,
            );
        }
        match free_bytes(&dir) {
            Some(free) if free.saturating_sub(size) >= self.floor => {}
            _ => {
                drop(st);
                return self.refuse(
                    who,
                    id,
                    "Not enough free disk space on the server",
                    name,
                    size,
                );
            }
        }
        let (part, file) = match create_part(&dir) {
            Ok(p) => p,
            Err(_) => {
                drop(st);
                return self.refuse(who, id, "The folder cannot be written to", name, size);
            }
        };
        let reply = ServerMessage::UploadReady {
            id,
            dir: dir.to_string_lossy().into_owned(),
            offset: 0,
        };
        st.entries.insert(
            (key, id),
            Entry {
                id,
                owner: who.key(),
                conn: Some(conn),
                detached: None,
                dir: dir.clone(),
                name: name.to_string(),
                part,
                file,
                size,
                received: 0,
                acked: 0,
                collision,
            },
        );
        drop(st);
        self.audit("upload_started", who, &dir, name, size);
        reply
    }

    /// Handles `upload_resume`: the entry, if it is still kept, moves to this connection and says where to continue.
    pub fn resume(&self, who: &Who, conn: u64, id: u32, now: Instant) -> ServerMessage {
        let mut st = self.state.lock().unwrap();
        self.sweep_locked(&mut st, now);
        if !who.owner || st.entries.values().any(|e| e.conn == Some(conn)) {
            return failed(id, "That upload cannot be resumed");
        }
        let Some(e) = st.entries.get_mut(&(who.key(), id)) else {
            return failed(id, "That upload is no longer kept; start it again");
        };
        e.conn = Some(conn);
        e.detached = None;
        let reply = ServerMessage::UploadReady {
            id,
            dir: e.dir.to_string_lossy().into_owned(),
            offset: e.received,
        };
        let (dir, name, size) = (e.dir.clone(), e.name.clone(), e.size);
        drop(st);
        self.audit("upload_resumed", who, &dir, &name, size);
        reply
    }

    /// Handles one binary chunk; a reply when the client is owed one.
    pub fn chunk(&self, who: &Who, conn: u64, bytes: &[u8]) -> Option<ServerMessage> {
        let frame = UploadFrame::decode(bytes).ok()?;
        let mut st = self.state.lock().unwrap();
        let key = (who.key(), frame.id);
        let e = st.entries.get_mut(&key).filter(|e| e.conn == Some(conn))?;
        let end = e.received + frame.data.len() as u64;
        let problem = if frame.offset != e.received {
            Some("The upload arrived out of order")
        } else if end > e.size {
            Some("The upload is larger than it said")
        } else if free_bytes(&e.dir)
            .is_none_or(|f| f.saturating_sub(frame.data.len() as u64) < self.floor)
        {
            Some("The server ran out of free disk space")
        } else if e.file.write_all_at(&frame.data, frame.offset).is_err() {
            Some("The file could not be written")
        } else {
            None
        };
        if let Some(why) = problem {
            let e = st.entries.remove(&key).unwrap();
            drop(st);
            return Some(self.discard(who, e, why));
        }
        e.received = end;
        if e.received == e.size || e.received - e.acked >= (UPLOAD_WINDOW_BYTES / 4) as u64 {
            e.acked = e.received;
            return Some(ServerMessage::UploadProgress {
                id: frame.id,
                received: e.received,
            });
        }
        None
    }

    /// Deletes the partial of an entry already taken out of the table and reports why.
    fn discard(&self, who: &Who, e: Entry, why: &str) -> ServerMessage {
        let _ = std::fs::remove_file(&e.part);
        self.audit("upload_failed", who, &e.dir, &e.name, e.size);
        failed(e.id, why)
    }

    /// Handles `upload_finish`: moves the partial into place under the chosen collision rule.
    pub fn finish(&self, who: &Who, conn: u64, id: u32) -> ServerMessage {
        let mut st = self.state.lock().unwrap();
        let key = (who.key(), id);
        if !st.entries.get(&key).is_some_and(|e| e.conn == Some(conn)) {
            return failed(id, "That upload is not running");
        }
        let e = st.entries.remove(&key).unwrap();
        drop(st);
        if e.received != e.size {
            return self.discard(who, e, "The file is incomplete");
        }
        let _ = e.file.sync_all();
        let moved = match e.collision {
            UploadCollision::Replace => {
                let to = e.dir.join(&e.name);
                std::fs::rename(&e.part, &to).map(|_| to)
            }
            UploadCollision::Skip if std::fs::symlink_metadata(e.dir.join(&e.name)).is_ok() => {
                let _ = std::fs::remove_file(&e.part);
                self.audit("upload_cancelled", who, &e.dir, &e.name, e.size);
                return ServerMessage::UploadCancelled { id };
            }
            // Keep both, and Skip when the name is still free: a hard link never replaces an existing name.
            _ => link_free(&e.part, &e.dir, &e.name).inspect(|_| {
                let _ = std::fs::remove_file(&e.part);
            }),
        };
        match moved {
            Ok(path) => {
                self.audit("upload_done", who, &e.dir, &e.name, e.size);
                ServerMessage::UploadDone {
                    id,
                    path: path.to_string_lossy().into_owned(),
                    size: e.size,
                }
            }
            Err(_) => self.discard(who, e, "The file could not be put in place"),
        }
    }

    /// Handles `upload_cancel`: the partial goes at once.
    pub fn cancel(&self, who: &Who, id: u32) -> ServerMessage {
        let e = self.state.lock().unwrap().entries.remove(&(who.key(), id));
        if let Some(e) = e {
            let _ = std::fs::remove_file(&e.part);
            self.audit("upload_cancelled", who, &e.dir, &e.name, e.size);
        }
        ServerMessage::UploadCancelled { id }
    }

    /// The connection `conn` ended: its uploads are kept for the grace period.
    pub fn detach(&self, conn: u64, now: Instant) {
        for e in self.state.lock().unwrap().entries.values_mut() {
            if e.conn == Some(conn) {
                e.conn = None;
                e.detached = Some(now);
            }
        }
    }

    /// Deletes the partials whose grace period is over.
    pub fn sweep(&self, now: Instant) {
        let mut st = self.state.lock().unwrap();
        self.sweep_locked(&mut st, now);
    }

    fn sweep_locked(&self, st: &mut State, now: Instant) {
        let grace = self.grace;
        let gone: Vec<(String, u32)> = st
            .entries
            .iter()
            .filter(|(_, e)| e.detached.is_some_and(|t| now.duration_since(t) >= grace))
            .map(|(k, _)| k.clone())
            .collect();
        for k in gone {
            if let Some(e) = st.entries.remove(&k) {
                let _ = std::fs::remove_file(&e.part);
                let (provider, account) = e.owner.split_once(':').unwrap_or(("", ""));
                let who = Who {
                    provider: provider.into(),
                    account: account.into(),
                    owner: true,
                };
                self.audit("upload_failed", &who, &e.dir, &e.name, e.size);
            }
        }
    }
}

/// A hidden `.tb-upload-<random>.part` in `dir`, created new with mode 0600 (never executable, never through a link).
fn create_part(dir: &Path) -> std::io::Result<(PathBuf, File)> {
    use rand::Rng;
    for _ in 0..8 {
        let tag: u64 = rand::thread_rng().gen();
        let path = dir.join(format!(".tb-upload-{tag:016x}.part"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(ErrorKind::AlreadyExists.into())
}

/// Hard-links `part` to the first free `name`, `name (1)`, ...; an existing name is never replaced.
fn link_free(part: &Path, dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    for n in 0..1000 {
        let to = dir.join(numbered(name, n));
        match std::fs::hard_link(part, &to) {
            Ok(()) => return Ok(to),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(ErrorKind::AlreadyExists.into())
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

#[cfg(test)]
mod writer {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("tb-upwr-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            for d in ["home/code", "home/other", "outside"] {
                fs::create_dir_all(root.join(d)).unwrap();
            }
            let root = fs::canonicalize(root).unwrap();
            let home = root.join("home");
            Fixture { root, home }
        }

        fn dir(&self) -> String {
            self.home.join("code").to_string_lossy().into_owned()
        }

        fn uploads(&self) -> Uploads {
            Uploads::with_limits(Some(self.home.clone()), 0, Duration::from_secs(600))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn owner() -> Who {
        Who {
            provider: "github".into(),
            account: "42".into(),
            owner: true,
        }
    }

    fn start(
        u: &Uploads,
        f: &Fixture,
        id: u32,
        name: &str,
        size: u64,
        c: UploadCollision,
    ) -> ServerMessage {
        u.start(&owner(), 1, id, Ok(f.dir()), name, size, c, Instant::now())
    }

    fn send(u: &Uploads, id: u32, offset: u64, data: &[u8]) -> Option<ServerMessage> {
        let bytes = UploadFrame {
            id,
            offset,
            data: data.to_vec(),
        }
        .encode()
        .unwrap();
        u.chunk(&owner(), 1, &bytes)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    fn upload(
        u: &Uploads,
        f: &Fixture,
        id: u32,
        name: &str,
        body: &[u8],
        c: UploadCollision,
    ) -> ServerMessage {
        assert!(matches!(
            start(u, f, id, name, body.len() as u64, c),
            ServerMessage::UploadReady { .. }
        ));
        send(u, id, 0, body);
        u.finish(&owner(), 1, id)
    }

    #[test]
    fn upload_a_file_lands_in_the_pinned_folder_with_mode_0600() {
        let f = Fixture::new("flow");
        let u = f.uploads();
        let ready = start(&u, &f, 1, "a.txt", 5, UploadCollision::KeepBoth);
        assert!(
            matches!(&ready, ServerMessage::UploadReady { offset: 0, dir, .. } if *dir == f.dir())
        );
        // While it runs, only the hidden partial exists, and it is 0600.
        let during = names(&f.home.join("code"));
        assert_eq!(during.len(), 1);
        assert!(during[0].starts_with(".tb-upload-") && during[0].ends_with(".part"));
        let mode = fs::metadata(f.home.join("code").join(&during[0]))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o600);
        assert!(matches!(
            send(&u, 1, 0, b"hello"),
            Some(ServerMessage::UploadProgress { received: 5, .. })
        ));
        let done = u.finish(&owner(), 1, 1);
        let path = f.home.join("code/a.txt");
        assert_eq!(
            done,
            ServerMessage::UploadDone {
                id: 1,
                path: path.to_string_lossy().into_owned(),
                size: 5
            }
        );
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        assert_eq!(names(&f.home.join("code")), ["a.txt"]);
    }

    #[test]
    fn upload_a_cd_after_start_does_not_move_the_file() {
        let f = Fixture::new("pinned");
        let u = f.uploads();
        start(&u, &f, 1, "a.txt", 1, UploadCollision::KeepBoth);
        // The tab's shell moves to ~/other now; the upload keeps no reference to the live folder.
        send(&u, 1, 0, b"x");
        u.finish(&owner(), 1, 1);
        assert_eq!(names(&f.home.join("code")), ["a.txt"]);
        assert!(names(&f.home.join("other")).is_empty());
    }

    #[test]
    fn upload_a_guest_is_refused() {
        let f = Fixture::new("guest");
        let u = f.uploads();
        let guest = Who {
            owner: false,
            ..owner()
        };
        let r = u.start(
            &guest,
            1,
            1,
            Ok(f.dir()),
            "a",
            1,
            UploadCollision::KeepBoth,
            Instant::now(),
        );
        assert!(matches!(r, ServerMessage::UploadDenied { .. }));
        assert!(names(&f.home.join("code")).is_empty());
    }

    #[test]
    fn upload_refuses_a_bad_name_a_bad_folder_and_a_missing_tab() {
        let f = Fixture::new("refuse");
        let u = f.uploads();
        for bad in ["../x", ".hidden", "a/b", ""] {
            assert!(
                matches!(
                    start(&u, &f, 1, bad, 1, UploadCollision::KeepBoth),
                    ServerMessage::UploadDenied { .. }
                ),
                "{bad}"
            );
        }
        let outside = f.root.join("outside").to_string_lossy().into_owned();
        let r = u.start(
            &owner(),
            1,
            2,
            Ok(outside),
            "a",
            1,
            UploadCollision::KeepBoth,
            Instant::now(),
        );
        assert!(matches!(r, ServerMessage::UploadDenied { .. }));
        let r = u.start(
            &owner(),
            1,
            3,
            Err(Refusal::NoWindow),
            "a",
            1,
            UploadCollision::KeepBoth,
            Instant::now(),
        );
        assert!(matches!(r, ServerMessage::UploadDenied { .. }));
    }

    #[test]
    fn upload_keep_both_never_overwrites_and_numbers_the_new_file() {
        let f = Fixture::new("both");
        let u = f.uploads();
        fs::write(f.home.join("code/a.txt"), "old").unwrap();
        assert!(matches!(
            upload(&u, &f, 1, "a.txt", b"new", UploadCollision::KeepBoth),
            ServerMessage::UploadDone { .. }
        ));
        assert!(matches!(
            upload(&u, &f, 2, "a.txt", b"newer", UploadCollision::KeepBoth),
            ServerMessage::UploadDone { .. }
        ));
        assert_eq!(fs::read(f.home.join("code/a.txt")).unwrap(), b"old");
        assert_eq!(fs::read(f.home.join("code/a (1).txt")).unwrap(), b"new");
        assert_eq!(fs::read(f.home.join("code/a (2).txt")).unwrap(), b"newer");
        assert_eq!(names(&f.home.join("code")).len(), 3);
    }

    #[test]
    fn upload_replace_overwrites_but_never_writes_through_a_symlink() {
        let f = Fixture::new("replace");
        let u = f.uploads();
        fs::write(f.home.join("code/a.txt"), "old").unwrap();
        assert!(matches!(
            upload(&u, &f, 1, "a.txt", b"new", UploadCollision::Replace),
            ServerMessage::UploadDone { .. }
        ));
        assert_eq!(fs::read(f.home.join("code/a.txt")).unwrap(), b"new");
        let target = f.root.join("outside/secret");
        fs::write(&target, "keep").unwrap();
        symlink(&target, f.home.join("code/l.txt")).unwrap();
        assert!(matches!(
            upload(&u, &f, 2, "l.txt", b"x", UploadCollision::Replace),
            ServerMessage::UploadDone { .. }
        ));
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        assert!(!fs::symlink_metadata(f.home.join("code/l.txt"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn upload_keep_both_does_not_follow_a_dangling_symlink_in_the_way() {
        let f = Fixture::new("dangling");
        let u = f.uploads();
        symlink(f.root.join("outside/none"), f.home.join("code/a.txt")).unwrap();
        assert!(matches!(
            upload(&u, &f, 1, "a.txt", b"x", UploadCollision::KeepBoth),
            ServerMessage::UploadDone { .. }
        ));
        assert!(!f.root.join("outside/none").exists());
        assert_eq!(fs::read(f.home.join("code/a (1).txt")).unwrap(), b"x");
    }

    #[test]
    fn upload_skip_leaves_an_existing_name_alone() {
        let f = Fixture::new("skip");
        let u = f.uploads();
        fs::write(f.home.join("code/a.txt"), "old").unwrap();
        assert!(matches!(
            start(&u, &f, 1, "a.txt", 1, UploadCollision::Skip),
            ServerMessage::UploadDenied { .. }
        ));
        assert!(matches!(
            upload(&u, &f, 2, "b.txt", b"x", UploadCollision::Skip),
            ServerMessage::UploadDone { .. }
        ));
        assert_eq!(names(&f.home.join("code")), ["a.txt", "b.txt"]);
    }

    #[test]
    fn upload_a_partial_is_gone_after_cancel() {
        let f = Fixture::new("cancel");
        let u = f.uploads();
        start(&u, &f, 1, "a.txt", 10, UploadCollision::KeepBoth);
        send(&u, 1, 0, b"abc");
        assert_eq!(
            u.cancel(&owner(), 1),
            ServerMessage::UploadCancelled { id: 1 }
        );
        assert!(names(&f.home.join("code")).is_empty());
    }

    #[test]
    fn upload_a_partial_is_gone_after_the_grace_period_and_resume_continues_before_it() {
        let f = Fixture::new("grace");
        let u = f.uploads();
        let t0 = Instant::now();
        start(&u, &f, 1, "a.txt", 6, UploadCollision::KeepBoth);
        send(&u, 1, 0, b"abc");
        u.detach(1, t0);
        // A new connection (2) resumes within the grace period, from the server's offset.
        let r = u.resume(&owner(), 2, 1, t0 + Duration::from_secs(599));
        assert!(matches!(
            r,
            ServerMessage::UploadReady {
                id: 1,
                offset: 3,
                ..
            }
        ));
        let bytes = UploadFrame {
            id: 1,
            offset: 3,
            data: b"def".to_vec(),
        }
        .encode()
        .unwrap();
        assert!(u.chunk(&owner(), 2, &bytes).is_some());
        assert!(matches!(
            u.finish(&owner(), 2, 1),
            ServerMessage::UploadDone { .. }
        ));
        assert_eq!(fs::read(f.home.join("code/a.txt")).unwrap(), b"abcdef");
        // Another that nobody resumes is deleted when the period ends, and cannot be resumed.
        start(&u, &f, 2, "b.txt", 6, UploadCollision::KeepBoth);
        u.detach(1, t0);
        u.sweep(t0 + Duration::from_secs(601));
        assert_eq!(names(&f.home.join("code")), ["a.txt"]);
        assert!(matches!(
            u.resume(&owner(), 3, 2, t0 + Duration::from_secs(602)),
            ServerMessage::UploadFailed { .. }
        ));
    }

    #[test]
    fn upload_an_out_of_order_or_oversize_chunk_fails_and_deletes_the_partial() {
        let f = Fixture::new("order");
        let u = f.uploads();
        start(&u, &f, 1, "a.txt", 4, UploadCollision::KeepBoth);
        assert!(matches!(
            send(&u, 1, 2, b"ab"),
            Some(ServerMessage::UploadFailed { id: 1, .. })
        ));
        assert!(names(&f.home.join("code")).is_empty());
        start(&u, &f, 2, "b.txt", 2, UploadCollision::KeepBoth);
        assert!(matches!(
            send(&u, 2, 0, b"abc"),
            Some(ServerMessage::UploadFailed { id: 2, .. })
        ));
        assert!(names(&f.home.join("code")).is_empty());
        // Finishing short fails too.
        start(&u, &f, 3, "c.txt", 4, UploadCollision::KeepBoth);
        send(&u, 3, 0, b"ab");
        assert!(matches!(
            u.finish(&owner(), 1, 3),
            ServerMessage::UploadFailed { .. }
        ));
        assert!(names(&f.home.join("code")).is_empty());
    }

    #[test]
    fn upload_a_start_below_the_disk_floor_is_refused() {
        let f = Fixture::new("floor");
        let u = Uploads::with_limits(Some(f.home.clone()), u64::MAX / 2, Duration::from_secs(600));
        assert!(matches!(
            start(&u, &f, 1, "a.txt", 1, UploadCollision::KeepBoth),
            ServerMessage::UploadDenied { .. }
        ));
        assert!(names(&f.home.join("code")).is_empty());
    }

    #[test]
    fn upload_the_sixty_first_start_in_a_minute_is_refused() {
        let f = Fixture::new("rate");
        let u = f.uploads();
        let t0 = Instant::now();
        for i in 0..60 {
            let r = u.start(
                &owner(),
                1,
                i,
                Ok(f.dir()),
                "a.txt",
                1,
                UploadCollision::KeepBoth,
                t0,
            );
            assert!(matches!(r, ServerMessage::UploadReady { .. }), "{i}");
            u.cancel(&owner(), i);
        }
        let r = u.start(
            &owner(),
            1,
            60,
            Ok(f.dir()),
            "a.txt",
            1,
            UploadCollision::KeepBoth,
            t0,
        );
        assert!(
            matches!(&r, ServerMessage::UploadDenied { reason, .. } if reason.contains("Too many"))
        );
        let later = t0 + Duration::from_secs(61);
        let r = u.start(
            &owner(),
            1,
            61,
            Ok(f.dir()),
            "a.txt",
            1,
            UploadCollision::KeepBoth,
            later,
        );
        assert!(matches!(r, ServerMessage::UploadReady { .. }));
    }

    #[test]
    fn upload_one_connection_runs_one_upload_at_a_time() {
        let f = Fixture::new("one");
        let u = f.uploads();
        start(&u, &f, 1, "a.txt", 1, UploadCollision::KeepBoth);
        assert!(matches!(
            start(&u, &f, 2, "b.txt", 1, UploadCollision::KeepBoth),
            ServerMessage::UploadDenied { .. }
        ));
    }

    #[test]
    fn upload_the_audit_lines_hold_no_name_no_path_and_no_content() {
        let f = Fixture::new("audit");
        let u = f.uploads();
        upload(
            &u,
            &f,
            1,
            "secret-report.txt",
            b"TOPSECRETBODY",
            UploadCollision::KeepBoth,
        );
        start(&u, &f, 2, "other-name.txt", 4, UploadCollision::KeepBoth);
        u.cancel(&owner(), 2);
        let _ = start(&u, &f, 3, "../bad", 1, UploadCollision::KeepBoth);
        let lines = u.audited.lock().unwrap().join("\n");
        assert!(
            lines.contains("upload_started")
                && lines.contains("upload_done")
                && lines.contains("upload_cancelled")
                && lines.contains("upload_refused")
        );
        for secret in [
            "secret-report",
            "other-name",
            "TOPSECRET",
            "bad",
            "code",
            "tb-upwr",
            "/",
        ] {
            assert!(!lines.contains(secret), "{secret} in {lines}");
        }
    }
}
