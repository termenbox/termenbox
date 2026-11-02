//! Warm standby conductor (plan step 2.8, `conductor.md` section 8): the decision logic only. The active conductor
//! appends its operator state to a log with a monotone `lsn`; the standby applies it in order and holds no node
//! links. It takes over only when two views agree the active is gone, and takes a higher conductor epoch, which a
//! node trusts over any lower one (`Lease`). The links that carry the log and the node's dial to the standby are not
//! built here.

use std::collections::{BTreeMap, VecDeque};

/// How long the active may be silent before the standby counts its own view as "gone" (three heartbeats).
pub const TAKEOVER_AFTER_MS: u64 = 90_000;
/// How long a node's `PeerReport` stays a valid second view.
pub const REPORT_TTL_MS: u64 = 2 * TAKEOVER_AFTER_MS;
/// Applied answer keys a node remembers, shared by whichever conductor delivered them.
const APPLIED: usize = 256;

/// One change of the operator state. Opaque `Rules` stands for the later rules and contracts of slices 6 and 7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    NodeSet { node: String, name: String },
    NodeRemoved { node: String },
    Rules(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub lsn: u64,
    pub op: Op,
}

/// The operator state both conductors hold: the node registry and the opaque rules.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    pub nodes: BTreeMap<String, String>,
    pub rules: Option<String>,
}

impl State {
    fn apply(&mut self, op: &Op) {
        match op {
            Op::NodeSet { node, name } => {
                self.nodes.insert(node.clone(), name.clone());
            }
            Op::NodeRemoved { node } => {
                self.nodes.remove(node);
            }
            Op::Rules(r) => self.rules = Some(r.clone()),
        }
    }
}

/// The active side: numbers each change and keeps the tail a standby may ask for again.
#[derive(Debug, Default)]
pub struct Log {
    state: State,
    last: u64,
    tail: VecDeque<Entry>,
}

impl Log {
    pub fn append(&mut self, op: Op) -> Entry {
        self.last += 1;
        self.state.apply(&op);
        let e = Entry { lsn: self.last, op };
        self.tail.push_back(e.clone());
        while self.tail.len() > 1024 {
            self.tail.pop_front();
        }
        e
    }

    /// Everything after `have`, or the whole state as a snapshot when the tail no longer reaches back that far.
    pub fn since(&self, have: u64) -> Sync {
        match self.tail.front() {
            Some(first) if first.lsn > have + 1 => Sync::Snapshot {
                lsn: self.last,
                state: self.state.clone(),
            },
            _ => Sync::Entries(self.tail.iter().filter(|e| e.lsn > have).cloned().collect()),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sync {
    Entries(Vec<Entry>),
    Snapshot { lsn: u64, state: State },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    Done,
    /// Already applied; ignored.
    Duplicate,
    /// An entry was skipped: ask the active for everything after `have`.
    Gap {
        have: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Standby,
    Active,
}

/// A conductor's failover state. The standby starts in `Standby`; a returning old active that sees a higher epoch
/// is demoted with `observe_epoch`.
#[derive(Debug)]
pub struct Failover {
    role: Role,
    epoch: u64,
    state: State,
    lsn: u64,
    /// When the active was last heard on the replication link.
    heard: u64,
    /// Node id -> when it reported it cannot reach the active.
    reports: BTreeMap<String, u64>,
    incidents: Vec<String>,
}

impl Failover {
    pub fn standby(now: u64, epoch: u64) -> Self {
        Self {
            role: Role::Standby,
            epoch,
            state: State::default(),
            lsn: 0,
            heard: now,
            reports: BTreeMap::new(),
            incidents: Vec::new(),
        }
    }

    pub fn active(epoch: u64) -> Self {
        Self {
            role: Role::Active,
            ..Self::standby(0, epoch)
        }
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn incidents(&self) -> &[String] {
        &self.incidents
    }

    /// The active spoke (a heartbeat or an entry).
    pub fn heard_active(&mut self, now: u64) {
        self.heard = now;
        self.reports.clear();
    }

    pub fn apply(&mut self, e: &Entry, now: u64) -> Applied {
        self.heard_active(now);
        if e.lsn <= self.lsn {
            return Applied::Duplicate;
        }
        if e.lsn != self.lsn + 1 {
            return Applied::Gap { have: self.lsn };
        }
        self.state.apply(&e.op);
        self.lsn = e.lsn;
        Applied::Done
    }

    pub fn sync(&mut self, s: Sync, now: u64) -> Applied {
        match s {
            Sync::Snapshot { lsn, state } => {
                self.heard_active(now);
                self.state = state;
                self.lsn = lsn;
                Applied::Done
            }
            Sync::Entries(es) => {
                let mut out = Applied::Duplicate;
                for e in &es {
                    match self.apply(e, now) {
                        Applied::Gap { have } => return Applied::Gap { have },
                        Applied::Done => out = Applied::Done,
                        Applied::Duplicate => {}
                    }
                }
                out
            }
        }
    }

    /// A node dialled this standby because it could not reach the active.
    pub fn peer_report(&mut self, node: &str, now: u64) {
        self.reports.insert(node.to_string(), now);
    }

    /// Both views: the replication link has been silent for the window, and a node has reported the same.
    pub fn should_take_over(&self, now: u64) -> bool {
        self.role == Role::Standby
            && now.saturating_sub(self.heard) >= TAKEOVER_AFTER_MS
            && self
                .reports
                .values()
                .any(|at| now.saturating_sub(*at) <= REPORT_TTL_MS)
    }

    /// Takes over if both views agree. Returns the new conductor epoch, which is what a node must see in `Welcome`.
    pub fn take_over(&mut self, now: u64) -> Option<u64> {
        if !self.should_take_over(now) {
            return None;
        }
        self.role = Role::Active;
        self.epoch += 1;
        self.incidents.push("conductor failed over".to_string());
        Some(self.epoch)
    }

    /// A peer announced `epoch`. A higher one means someone took over: an active steps down to standby.
    pub fn observe_epoch(&mut self, epoch: u64, now: u64) {
        if epoch > self.epoch {
            self.epoch = epoch;
            if self.role == Role::Active {
                self.role = Role::Standby;
                self.heard = now;
                self.reports.clear();
                self.incidents.push("conductor stepped down".to_string());
            }
        }
    }
}

/// The node's view of which conductor to trust: the highest `conductor_epoch` it has seen wins, a lower one is
/// refused, so a returning old active cannot act after the standby took over.
#[derive(Debug, Default)]
pub struct Lease {
    seen: u64,
}

impl Lease {
    pub fn accept(&mut self, conductor_epoch: u64) -> bool {
        if conductor_epoch < self.seen {
            return false;
        }
        self.seen = conductor_epoch;
        true
    }
}

/// Answer keys a node has applied, whichever conductor delivered them.
#[derive(Debug, Default)]
pub struct AppliedKeys {
    keys: VecDeque<String>,
}

impl AppliedKeys {
    /// True the first time a key is seen.
    pub fn once(&mut self, idem: &str) -> bool {
        if self.keys.iter().any(|k| k == idem) {
            return false;
        }
        self.keys.push_back(idem.to_string());
        while self.keys.len() > APPLIED {
            self.keys.pop_front();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(n: &str) -> Op {
        Op::NodeSet {
            node: n.into(),
            name: format!("name-{n}"),
        }
    }

    #[test]
    fn conductor_standby_replays_the_active_in_order() {
        let mut log = Log::default();
        let mut sb = Failover::standby(0, 1);
        for n in ["a", "b"] {
            let e = log.append(set(n));
            assert_eq!(sb.apply(&e, 1_000), Applied::Done);
        }
        let e = log.append(Op::NodeRemoved { node: "a".into() });
        assert_eq!(sb.apply(&e, 2_000), Applied::Done);
        assert_eq!(sb.state(), log.state());
        // A redelivery changes nothing.
        assert_eq!(sb.apply(&e, 2_100), Applied::Duplicate);
        assert_eq!(sb.state(), log.state());
    }

    #[test]
    fn conductor_standby_asks_again_after_a_gap_and_takes_a_snapshot_past_the_tail() {
        let mut log = Log::default();
        let mut sb = Failover::standby(0, 1);
        let first = log.append(set("a"));
        let _lost = log.append(set("b"));
        let third = log.append(Op::Rules("r".into()));
        assert_eq!(sb.apply(&first, 1), Applied::Done);
        let Applied::Gap { have } = sb.apply(&third, 2) else {
            panic!("a skipped entry must be a gap")
        };
        assert_eq!(have, 1);
        assert_eq!(sb.sync(log.since(have), 3), Applied::Done);
        assert_eq!(sb.state(), log.state());

        // The tail only reaches back so far: an old standby gets the whole state.
        let mut log = Log::default();
        for i in 0..1100 {
            log.append(set(&i.to_string()));
        }
        let mut late = Failover::standby(0, 1);
        assert!(matches!(log.since(0), Sync::Snapshot { .. }));
        assert_eq!(late.sync(log.since(0), 5), Applied::Done);
        assert_eq!(late.state(), log.state());
    }

    #[test]
    fn conductor_standby_needs_both_views_before_it_takes_over() {
        let mut sb = Failover::standby(0, 1);
        // Its own link is silent but no node reports: a partition between the two conductors alone.
        assert!(!sb.should_take_over(TAKEOVER_AFTER_MS + 1));
        assert_eq!(sb.take_over(TAKEOVER_AFTER_MS + 1), None);
        // A node reports but the active is still heard.
        let mut sb = Failover::standby(0, 1);
        sb.heard_active(80_000);
        sb.peer_report("n1", 85_000);
        assert!(!sb.should_take_over(95_000));
        // Both views, and the window has passed.
        let mut sb = Failover::standby(0, 1);
        sb.peer_report("n1", 80_000);
        assert!(!sb.should_take_over(TAKEOVER_AFTER_MS - 1));
        assert_eq!(sb.take_over(TAKEOVER_AFTER_MS), Some(2));
        assert_eq!(sb.role(), Role::Active);
        assert_eq!(sb.incidents(), ["conductor failed over"]);
        // It cannot take over twice.
        assert_eq!(sb.take_over(TAKEOVER_AFTER_MS * 2), None);
    }

    #[test]
    fn conductor_standby_a_stale_report_does_not_count() {
        let mut sb = Failover::standby(0, 1);
        sb.peer_report("n1", 0);
        assert!(!sb.should_take_over(REPORT_TTL_MS + 1));
    }

    #[test]
    fn conductor_standby_the_old_active_steps_down_and_a_node_refuses_it() {
        let mut standby = Failover::standby(0, 1);
        standby.peer_report("n1", 1_000);
        let new_epoch = standby.take_over(TAKEOVER_AFTER_MS).unwrap();

        let mut old = Failover::active(1);
        old.observe_epoch(new_epoch, 200_000);
        assert_eq!(old.role(), Role::Standby);
        assert_eq!(old.epoch(), new_epoch);

        let mut lease = Lease::default();
        assert!(lease.accept(1));
        assert!(lease.accept(new_epoch));
        // The returning old active is ignored by a node that has seen the takeover.
        assert!(!lease.accept(1));
        assert!(lease.accept(new_epoch));
    }

    #[test]
    fn conductor_standby_an_answer_made_during_failover_is_applied_once() {
        let mut applied = AppliedKeys::default();
        // The old active delivered it, the link broke, the device sent it again through the new active.
        let key = "node1:phone-tap-7";
        assert!(applied.once(key));
        assert!(!applied.once(key));
        assert!(applied.once("node1:phone-tap-8"));
    }
}
