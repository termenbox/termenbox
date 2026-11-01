//! The machines of every live session, one per (workspace, tab), and the transcript tails that feed them. Evidence
//! sources (hook route, pane scan, transcript) call `feed`; nothing sets a status. Through B.6a nothing reads the
//! machines except tests: the old paths still decide what clients see.
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;

use super::event::Event;
use super::machine::{Effect, Machine, View};
use crate::session_log::observe::Tail;
use crate::session_log::LogError;

type Key = (String, String);

/// An effect with the session it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    pub workspace: String,
    pub tab: String,
    pub effect: Effect,
}

struct Followed {
    path: PathBuf,
    tail: Tail,
}

#[derive(Default)]
pub struct Machines {
    machines: Mutex<HashMap<Key, Machine>>,
    tails: Mutex<HashMap<Key, Followed>>,
    /// Sessions whose Attention the machine owns: a Claude hook has reached them (plan B.6b-2).
    backed: Mutex<HashSet<Key>>,
}

impl Machines {
    /// Applies one event to the machine of `(workspace, tab)`, creating it on first use.
    pub fn feed(&self, workspace: &str, tab: &str, event: Event, now: u64) -> Vec<Effect> {
        self.machines
            .lock()
            .unwrap()
            .entry((workspace.to_string(), tab.to_string()))
            .or_default()
            .apply(event, now)
    }

    /// Marks a session as machine-backed: the legacy writers stop raising and clearing its Attention.
    pub fn mark_backed(&self, workspace: &str, tab: &str) {
        self.backed
            .lock()
            .unwrap()
            .insert((workspace.to_string(), tab.to_string()));
    }

    pub fn is_backed(&self, workspace: &str, tab: &str) -> bool {
        self.backed
            .lock()
            .unwrap()
            .contains(&(workspace.to_string(), tab.to_string()))
    }

    pub fn view(&self, workspace: &str, tab: &str, now: u64) -> Option<View> {
        let machines = self.machines.lock().unwrap();
        machines
            .get(&(workspace.to_string(), tab.to_string()))
            .map(|m| m.view(now))
    }

    /// The views of every machine in `workspace`, one per tab.
    pub fn views_for(&self, workspace: &str, now: u64) -> Vec<View> {
        self.machines
            .lock()
            .unwrap()
            .iter()
            .filter(|((w, _), _)| w == workspace)
            .map(|(_, m)| m.view(now))
            .collect()
    }

    /// Lets the quiet window and the retry threshold lapse on every machine.
    pub fn tick(&self, now: u64) -> Vec<Routed> {
        let mut out = Vec::new();
        for ((workspace, tab), m) in self.machines.lock().unwrap().iter_mut() {
            for effect in m.tick(now) {
                out.push(Routed {
                    workspace: workspace.clone(),
                    tab: tab.clone(),
                    effect,
                });
            }
        }
        out
    }

    pub fn forget_tab(&self, workspace: &str, tab: &str) {
        let key = (workspace.to_string(), tab.to_string());
        self.machines.lock().unwrap().remove(&key);
        self.tails.lock().unwrap().remove(&key);
        self.backed.lock().unwrap().remove(&key);
    }

    /// Drops the sessions of workspaces under `prefix` that are not in `live`.
    pub fn retain_live(&self, prefix: &str, live: &[String]) {
        let keep = |(w, _): &Key| !w.starts_with(prefix) || live.contains(w);
        self.machines.lock().unwrap().retain(|k, _| keep(k));
        self.tails.lock().unwrap().retain(|k, _| keep(k));
        self.backed.lock().unwrap().retain(|k| keep(k));
    }

    /// Follows the transcript a hook named for this session: a new path starts a tail at the end of the file, so only
    /// what is appended from now on is read (the hooks cover the past). The path is already resolved under the root.
    pub fn follow(&self, workspace: &str, tab: &str, path: PathBuf) {
        let key = (workspace.to_string(), tab.to_string());
        let mut tails = self.tails.lock().unwrap();
        if tails.get(&key).is_some_and(|f| f.path == path) {
            return;
        }
        let Ok(len) = std::fs::metadata(&path).map(|m| m.len()) else {
            return;
        };
        tails.insert(
            key,
            Followed {
                path,
                tail: Tail::from(len),
            },
        );
    }

    /// Reads what each followed transcript gained and feeds it to its machine. Blocking file reads: call it from a
    /// blocking thread. An unreadable or replaced file is dropped from the tails (the hook and the screen carry on)
    /// and logged without content.
    pub fn poll_transcripts(&self, now: u64) -> Vec<Routed> {
        let mut batches: Vec<(Key, Vec<Event>)> = Vec::new();
        {
            let mut tails = self.tails.lock().unwrap();
            tails.retain(|key, f| {
                let read = File::open(&f.path)
                    .map_err(|_| LogError::Disabled("unreadable"))
                    .and_then(|file| f.tail.read(&file));
                match read {
                    Ok(events) => {
                        if !events.is_empty() {
                            batches.push((key.clone(), events));
                        }
                        true
                    }
                    Err(e) => {
                        tracing::info!(target: "audit", event = "transcript_tail_dropped", session = %key.0, reason = %e);
                        false
                    }
                }
            });
        }
        let mut out = Vec::new();
        for ((workspace, tab), events) in batches {
            for event in events {
                for effect in self.feed(&workspace, &tab, event, now) {
                    out.push(Routed {
                        workspace: workspace.clone(),
                        tab: tab.clone(),
                        effect,
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::event::{Seen, Source};
    use crate::status::machine::Status;
    use std::io::Write;

    #[test]
    fn a_session_gets_its_own_machine() {
        let m = Machines::default();
        m.feed("w", "t1", Event::TurnStart { title: None }, 1_000);
        m.feed(
            "w",
            "t2",
            Event::Observe {
                source: Source::Screen,
                seen: Seen::Idle,
            },
            1_000,
        );
        assert_eq!(m.view("w", "t1", 1_000).unwrap().status, Status::Working);
        assert_ne!(m.view("w", "t2", 1_000).unwrap().status, Status::Working);
        assert!(m.view("w", "t3", 1_000).is_none());
        m.forget_tab("w", "t1");
        assert!(m.view("w", "t1", 1_000).is_none());
    }

    #[test]
    fn a_followed_transcript_feeds_only_what_is_appended() {
        let dir = std::env::temp_dir().join(format!("tb-feed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"old\"}}\n",
        )
        .unwrap();
        let m = Machines::default();
        m.follow("w", "t", path.clone());
        m.poll_transcripts(1_000);
        assert!(
            m.view("w", "t", 1_000).is_none(),
            "the past is not replayed"
        );
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(
            f,
            "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"go\"}}}}"
        )
        .unwrap();
        m.poll_transcripts(2_000);
        assert!(
            m.view("w", "t", 2_000).is_some(),
            "the new line reached the machine"
        );
        std::fs::remove_file(&path).unwrap();
        m.poll_transcripts(3_000);
        m.follow("w", "t", path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
