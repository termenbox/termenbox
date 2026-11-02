//! The conductor side of the node link (decision 0170, `docs/plans/assets/control-plane/conductor.md`).
//!
//! Nodes dial in; this module enrols them by key, merges their attention into one stream, bounds what it holds per
//! node, marks a silent node and routes an answer back to the node that owns the item. It holds attention metadata
//! only (`conductor-links-carry-attention-metadata-only`): terminals cross only through `conductor_terminal` on an
//! explicit open, and the agent text of an item is dropped once the item clears. `Conductor` is plain state driven by messages and a clock, so the transport
//! (`serve`) is thin and the behaviour is tested without sockets.
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use termenbox_core::conductor::{
    cap, AnswerTarget, CardInfo, ClearedBy, ConductorMessage, NodeInfo, NodeMessage, NodeStanding,
    Obligation, MAX_NAME,
};
use termenbox_core::events::AskKind;
use tokio::sync::mpsc::UnboundedSender;

use crate::link::{Inbound, Link};

/// A node that sent nothing for this long is silent (three heartbeats).
pub const SILENT_AFTER_MS: u64 = 90_000;
/// A silent node raises an incident after this long.
pub const SILENT_INCIDENT_MS: u64 = 10 * 60_000;
/// The enrolment secret is valid this long and works once.
pub const ENROL_TTL_MS: u64 = 15 * 60_000;
/// Heartbeat the conductor asks of a node.
pub const HEARTBEAT_S: u32 = 30;
/// What the conductor holds per node, by bytes. The 200-item `MAX_ATTENTION` of a node fits with room to spare.
pub const NODE_BYTES: usize = 256 * 1024;
/// A cleared item is kept this long, then dropped.
const CLEARED_KEEP_MS: u64 = 24 * 3_600_000;
/// Incidents are kept this long, and at most this many per node.
const INCIDENT_KEEP_MS: u64 = 7 * 24 * 3_600_000;
const INCIDENT_MAX: usize = 64;
const FILE: &str = "conductor-nodes.json";

/// An item as held for one node.
#[derive(Debug, Clone)]
struct Held {
    card: String,
    obligation: Obligation,
    cleared: Option<(u64, ClearedBy)>,
    bytes: usize,
}

/// A recorded incident of a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incident {
    pub at: u64,
    pub cause: String,
    pub workspace: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Presented the enrolment secret and waits for the owner's tap; its messages are discarded.
    Pending,
    Enrolled,
}

struct NodeState {
    name: String,
    standing: Standing,
    epoch: u64,
    /// The last `seq` applied; `None` right after a snapshot, which accepts whatever comes next as the baseline.
    last_seq: Option<u64>,
    /// A gap was seen: everything is ignored until the snapshot the conductor asked for arrives.
    awaiting_snapshot: bool,
    last_seen: u64,
    items: BTreeMap<String, Held>,
    bytes: usize,
    dropped: u32,
    incidents: VecDeque<Incident>,
    silent_incident: bool,
    agents: u32,
    working: u32,
    waiting: u32,
    tokens: u64,
    cost_micro: u64,
    sender: Option<UnboundedSender<ConductorMessage>>,
    /// Answers already forwarded, so a redelivery applies once.
    idem: VecDeque<String>,
}

impl NodeState {
    fn new(name: String, standing: Standing, now: u64) -> Self {
        NodeState {
            name,
            standing,
            epoch: 0,
            last_seq: None,
            awaiting_snapshot: false,
            last_seen: now,
            items: BTreeMap::new(),
            bytes: 0,
            dropped: 0,
            incidents: VecDeque::new(),
            silent_incident: false,
            agents: 0,
            working: 0,
            waiting: 0,
            tokens: 0,
            cost_micro: 0,
            sender: None,
            idem: VecDeque::new(),
        }
    }

    fn silent(&self, now: u64) -> bool {
        now.saturating_sub(self.last_seen) >= SILENT_AFTER_MS
    }

    fn incident(&mut self, at: u64, cause: &str, workspace: Option<String>, detail: &str) {
        self.incidents.push_back(Incident {
            at,
            cause: cap(cause, MAX_NAME),
            workspace,
            detail: detail.to_string(),
        });
        while self.incidents.len() > INCIDENT_MAX {
            self.incidents.pop_front();
        }
    }

    /// The text of an open item arrived after it was raised; the card and the id stay, nothing alerts.
    fn fill(&mut self, id: &str, prompt: Option<String>, ask: Option<AskKind>) {
        let Some(h) = self.items.get_mut(id).filter(|h| h.cleared.is_none()) else {
            return;
        };
        let before = h.bytes;
        h.obligation.prompt = prompt;
        h.obligation.ask = ask.or(h.obligation.ask);
        h.bytes = size_of(&h.obligation);
        self.bytes = self.bytes - before + h.bytes;
    }

    fn insert(&mut self, card: String, o: Obligation) {
        let bytes = size_of(&o);
        if let Some(old) = self.items.insert(
            o.id.clone(),
            Held {
                card,
                obligation: o,
                cleared: None,
                bytes,
            },
        ) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
    }

    /// Keeps the node under `NODE_BYTES`: cleared items go first, oldest first, then the oldest open ones with a
    /// count the owner sees as an incident.
    fn bound(&mut self, now: u64) {
        let keep_from = now.saturating_sub(CLEARED_KEEP_MS);
        let stale: Vec<String> = self
            .items
            .iter()
            .filter(|(_, h)| h.cleared.is_some_and(|(at, _)| at < keep_from))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            self.remove(&id);
        }
        let incident_from = now.saturating_sub(INCIDENT_KEEP_MS);
        while self.incidents.front().is_some_and(|i| i.at < incident_from) {
            self.incidents.pop_front();
        }
        let mut lost = 0u32;
        while self.bytes > NODE_BYTES {
            let victim = self
                .items
                .iter()
                .filter(|(_, h)| h.cleared.is_some())
                .min_by_key(|(_, h)| h.cleared.map(|c| c.0))
                .or_else(|| {
                    self.items
                        .iter()
                        .min_by_key(|(_, h)| h.obligation.raised_at)
                })
                .map(|(id, _)| id.clone());
            let Some(id) = victim else { break };
            let was_open = self.items.get(&id).is_some_and(|h| h.cleared.is_none());
            self.remove(&id);
            if was_open {
                lost += 1;
            }
        }
        if lost > 0 {
            self.dropped = self.dropped.saturating_add(lost);
            self.incident(
                now,
                "conductor queue overflow",
                None,
                &format!("{lost} open items dropped to stay under the byte budget"),
            );
        }
    }

    fn remove(&mut self, id: &str) {
        if let Some(h) = self.items.remove(id) {
            self.bytes -= h.bytes;
        }
    }
}

fn size_of(o: &Obligation) -> usize {
    // The held record plus fixed overhead for the map entry and card id.
    serde_json::to_string(o).map_or(0, |s| s.len()) + 96
}

struct Inner {
    nodes: HashMap<String, NodeState>,
    /// SHA-256 of the one-time secret and when it stops working; memory only.
    enrol: Option<(String, u64)>,
    next_card: u64,
    /// Card id to `(node, obligation id)`.
    cards: HashMap<String, (String, String)>,
}

/// Why a node was not admitted. The text is for the log; the node only sees the connection close.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    BadSignature,
    NotEnrolled,
    Unsupported,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Enrolled,
    /// Accepted on a valid secret; waits for the owner's tap.
    Pending,
}

/// Why an answer did not go out.
#[derive(Debug, PartialEq, Eq)]
pub enum AnswerError {
    UnknownCard,
    /// The node is silent: there is nobody to apply the answer, so none is queued.
    NodeAway,
    /// The item already left; the winner is named.
    Cleared(ClearedBy),
    NodeGone,
}

/// One row of the merged stream. The device sees `card`; the node's epoch and ids stay here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub card: String,
    pub node: String,
    pub node_name: String,
    /// Since when the owning node has been silent.
    pub silent_since: Option<u64>,
    pub obligation: Obligation,
}

/// What the owner sees of a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub node: String,
    pub name: String,
    pub standing: Standing,
    pub connected: bool,
    pub silent_since: Option<u64>,
    pub open: usize,
    pub agents: u32,
    pub working: u32,
    pub waiting: u32,
    pub dropped: u32,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    nodes: Vec<SavedNode>,
}

#[derive(Serialize, Deserialize)]
struct SavedNode {
    node: String,
    name: String,
}

pub struct Conductor {
    inner: Mutex<Inner>,
    dir: Option<PathBuf>,
    /// Counts changes, so an owner's connection pushes the fleet when something moved.
    changed: tokio::sync::watch::Sender<u64>,
    /// Terminals opened on nodes by devices; empty unless a device asked (step 2.6).
    terms: crate::conductor_terminal::Terminals,
}

impl Conductor {
    /// A conductor that keeps its enrolled keys in `dir` (the server's config directory), or none in tests.
    pub fn new(dir: Option<PathBuf>, now: u64) -> Self {
        let mut nodes = HashMap::new();
        if let Some(saved) = dir
            .as_ref()
            .and_then(|d| std::fs::read(d.join(FILE)).ok())
            .and_then(|b| serde_json::from_slice::<Saved>(&b).ok())
        {
            for n in saved.nodes {
                nodes.insert(n.node, NodeState::new(n.name, Standing::Enrolled, now));
            }
        }
        Conductor {
            inner: Mutex::new(Inner {
                nodes,
                enrol: None,
                next_card: 1,
                cards: HashMap::new(),
            }),
            dir,
            changed: tokio::sync::watch::channel(0).0,
            terms: Default::default(),
        }
    }

    /// Wakes whenever the fleet or the stream changed.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changed.subscribe()
    }

    fn bump(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    fn save(&self, inner: &Inner) {
        let Some(dir) = &self.dir else { return };
        let saved = Saved {
            nodes: inner
                .nodes
                .iter()
                .filter(|(_, n)| n.standing == Standing::Enrolled)
                .map(|(k, n)| SavedNode {
                    node: k.clone(),
                    name: n.name.clone(),
                })
                .collect(),
        };
        if let Ok(bytes) = serde_json::to_vec(&saved) {
            if let Err(e) = crate::auth::write_private(&dir.join(FILE), &bytes) {
                tracing::warn!("conductor: could not save the enrolled nodes: {e:#}");
            }
        }
    }

    /// A one-time enrolment secret for `--conductor-enrol`; the previous one stops working.
    pub fn mint_enrol(&self, now: u64) -> String {
        let mut bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut bytes);
        let secret = URL_SAFE_NO_PAD.encode(bytes);
        self.inner.lock().unwrap().enrol = Some((hash(&secret), now + ENROL_TTL_MS));
        secret
    }

    /// Checks a node's `hello` against the nonce it was challenged with. A node already enrolled needs only the
    /// signature; a new one also needs the live secret, which is burnt on use.
    pub fn admit(
        &self,
        hello: &NodeMessage,
        nonce: &str,
        now: u64,
    ) -> Result<(String, Admission), Refusal> {
        let NodeMessage::Hello {
            v,
            node,
            name,
            enrol,
            sig,
            ..
        } = hello
        else {
            return Err(Refusal::Unsupported);
        };
        if *v == 0 {
            return Err(Refusal::Unsupported);
        }
        verify(node, nonce, sig).map_err(|_| Refusal::BadSignature)?;
        let mut inner = self.inner.lock().unwrap();
        if inner
            .nodes
            .get(node)
            .is_some_and(|n| n.standing == Standing::Enrolled)
        {
            return Ok((node.clone(), Admission::Enrolled));
        }
        let live = inner.enrol.as_ref().is_some_and(|(h, until)| {
            now <= *until && enrol.as_deref().is_some_and(|s| &hash(s) == h)
        });
        if !live {
            return Err(Refusal::NotEnrolled);
        }
        inner.enrol = None;
        inner.nodes.insert(
            node.clone(),
            NodeState::new(cap(name, MAX_NAME), Standing::Pending, now),
        );
        drop(inner);
        self.bump();
        Ok((node.clone(), Admission::Pending))
    }

    /// The owner's tap: a pending node becomes enrolled and is remembered across restarts.
    pub fn approve(&self, node: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let Some(n) = inner.nodes.get_mut(node) else {
            return false;
        };
        n.standing = Standing::Enrolled;
        // What it said while pending was discarded: ask for its list again.
        if let Some(tx) = &n.sender {
            let _ = tx.send(ConductorMessage::Resync);
        }
        self.save(&inner);
        drop(inner);
        self.bump();
        true
    }

    /// Removes a node and its history; if it is connected it is told to stop dialling.
    pub fn remove(&self, node: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let Some(n) = inner.nodes.remove(node) else {
            return false;
        };
        if let Some(tx) = n.sender {
            let _ = tx.send(ConductorMessage::Disable);
        }
        inner.cards.retain(|_, (k, _)| k != node);
        self.save(&inner);
        drop(inner);
        self.bump();
        true
    }

    /// A node's connection is up. Returns the welcome, and the node's items from an older epoch are dropped since
    /// nobody can answer them any more.
    pub fn attach(
        &self,
        node: &str,
        epoch: u64,
        tx: UnboundedSender<ConductorMessage>,
        now: u64,
    ) -> ConductorMessage {
        let mut inner = self.inner.lock().unwrap();
        let mut live = std::collections::HashSet::new();
        if let Some(n) = inner.nodes.get_mut(node) {
            if n.epoch != epoch {
                n.items.clear();
                n.bytes = 0;
                n.epoch = epoch;
            }
            n.last_seq = None;
            n.awaiting_snapshot = false;
            n.last_seen = now;
            n.silent_incident = false;
            n.sender = Some(tx);
            live = n.items.keys().cloned().collect();
        }
        inner
            .cards
            .retain(|_, (k, id)| k != node || live.contains(id));
        drop(inner);
        self.bump();
        ConductorMessage::Welcome {
            epoch_ack: epoch,
            heartbeat_s: HEARTBEAT_S,
            conductor_epoch: 0,
            standby: None,
        }
    }

    /// The node's connection went away; its items stay, marked silent once the threshold passes.
    pub fn detach(&self, node: &str) {
        if let Some(n) = self.inner.lock().unwrap().nodes.get_mut(node) {
            n.sender = None;
        }
        self.terms.close_node(node);
        self.bump();
    }

    /// Applies one message of an enrolled node. The reply is what goes back to that node (a `resync` on a gap).
    pub fn handle(&self, node: &str, msg: NodeMessage, now: u64) -> Vec<ConductorMessage> {
        match msg {
            // Terminal traffic belongs to a ticket, not to the ordered attention stream.
            NodeMessage::TerminalOutput { ticket, data } => {
                if !self.terms.output(node, &ticket, &data) {
                    self.send_to(node, ConductorMessage::CloseTerminal { ticket });
                }
                return Vec::new();
            }
            NodeMessage::TerminalClosed { ticket } => {
                self.terms.close(&ticket, Some(node), None);
                return Vec::new();
            }
            msg => self.handle_attention(node, msg, now),
        }
    }

    fn handle_attention(&self, node: &str, msg: NodeMessage, now: u64) -> Vec<ConductorMessage> {
        let reply = self.apply_message(node, msg, now);
        self.bump();
        reply
    }

    fn apply_message(&self, node: &str, msg: NodeMessage, now: u64) -> Vec<ConductorMessage> {
        let msg = msg.capped();
        let mut inner = self.inner.lock().unwrap();
        let next = inner.next_card;
        let mut used = 0u64;
        let mut cards = Vec::new();
        let mut reply = Vec::new();
        let Some(n) = inner.nodes.get_mut(node) else {
            return reply;
        };
        // A pending node is not yet trusted: nothing it says is kept.
        if n.standing != Standing::Enrolled {
            return reply;
        }
        n.last_seen = now;
        n.silent_incident = false;

        // `epoch` and `seq` order everything but the snapshot.
        let seqd = match &msg {
            NodeMessage::Raised { epoch, seq, .. }
            | NodeMessage::Cleared { epoch, seq, .. }
            | NodeMessage::Filled { epoch, seq, .. }
            | NodeMessage::Acked { epoch, seq, .. }
            | NodeMessage::Answered { epoch, seq, .. }
            | NodeMessage::Usage { epoch, seq, .. }
            | NodeMessage::Incident { epoch, seq, .. }
            | NodeMessage::Health { epoch, seq, .. } => Some((*epoch, *seq)),
            _ => None,
        };
        if let Some((epoch, seq)) = seqd {
            if epoch != n.epoch || n.awaiting_snapshot {
                return reply;
            }
            match n.last_seq {
                Some(last) if seq <= last => return reply,
                Some(last) if seq > last + 1 => {
                    n.awaiting_snapshot = true;
                    reply.push(ConductorMessage::Resync);
                    return reply;
                }
                _ => {}
            }
            n.last_seq = Some(seq);
        }

        match msg {
            NodeMessage::Snapshot { epoch, items, .. } if epoch == n.epoch => {
                n.awaiting_snapshot = false;
                n.last_seq = None;
                let open: std::collections::HashSet<String> =
                    items.iter().map(|o| o.id.clone()).collect();
                // What the snapshot lacks is gone from the node.
                for (id, h) in n.items.iter_mut() {
                    if h.cleared.is_none() && !open.contains(id) {
                        h.cleared = Some((now, ClearedBy::Fact));
                    }
                }
                for o in items {
                    if n.items.get(&o.id).is_some_and(|h| h.cleared.is_none()) {
                        continue;
                    }
                    let card = format!("c{}", next + used);
                    used += 1;
                    cards.push((card.clone(), o.id.clone()));
                    n.insert(card, o);
                }
            }
            NodeMessage::Raised { obligation, .. } => {
                if n.items
                    .get(&obligation.id)
                    .is_some_and(|h| h.cleared.is_none())
                {
                    return reply;
                }
                let card = format!("c{}", next + used);
                used += 1;
                cards.push((card.clone(), obligation.id.clone()));
                n.insert(card, obligation);
            }
            NodeMessage::Cleared { id, by, .. } => clear(n, &id, by, now),
            NodeMessage::Filled {
                id, prompt, ask, ..
            } => n.fill(&id, prompt, ask),
            NodeMessage::Acked { id, .. } => clear(n, &id, ClearedBy::Ack, now),
            NodeMessage::Answered { id, .. } => clear(n, &id, ClearedBy::Answer, now),
            NodeMessage::Usage { rows, .. } => {
                for r in rows {
                    n.tokens = n.tokens.saturating_add(r.tokens_in + r.tokens_out);
                    n.cost_micro = n.cost_micro.saturating_add(r.cost_micro);
                }
            }
            NodeMessage::Incident {
                cause,
                workspace,
                detail,
                ..
            } => n.incident(now, &cause, workspace, &detail),
            NodeMessage::Health {
                agents,
                working,
                waiting,
                queue_dropped,
                ..
            } => {
                n.agents = agents;
                n.working = working;
                n.waiting = waiting;
                if queue_dropped > 0 {
                    n.dropped = n.dropped.saturating_add(queue_dropped);
                    n.incident(
                        now,
                        "node queue overflow",
                        None,
                        &format!(
                            "{queue_dropped} messages were dropped while the conductor was away"
                        ),
                    );
                }
            }
            _ => {}
        }
        n.bound(now);
        let live: std::collections::HashSet<String> = n.items.keys().cloned().collect();
        inner.next_card += used;
        for (card, id) in cards {
            inner.cards.insert(card, (node.to_string(), id));
        }
        inner
            .cards
            .retain(|_, (k, id)| k != node || live.contains(id));
        reply
    }

    /// Notes silence: a node quiet past the threshold gets an incident once, after the longer one.
    pub fn tick(&self, now: u64) {
        self.bump();
        let mut inner = self.inner.lock().unwrap();
        for n in inner.nodes.values_mut() {
            if n.standing != Standing::Enrolled {
                continue;
            }
            let quiet = now.saturating_sub(n.last_seen);
            if quiet >= SILENT_INCIDENT_MS && !n.silent_incident {
                n.silent_incident = true;
                let name = n.name.clone();
                n.incident(
                    now,
                    "node silent",
                    None,
                    &format!("{name} sent nothing for 10 minutes"),
                );
            }
        }
    }

    /// The merged stream: open items of every enrolled node ordered by when they were raised, ties by node and id,
    /// so an item's place never changes after it was shown (`ui-never-moves-under-the-finger`).
    pub fn stream(&self, now: u64) -> Vec<Card> {
        let inner = self.inner.lock().unwrap();
        let mut out: Vec<Card> = inner
            .nodes
            .iter()
            .filter(|(_, n)| n.standing == Standing::Enrolled)
            .flat_map(|(k, n)| {
                let silent_since = n.silent(now).then_some(n.last_seen);
                n.items
                    .values()
                    .filter(|h| h.cleared.is_none())
                    .map(move |h| Card {
                        card: h.card.clone(),
                        node: k.clone(),
                        node_name: n.name.clone(),
                        silent_since,
                        obligation: h.obligation.clone(),
                    })
            })
            .collect();
        out.sort_by(|a, b| {
            (a.obligation.raised_at, &a.node, &a.obligation.id).cmp(&(
                b.obligation.raised_at,
                &b.node,
                &b.obligation.id,
            ))
        });
        out
    }

    pub fn nodes(&self, now: u64) -> Vec<NodeView> {
        let inner = self.inner.lock().unwrap();
        let mut out: Vec<NodeView> = inner
            .nodes
            .iter()
            .map(|(k, n)| NodeView {
                node: k.clone(),
                name: n.name.clone(),
                standing: n.standing,
                connected: n.sender.is_some(),
                silent_since: n.silent(now).then_some(n.last_seen),
                open: n.items.values().filter(|h| h.cleared.is_none()).count(),
                agents: n.agents,
                working: n.working,
                waiting: n.waiting,
                dropped: n.dropped,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.node.cmp(&b.node)));
        out
    }

    /// The fleet and the merged stream in the form a device receives.
    pub fn state(&self, now: u64) -> (Vec<NodeInfo>, Vec<CardInfo>) {
        let nodes = self
            .nodes(now)
            .into_iter()
            .map(|n| NodeInfo {
                node: n.node,
                name: n.name,
                standing: match n.standing {
                    Standing::Pending => NodeStanding::Pending,
                    Standing::Enrolled => NodeStanding::Enrolled,
                },
                connected: n.connected,
                silent_since: n.silent_since,
                open: n.open as u32,
                agents: n.agents,
                working: n.working,
                waiting: n.waiting,
                dropped: n.dropped,
            })
            .collect();
        let cards = self
            .stream(now)
            .into_iter()
            .map(|c| CardInfo {
                card: c.card,
                node: c.node,
                node_name: c.node_name,
                silent_since: c.silent_since,
                obligation: c.obligation,
            })
            .collect();
        (nodes, cards)
    }

    pub fn incidents(&self, node: &str) -> Vec<Incident> {
        self.inner
            .lock()
            .unwrap()
            .nodes
            .get(node)
            .map(|n| n.incidents.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Approximate bytes held for `node`, for the memory-bound tests.
    pub fn held_bytes(&self, node: &str) -> usize {
        self.inner
            .lock()
            .unwrap()
            .nodes
            .get(node)
            .map_or(0, |n| n.bytes)
    }

    /// Sends the owner's answer to the node that owns the card. `actor` and `owner` are mandatory
    /// (`operator-actions-carry-an-actor-id-apart-from-the-owner`). The first answer wins: a card that already
    /// cleared names who cleared it, and `idem` makes a redelivery apply once.
    #[allow(clippy::too_many_arguments)]
    pub fn answer(
        &self,
        card: &str,
        actor: &str,
        owner: &str,
        device: &str,
        option: &str,
        idem: &str,
        now: u64,
    ) -> Result<(), AnswerError> {
        self.route(card, now, |h, node| ConductorMessage::Answer {
            target: AnswerTarget {
                workspace: h.obligation.workspace.clone(),
                tab: h.obligation.tab.clone(),
                prompt_id: h.obligation.id.clone(),
            },
            id: h.obligation.id.clone(),
            actor: actor.to_string(),
            owner: owner.to_string(),
            device: cap(device, MAX_NAME),
            option: cap(option, MAX_NAME),
            idem: format!("{node}:{idem}"),
        })
    }

    pub fn ack(
        &self,
        card: &str,
        actor: &str,
        owner: &str,
        idem: &str,
        now: u64,
    ) -> Result<(), AnswerError> {
        self.route(card, now, |h, node| ConductorMessage::Ack {
            id: h.obligation.id.clone(),
            actor: actor.to_string(),
            owner: owner.to_string(),
            idem: format!("{node}:{idem}"),
        })
    }

    fn route(
        &self,
        card: &str,
        now: u64,
        build: impl Fn(&Held, &str) -> ConductorMessage,
    ) -> Result<(), AnswerError> {
        let mut inner = self.inner.lock().unwrap();
        let (node, id) = inner
            .cards
            .get(card)
            .cloned()
            .ok_or(AnswerError::UnknownCard)?;
        let n = inner.nodes.get_mut(&node).ok_or(AnswerError::UnknownCard)?;
        let held = n.items.get(&id).ok_or(AnswerError::UnknownCard)?.clone();
        if let Some((_, by)) = held.cleared {
            return Err(AnswerError::Cleared(by));
        }
        if n.silent(now) {
            return Err(AnswerError::NodeAway);
        }
        let msg = build(&held, &node);
        let key = match &msg {
            ConductorMessage::Answer { idem, .. } | ConductorMessage::Ack { idem, .. } => {
                idem.clone()
            }
            _ => String::new(),
        };
        if n.idem.contains(&key) {
            return Ok(());
        }
        let tx = n.sender.as_ref().ok_or(AnswerError::NodeGone)?;
        tx.send(msg).map_err(|_| AnswerError::NodeGone)?;
        n.idem.push_back(key);
        while n.idem.len() > 256 {
            n.idem.pop_front();
        }
        Ok(())
    }
}

impl Conductor {
    fn send_to(&self, node: &str, msg: ConductorMessage) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .nodes
            .get(node)
            .and_then(|n| n.sender.as_ref())
            .is_some_and(|tx| tx.send(msg).is_ok())
    }

    /// Opens a terminal on `node` for the device connection `device`. The receiver yields the terminal's output
    /// and ends when the terminal does (closed, the node left, or the device fell behind).
    pub fn open_terminal(
        &self,
        device: u64,
        node: &str,
        workspace: &str,
        tab: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(String, tokio::sync::mpsc::Receiver<Vec<u8>>), crate::conductor_terminal::OpenError>
    {
        use crate::conductor_terminal::OpenError;
        let live = {
            let inner = self.inner.lock().unwrap();
            inner
                .nodes
                .get(node)
                .is_some_and(|n| n.standing == Standing::Enrolled && n.sender.is_some())
        };
        if !live {
            return Err(OpenError::NodeAway);
        }
        let (ticket, rx) = self.terms.open(device, node)?;
        let sent = self.send_to(
            node,
            ConductorMessage::OpenTerminal {
                ticket: ticket.clone(),
                workspace: cap(workspace, MAX_NAME),
                tab: cap(tab, MAX_NAME),
                cols,
                rows,
            },
        );
        if !sent {
            self.terms.close(&ticket, None, None);
            return Err(OpenError::NodeAway);
        }
        Ok((ticket, rx))
    }

    /// Keystrokes from the device that holds `ticket`; ignored for any other device.
    pub fn terminal_input(&self, device: u64, ticket: &str, bytes: &[u8]) {
        if let Some(node) = self.terms.node_of(device, ticket) {
            for chunk in bytes.chunks(crate::conductor_terminal::CHUNK) {
                self.send_to(
                    &node,
                    ConductorMessage::TerminalInput {
                        ticket: ticket.to_string(),
                        data: base64::engine::general_purpose::STANDARD.encode(chunk),
                    },
                );
            }
        }
    }

    pub fn terminal_resize(&self, device: u64, ticket: &str, cols: u16, rows: u16) {
        if let Some(node) = self.terms.node_of(device, ticket) {
            self.send_to(
                &node,
                ConductorMessage::ResizeTerminal {
                    ticket: ticket.to_string(),
                    cols,
                    rows,
                },
            );
        }
    }

    /// The device closes one terminal; the node is told to stop it.
    pub fn close_terminal(&self, device: u64, ticket: &str) {
        if let Some(node) = self.terms.node_of(device, ticket) {
            if self.terms.close(ticket, None, Some(device)) {
                self.send_to(
                    &node,
                    ConductorMessage::CloseTerminal {
                        ticket: ticket.to_string(),
                    },
                );
            }
        }
    }

    /// The device connection ended: nothing it opened stays open.
    pub fn close_device_terminals(&self, device: u64) {
        for (ticket, node) in self.terms.close_device(device) {
            self.send_to(&node, ConductorMessage::CloseTerminal { ticket });
        }
    }

    /// Open terminals, for tests and the audit trail.
    pub fn open_terminals(&self) -> usize {
        self.terms.count()
    }
}

fn clear(n: &mut NodeState, id: &str, by: ClearedBy, now: u64) {
    if let Some(h) = n.items.get_mut(id) {
        if h.cleared.is_none() {
            h.cleared = Some((now, by));
            // The question is no longer needed; keep only the metadata.
            h.obligation.prompt = None;
            h.obligation.task = None;
        }
    }
}

fn hash(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(secret.as_bytes()))
}

/// The node signed the nonce with its server key; `node` is that key's public half.
fn verify(node: &str, nonce: &str, sig: &str) -> Result<(), ()> {
    let key: [u8; 32] = URL_SAFE_NO_PAD
        .decode(node)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(())?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| ())?;
    let sig: [u8; 64] = URL_SAFE_NO_PAD
        .decode(sig)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(())?;
    key.verify(nonce.as_bytes(), &Signature::from_bytes(&sig))
        .map_err(|_| ())
}

/// One node's connection, after it answered the challenge: admit it, welcome it, then apply what it sends and pass
/// on what the conductor routes to it, until either side ends. Text messages only; a node sends no terminal bytes.
pub async fn serve(mut link: Link, conductor: Arc<Conductor>, hello: NodeMessage, nonce: &str) {
    let now = crate::events::now_ms();
    let (node, standing) = match conductor.admit(&hello, nonce, now) {
        Ok(ok) => ok,
        Err(why) => {
            tracing::info!(target: "audit", event = "conductor_node_refused", detail = ?why);
            return;
        }
    };
    let NodeMessage::Hello { epoch, .. } = hello else {
        return;
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let welcome = conductor.attach(&node, epoch, tx, now);
    tracing::info!(target: "audit", event = "conductor_node_up", detail = ?standing);
    if send_json(&link, &welcome).await {
        loop {
            tokio::select! {
                inbound = link.rx.recv() => match inbound {
                    Some(Inbound::Json(text)) => {
                        // An unreadable or unknown message is ignored, as a newer node may send what this one lacks.
                        let Ok(msg) = serde_json::from_str::<NodeMessage>(&text) else { continue };
                        for reply in conductor.handle(&node, msg, crate::events::now_ms()) {
                            if !send_json(&link, &reply).await {
                                break;
                            }
                        }
                    }
                    // The link carries attention metadata only (`conductor-links-carry-attention-metadata-only`).
                    Some(Inbound::Term(_) | Inbound::Upload(_)) => {}
                    None => break,
                },
                out = rx.recv() => match out {
                    Some(msg) => {
                        let stop = msg == ConductorMessage::Disable;
                        if !send_json(&link, &msg).await || stop {
                            break;
                        }
                    }
                    None => break,
                },
            }
        }
    }
    conductor.detach(&node);
}

/// Where `--conductor-enrol` asks the running server for a secret.
pub fn enrol_socket_path(dir: &std::path::Path) -> PathBuf {
    crate::events::socket_in(dir, "conductor.sock")
}

/// Answers each connection to a Unix socket (mode 0600, never TCP) with a freshly minted enrolment secret and one
/// newline. The secret lives in this process only: it is not logged, not written to a file, and the previous one stops
/// working.
pub async fn serve_enrol_socket(conductor: Arc<Conductor>, path: PathBuf) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::AsyncWriteExt;
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let secret = conductor.mint_enrol(crate::events::now_ms());
        tracing::info!(target: "audit", event = "conductor_enrol_minted");
        let _ = stream.write_all(format!("{secret}\n").as_bytes()).await;
    }
}

/// The CLI side: the secret the running server minted.
pub async fn request_enrol(path: &std::path::Path) -> anyhow::Result<String> {
    use tokio::io::AsyncReadExt;
    let mut stream = tokio::net::UnixStream::connect(path).await.map_err(|_| {
        anyhow::anyhow!(
            "no running conductor here; start the server with --accept-nodes (TERMENBOX_ACCEPT_NODES=1) first"
        )
    })?;
    let mut text = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        (&mut stream).take(256).read_to_string(&mut text),
    )
    .await
    .map_err(|_| anyhow::anyhow!("the conductor did not answer"))??;
    let secret = text.trim().to_string();
    anyhow::ensure!(!secret.is_empty(), "the conductor sent no secret");
    Ok(secret)
}

async fn send_json(link: &Link, m: &ConductorMessage) -> bool {
    match serde_json::to_string(m) {
        Ok(text) => link.tx.send(text).await,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory::ServerKey;
    use termenbox_core::events::AttentionKind;
    use tokio::sync::mpsc::unbounded_channel;

    const T0: u64 = 1_700_000_000_000;

    struct Sim {
        key: ServerKey,
        epoch: u64,
        seq: u64,
    }

    fn obligation(id: &str, at: u64) -> Obligation {
        Obligation {
            id: id.into(),
            workspace: "api".into(),
            tab: Some("t1".into()),
            kind: AttentionKind::NeedsInput,
            raised_at: at,
            urgency: None,
            class_features: vec!["claude".into()],
            task: Some("fix".into()),
            prompt: Some("Run cargo test?".into()),
            tier: None,
            ask: None,
            cause: None,
        }
    }

    impl Sim {
        fn new(seed: u8) -> Self {
            Sim {
                key: ServerKey::from_seed([seed; 32]),
                epoch: seed as u64 * 100,
                seq: 0,
            }
        }

        fn id(&self) -> String {
            self.key.server_id()
        }

        fn hello(&self, nonce: &str, enrol: Option<&str>) -> NodeMessage {
            NodeMessage::Hello {
                v: 1,
                node: self.id(),
                name: format!("node{}", self.epoch),
                epoch: self.epoch,
                version: "t".into(),
                caps: vec![],
                enrol: enrol.map(str::to_string),
                sig: URL_SAFE_NO_PAD.encode(self.key.sign(nonce.as_bytes())),
            }
        }

        /// Enrols, taps and connects, returning the receiver of what the conductor sends this node.
        fn join(
            &self,
            c: &Conductor,
            now: u64,
        ) -> tokio::sync::mpsc::UnboundedReceiver<ConductorMessage> {
            let secret = c.mint_enrol(now);
            let (id, adm) = c.admit(&self.hello("n", Some(&secret)), "n", now).unwrap();
            assert_eq!(adm, Admission::Pending);
            assert!(c.approve(&id));
            let (tx, rx) = unbounded_channel();
            c.attach(&id, self.epoch, tx, now);
            rx
        }

        fn next(&mut self) -> u64 {
            self.seq += 1;
            self.seq
        }

        fn raise(&mut self, c: &Conductor, o: Obligation, now: u64) {
            let m = NodeMessage::Raised {
                epoch: self.epoch,
                seq: self.next(),
                obligation: o,
            };
            c.handle(&self.id(), m, now);
        }

        fn health(&mut self, c: &Conductor, now: u64) {
            let m = NodeMessage::Health {
                epoch: self.epoch,
                seq: self.next(),
                agents: 50,
                working: 3,
                waiting: 1,
                load: 0,
                queue_dropped: 0,
            };
            c.handle(&self.id(), m, now);
        }
    }

    fn conductor() -> Conductor {
        Conductor::new(None, T0)
    }

    #[test]
    fn conductor_enrols_with_a_one_time_secret_and_the_owners_tap() {
        let c = conductor();
        let n = Sim::new(1);
        // No secret minted: refused.
        assert_eq!(
            c.admit(&n.hello("n", Some("guess")), "n", T0).unwrap_err(),
            Refusal::NotEnrolled
        );
        let secret = c.mint_enrol(T0);
        // A wrong signature is refused even with the secret.
        assert_eq!(
            c.admit(&n.hello("other", Some(&secret)), "n", T0)
                .unwrap_err(),
            Refusal::BadSignature
        );
        let (id, adm) = c.admit(&n.hello("n", Some(&secret)), "n", T0).unwrap();
        assert_eq!(adm, Admission::Pending);
        // The secret is single use.
        let m = Sim::new(2);
        assert_eq!(
            c.admit(&m.hello("n", Some(&secret)), "n", T0).unwrap_err(),
            Refusal::NotEnrolled
        );
        // Until the tap its messages are discarded.
        let (tx, _rx) = unbounded_channel();
        c.attach(&id, n.epoch, tx, T0);
        let mut n = n;
        n.raise(&c, obligation("a1", T0), T0);
        assert!(c.stream(T0).is_empty());
        assert!(c.approve(&id));
        n.raise(&c, obligation("a2", T0), T0);
        assert_eq!(c.stream(T0).len(), 1);
        // Enrolled: later connections need only the signature.
        assert_eq!(
            c.admit(&n.hello("n2", None), "n2", T0 + 1).unwrap().1,
            Admission::Enrolled
        );
    }

    #[test]
    fn conductor_secret_expires() {
        let c = conductor();
        let n = Sim::new(1);
        let secret = c.mint_enrol(T0);
        assert_eq!(
            c.admit(&n.hello("n", Some(&secret)), "n", T0 + ENROL_TTL_MS + 1)
                .unwrap_err(),
            Refusal::NotEnrolled
        );
    }

    fn drain(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ConductorMessage>,
    ) -> Vec<ConductorMessage> {
        let mut out = Vec::new();
        while let Ok(m) = rx.try_recv() {
            out.push(m);
        }
        out
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn conductor_terminal_bytes_flow_only_between_open_and_close() {
        let c = conductor();
        let n = Sim::new(1);
        let mut node_rx = n.join(&c, T0);
        let _ = drain(&mut node_rx);
        let out = |ticket: &str| NodeMessage::TerminalOutput {
            ticket: ticket.into(),
            data: b64(b"hi"),
        };

        // Before an open, output and input for a made-up ticket go nowhere.
        c.handle(&n.id(), out("nope"), T0);
        c.terminal_input(1, "nope", b"x");
        assert!(drain(&mut node_rx).is_empty());

        let (ticket, mut dev_rx) = c.open_terminal(1, &n.id(), "api", "t1", 80, 24).unwrap();
        assert!(matches!(
            drain(&mut node_rx).as_slice(),
            [ConductorMessage::OpenTerminal { ticket: t, cols: 80, rows: 24, .. }] if *t == ticket
        ));
        c.handle(&n.id(), out(&ticket), T0);
        assert_eq!(dev_rx.try_recv().unwrap(), b"hi");
        c.terminal_input(1, &ticket, b"ls\n");
        assert_eq!(
            drain(&mut node_rx),
            vec![ConductorMessage::TerminalInput {
                ticket: ticket.clone(),
                data: b64(b"ls\n")
            }]
        );

        c.close_terminal(1, &ticket);
        assert!(matches!(
            drain(&mut node_rx).as_slice(),
            [ConductorMessage::CloseTerminal { ticket: t }] if *t == ticket
        ));
        // After the close nothing crosses in either direction.
        c.handle(&n.id(), out(&ticket), T0);
        c.terminal_input(1, &ticket, b"x");
        assert!(dev_rx.try_recv().is_err());
        assert!(drain(&mut node_rx).is_empty());
        assert_eq!(c.open_terminals(), 0);
    }

    #[test]
    fn conductor_terminal_belongs_to_its_device_and_its_node() {
        let c = conductor();
        let (a, b) = (Sim::new(1), Sim::new(2));
        let mut a_rx = a.join(&c, T0);
        let mut b_rx = b.join(&c, T0);
        let (ticket, mut dev_rx) = c.open_terminal(1, &a.id(), "w", "t", 80, 24).unwrap();
        let _ = (drain(&mut a_rx), drain(&mut b_rx));
        // Another node cannot write to it; another device cannot type into it or close it.
        c.handle(
            &b.id(),
            NodeMessage::TerminalOutput {
                ticket: ticket.clone(),
                data: b64(b"x"),
            },
            T0,
        );
        assert!(dev_rx.try_recv().is_err());
        c.terminal_input(2, &ticket, b"x");
        c.terminal_resize(2, &ticket, 10, 10);
        c.close_terminal(2, &ticket);
        assert!(drain(&mut a_rx).is_empty());
        assert_eq!(c.open_terminals(), 1);
    }

    #[test]
    fn conductor_terminal_a_closed_device_leaves_nothing_open() {
        let c = conductor();
        let n = Sim::new(1);
        let mut node_rx = n.join(&c, T0);
        let (t1, _r1) = c.open_terminal(7, &n.id(), "w", "a", 80, 24).unwrap();
        let (t2, _r2) = c.open_terminal(7, &n.id(), "w", "b", 80, 24).unwrap();
        let (_t3, _r3) = c.open_terminal(8, &n.id(), "w", "c", 80, 24).unwrap();
        let _ = drain(&mut node_rx);
        c.close_device_terminals(7);
        let told = drain(&mut node_rx);
        assert_eq!(told.len(), 2);
        for t in [t1, t2] {
            assert!(told.contains(&ConductorMessage::CloseTerminal { ticket: t }));
        }
        // The other device keeps its terminal.
        assert_eq!(c.open_terminals(), 1);
    }

    #[test]
    fn conductor_terminal_ends_when_the_node_leaves_or_ends_it() {
        let c = conductor();
        let n = Sim::new(1);
        let _node_rx = n.join(&c, T0);
        let (ticket, mut rx) = c.open_terminal(1, &n.id(), "w", "t", 80, 24).unwrap();
        c.handle(&n.id(), NodeMessage::TerminalClosed { ticket }, T0);
        assert_eq!(c.open_terminals(), 0);
        assert!(rx.try_recv().is_err());
        let (_t, mut rx) = c.open_terminal(1, &n.id(), "w", "t", 80, 24).unwrap();
        c.detach(&n.id());
        assert_eq!(c.open_terminals(), 0);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));
        // A node whose link is down cannot be opened on.
        assert_eq!(
            c.open_terminal(1, &n.id(), "w", "t", 80, 24).unwrap_err(),
            crate::conductor_terminal::OpenError::NodeAway
        );
    }

    #[test]
    fn conductor_terminal_limits_per_device_and_drops_a_device_that_falls_behind() {
        use crate::conductor_terminal::{OpenError, BUFFER_CHUNKS, PER_DEVICE};
        let c = conductor();
        let n = Sim::new(1);
        let mut node_rx = n.join(&c, T0);
        let mut held = Vec::new();
        for _ in 0..PER_DEVICE {
            held.push(c.open_terminal(1, &n.id(), "w", "t", 80, 24).unwrap());
        }
        assert_eq!(
            c.open_terminal(1, &n.id(), "w", "t", 80, 24).unwrap_err(),
            OpenError::TooMany
        );
        let _ = drain(&mut node_rx);
        // Nobody reads the first terminal: once its buffer is full it is closed, and the node is told.
        let ticket = held[0].0.clone();
        for _ in 0..=BUFFER_CHUNKS {
            c.handle(
                &n.id(),
                NodeMessage::TerminalOutput {
                    ticket: ticket.clone(),
                    data: b64(b"x"),
                },
                T0,
            );
        }
        assert_eq!(c.open_terminals(), PER_DEVICE - 1);
        assert!(drain(&mut node_rx).contains(&ConductorMessage::CloseTerminal { ticket }));
    }

    #[test]
    fn conductor_enrolled_nodes_survive_a_restart() {
        let dir =
            std::env::temp_dir().join(format!("termenbox-conductor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = Conductor::new(Some(dir.clone()), T0);
        let n = Sim::new(1);
        let _rx = n.join(&c, T0);
        let again = Conductor::new(Some(dir.clone()), T0);
        assert_eq!(
            again.admit(&n.hello("x", None), "x", T0).unwrap().1,
            Admission::Enrolled
        );
    }

    #[test]
    fn conductor_merges_three_nodes_in_a_stable_order() {
        let c = conductor();
        let (mut a, mut b, mut d) = (Sim::new(1), Sim::new(2), Sim::new(3));
        let _ra = a.join(&c, T0);
        let _rb = b.join(&c, T0);
        let _rd = d.join(&c, T0);
        b.raise(&c, obligation("a1", T0 + 10), T0 + 10);
        a.raise(&c, obligation("a1", T0 + 20), T0 + 20);
        d.raise(&c, obligation("a1", T0 + 5), T0 + 20);
        a.raise(&c, obligation("a2", T0 + 20), T0 + 20);
        let order: Vec<(String, String)> = c
            .stream(T0 + 20)
            .into_iter()
            .map(|x| (x.node, x.obligation.id))
            .collect();
        assert_eq!(order.len(), 4);
        // By raise time first; the two raised at the same time are ordered by node then id.
        assert_eq!(order[0], (d.id(), "a1".into()));
        assert_eq!(order[1], (b.id(), "a1".into()));
        assert!(order[2] < order[3] || order[2].0 != order[3].0);
        // The same id on three nodes is three different cards.
        let cards: std::collections::HashSet<_> =
            c.stream(T0 + 20).into_iter().map(|x| x.card).collect();
        assert_eq!(cards.len(), 4);
        // A clear leaves the others where they were.
        let before: Vec<_> = c.stream(T0 + 20).into_iter().map(|x| x.card).collect();
        let m = NodeMessage::Cleared {
            epoch: b.epoch,
            seq: b.next(),
            id: "a1".into(),
            by: ClearedBy::Fact,
        };
        c.handle(&b.id(), m, T0 + 30);
        let after: Vec<_> = c.stream(T0 + 30).into_iter().map(|x| x.card).collect();
        let expect: Vec<_> = before
            .into_iter()
            .filter(|x| x != &after_missing(&after, &c, &b))
            .collect();
        assert_eq!(after, expect);
    }

    /// The card that left: the one of node `b` (the test cleared its only item).
    fn after_missing(_after: &[String], c: &Conductor, b: &Sim) -> String {
        let inner = c.inner.lock().unwrap();
        inner
            .nodes
            .get(&b.id())
            .and_then(|n| n.items.get("a1"))
            .map(|h| h.card.clone())
            .unwrap()
    }

    #[test]
    fn conductor_routes_an_answer_to_the_owning_node_only() {
        let c = conductor();
        let (mut a, mut b) = (Sim::new(1), Sim::new(2));
        let mut ra = a.join(&c, T0);
        let mut rb = b.join(&c, T0);
        ra.try_recv().ok();
        a.raise(&c, obligation("a1", T0), T0);
        b.raise(&c, obligation("a1", T0 + 1), T0 + 1);
        let card = c
            .stream(T0 + 1)
            .into_iter()
            .find(|x| x.node == b.id())
            .unwrap()
            .card;
        c.answer(&card, "me", "me", "phone", "1", "i1", T0 + 2)
            .unwrap();
        match rb.try_recv().unwrap() {
            ConductorMessage::Answer {
                id,
                actor,
                owner,
                device,
                option,
                ..
            } => {
                assert_eq!(
                    (id.as_str(), actor.as_str(), owner.as_str()),
                    ("a1", "me", "me")
                );
                assert_eq!((device.as_str(), option.as_str()), ("phone", "1"));
            }
            other => panic!("{other:?}"),
        }
        assert!(ra.try_recv().is_err(), "node a must hear nothing");
        // Redelivery with the same idem applies once.
        c.answer(&card, "me", "me", "phone", "1", "i1", T0 + 3)
            .unwrap();
        assert!(rb.try_recv().is_err());
    }

    #[test]
    fn conductor_second_answer_loses_after_the_first_cleared() {
        let c = conductor();
        let mut a = Sim::new(1);
        let _ra = a.join(&c, T0);
        a.raise(&c, obligation("a1", T0), T0);
        let card = c.stream(T0)[0].card.clone();
        let m = NodeMessage::Answered {
            epoch: a.epoch,
            seq: a.next(),
            id: "a1".into(),
            actor: "me".into(),
            owner: "me".into(),
            device: "desktop".into(),
            option: "0".into(),
        };
        c.handle(&a.id(), m, T0 + 1);
        assert_eq!(
            c.answer(&card, "me", "me", "phone", "1", "i2", T0 + 2),
            Err(AnswerError::Cleared(ClearedBy::Answer))
        );
    }

    #[test]
    fn conductor_silent_node_keeps_its_items_and_refuses_answers() {
        let c = conductor();
        let (mut a, mut b) = (Sim::new(1), Sim::new(2));
        let _ra = a.join(&c, T0);
        let _rb = b.join(&c, T0);
        a.raise(&c, obligation("a1", T0), T0);
        b.raise(&c, obligation("a1", T0 + 1), T0 + 1);
        // Only b keeps talking.
        let later = T0 + SILENT_AFTER_MS + 1_000;
        b.health(&c, later);
        let stream = c.stream(later);
        assert_eq!(stream.len(), 2, "silent items stay in the stream");
        let silent = stream.iter().find(|x| x.node == a.id()).unwrap();
        assert!(silent.silent_since.is_some());
        assert!(stream
            .iter()
            .find(|x| x.node == b.id())
            .unwrap()
            .silent_since
            .is_none());
        assert_eq!(
            c.answer(&silent.card, "me", "me", "phone", "1", "i", later),
            Err(AnswerError::NodeAway)
        );
        // After ten minutes it is an incident, once.
        let long = T0 + SILENT_INCIDENT_MS + 1;
        c.tick(long);
        c.tick(long + 1);
        let inc = c.incidents(&a.id());
        assert_eq!(inc.iter().filter(|i| i.cause == "node silent").count(), 1);
        // A message from it ends the silence.
        a.health(&c, long + 5);
        assert!(c
            .stream(long + 5)
            .iter()
            .all(|x| x.silent_since.is_none() || x.node == b.id()));
    }

    #[test]
    fn conductor_gap_asks_for_a_resync_and_ignores_until_the_snapshot() {
        let c = conductor();
        let mut a = Sim::new(1);
        let _ra = a.join(&c, T0);
        a.raise(&c, obligation("a1", T0), T0);
        a.seq += 1; // a message that never arrived
        let m = NodeMessage::Raised {
            epoch: a.epoch,
            seq: a.next(),
            obligation: obligation("a2", T0 + 1),
        };
        assert_eq!(c.handle(&a.id(), m, T0 + 1), vec![ConductorMessage::Resync]);
        assert_eq!(c.stream(T0 + 1).len(), 1);
        // The snapshot replaces the view and the next seq is the new baseline.
        let snap = NodeMessage::Snapshot {
            epoch: a.epoch,
            items: vec![obligation("a2", T0 + 1)],
            workspaces: vec![],
        };
        c.handle(&a.id(), snap, T0 + 2);
        let ids: Vec<_> = c
            .stream(T0 + 2)
            .into_iter()
            .map(|x| x.obligation.id)
            .collect();
        assert_eq!(ids, vec!["a2".to_string()]);
        a.raise(&c, obligation("a3", T0 + 3), T0 + 3);
        assert_eq!(c.stream(T0 + 3).len(), 2);
        // A duplicate is ignored.
        let dup = NodeMessage::Raised {
            epoch: a.epoch,
            seq: a.seq,
            obligation: obligation("a9", T0 + 4),
        };
        c.handle(&a.id(), dup, T0 + 4);
        assert_eq!(c.stream(T0 + 4).len(), 2);
    }

    #[test]
    fn conductor_new_epoch_drops_items_the_node_can_no_longer_answer() {
        let c = conductor();
        let mut a = Sim::new(1);
        let _ra = a.join(&c, T0);
        a.raise(&c, obligation("a1", T0), T0);
        // The node restarted: a new epoch and a counter that starts again.
        let (tx, _rx) = unbounded_channel();
        c.attach(&a.id(), 999, tx, T0 + 10);
        assert!(c.stream(T0 + 10).is_empty());
        let snap = NodeMessage::Snapshot {
            epoch: 999,
            items: vec![obligation("a1", T0 + 11)],
            workspaces: vec![],
        };
        c.handle(&a.id(), snap, T0 + 11);
        assert_eq!(c.stream(T0 + 11).len(), 1);
    }

    #[test]
    fn conductor_cleared_items_keep_no_agent_text() {
        let c = conductor();
        let mut a = Sim::new(1);
        let _ra = a.join(&c, T0);
        a.raise(&c, obligation("a1", T0), T0);
        let m = NodeMessage::Cleared {
            epoch: a.epoch,
            seq: a.next(),
            id: "a1".into(),
            by: ClearedBy::Ack,
        };
        c.handle(&a.id(), m, T0 + 1);
        let inner = c.inner.lock().unwrap();
        let h = &inner.nodes[&a.id()].items["a1"];
        assert!(h.obligation.prompt.is_none() && h.obligation.task.is_none());
    }

    #[test]
    fn conductor_memory_is_bounded_per_node() {
        let c = conductor();
        let mut a = Sim::new(1);
        let _ra = a.join(&c, T0);
        let long = "x".repeat(600);
        for i in 0..2_000u64 {
            let mut o = obligation(&format!("a{i}"), T0 + i);
            o.prompt = Some(long.clone());
            a.raise(&c, o, T0 + i);
        }
        assert!(c.held_bytes(&a.id()) <= NODE_BYTES);
        let open = c.stream(T0 + 2_000).len();
        assert!(open < 2_000 && open > 0);
        // Losing open items is visible, not silent.
        assert!(c
            .incidents(&a.id())
            .iter()
            .any(|i| i.cause == "conductor queue overflow"));
        assert!(c.nodes(T0 + 2_000)[0].dropped > 0);
    }

    #[test]
    fn conductor_three_nodes_of_fifty_agents_fit_the_budget() {
        let c = conductor();
        let mut sims = [Sim::new(1), Sim::new(2), Sim::new(3)];
        let _rx: Vec<_> = sims.iter().map(|s| s.join(&c, T0)).collect();
        // 50 agents each, a fifth of them waiting, for a simulated hour of heartbeats.
        for s in sims.iter_mut() {
            for i in 0..10 {
                s.raise(&c, obligation(&format!("a{i}"), T0 + i), T0 + i);
            }
        }
        for beat in 0..120u64 {
            for s in sims.iter_mut() {
                s.health(&c, T0 + beat * 30_000);
            }
        }
        let total: usize = sims.iter().map(|s| c.held_bytes(&s.id())).sum();
        assert!(total < 1024 * 1024, "{total} bytes for three nodes");
        assert_eq!(c.stream(T0 + 3_600_000).len(), 30);
        assert!(c.nodes(T0 + 3_600_000).iter().all(|n| n.agents == 50));
    }

    #[test]
    fn conductor_removing_a_node_disables_it_and_forgets_its_items() {
        let c = conductor();
        let mut a = Sim::new(1);
        let mut ra = a.join(&c, T0);
        a.raise(&c, obligation("a1", T0), T0);
        assert!(c.remove(&a.id()));
        assert_eq!(ra.try_recv().unwrap(), ConductorMessage::Disable);
        assert!(c.stream(T0).is_empty());
        assert!(c.nodes(T0).is_empty());
    }

    /// A minimal transport for a conductor: the handshake the real one does, then `serve` over a `Link`. Returns the
    /// port it listens on.
    async fn spawn_transport(c: Arc<Conductor>, conductor_key: &ServerKey) -> u16 {
        use futures_util::{SinkExt, StreamExt};
        use termenbox_core::protocol::ServerMessage;
        use tokio_tungstenite::tungstenite::Message;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // A minimal transport: the handshake the real one does, then `serve` over a `Link`.
        tokio::spawn({
            let (c, id) = (c.clone(), conductor_key.server_id());
            async move {
                loop {
                    let (sock, _) = listener.accept().await.unwrap();
                    let ws = tokio_tungstenite::accept_async(sock).await.unwrap();
                    let (c, id) = (c.clone(), id.clone());
                    tokio::spawn(async move {
                        let (mut tx, mut rx) = ws.split();
                        let Some(Ok(Message::Text(_hello))) = rx.next().await else {
                            return;
                        };
                        let challenge = ServerMessage::Challenge {
                            nonce: "nonce-1".into(),
                            server_id: id,
                        };
                        tx.send(Message::Text(serde_json::to_string(&challenge).unwrap()))
                            .await
                            .unwrap();
                        let Some(Ok(Message::Text(text))) = rx.next().await else {
                            return;
                        };
                        let hello: NodeMessage = serde_json::from_str(&text).unwrap();
                        let (link, mut end) = crate::link::pair(None, None);
                        tokio::spawn(serve(link, c, hello, "nonce-1"));
                        loop {
                            tokio::select! {
                                m = rx.next() => match m {
                                    Some(Ok(Message::Text(t))) => {
                                        if end.to_workspace.send(Inbound::Json(t)).await.is_err() { break }
                                    }
                                    Some(Ok(_)) => {}
                                    _ => break,
                                },
                                out = end.from_workspace.recv() => match out {
                                    Some(t) => { if tx.send(Message::Text(t)).await.is_err() { break } }
                                    None => break,
                                },
                            }
                        }
                    });
                }
            }
        });
        port
    }

    async fn until(what: &str, mut ok: impl FnMut() -> bool) {
        for _ in 0..200 {
            if ok() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("timed out waiting for {what}");
    }

    struct Typed(std::sync::Mutex<Vec<(String, u8)>>, EventHub, AgentState);

    use crate::events::EventHub;
    use termenbox_core::events::AgentState;

    #[async_trait::async_trait]
    impl crate::conductor_link::Applier for Typed {
        async fn answer(&self, id: &str, choice: u8) -> anyhow::Result<()> {
            self.0.lock().unwrap().push((id.to_string(), choice));
            // The agent moves on once the keys arrive.
            self.1.push(
                "w",
                termenbox_core::events::Source::Hook,
                termenbox_core::events::Confidence::High,
                termenbox_core::events::EventKind::State { state: self.2 },
                crate::events::now_ms(),
            );
            Ok(())
        }
    }

    #[tokio::test]
    async fn conductor_surface_answer_reaches_the_node_and_clears_there() {
        use crate::conductor_link::{self, Node, Pin};
        use crate::events::now_ms;
        use termenbox_core::events::{Confidence, EventKind, Source};

        let conductor_key = ServerKey::from_seed([9; 32]);
        let c = Arc::new(conductor());
        let secret = c.mint_enrol(now_ms());
        let port = spawn_transport(c.clone(), &conductor_key).await;
        let dir =
            std::env::temp_dir().join(format!("termenbox-conductor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pin = Pin {
            conductor: conductor_key.server_id(),
            urls: vec![format!("ws://127.0.0.1:{port}")],
            enrol: Some(secret),
            enabled: true,
            standby: None,
        };
        pin.save(&dir).unwrap();
        let hub = EventHub::new([7; 32]);
        hub.push(
            "w",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::NeedsInput,
            },
            now_ms(),
        );
        let typed = Arc::new(Typed(Default::default(), hub.clone(), AgentState::Idle));
        let node_key = ServerKey::from_seed([5; 32]);
        tokio::spawn(conductor_link::run(
            dir,
            pin,
            Node {
                key: ServerKey::from_seed([5; 32]),
                name: "vps".into(),
                version: "t".into(),
                hub: hub.clone(),
                applier: Some(typed.clone()),
            },
            crate::signal::Backoff::new(
                std::time::Duration::from_millis(20),
                std::time::Duration::from_millis(50),
            ),
        ));

        // Enrol, tap: the node appears pending, then live.
        until("the node to appear as pending", || {
            c.nodes(now_ms())
                .iter()
                .any(|n| n.standing == Standing::Pending && n.connected)
        })
        .await;
        assert!(
            c.state(now_ms()).1.is_empty(),
            "a pending node shows nothing"
        );
        assert!(c.approve(&node_key.server_id()));
        until("the ask to reach the stream", || {
            c.stream(now_ms()).len() == 1
        })
        .await;
        let (nodes, cards) = c.state(now_ms());
        assert_eq!(nodes[0].standing, NodeStanding::Enrolled);
        let card = cards[0].card.clone();

        // The owner's answer reaches the node, is typed there once, and the ask clears.
        c.answer(&card, "me", "me", "phone", "1", "i1", now_ms())
            .unwrap();
        until("the answer to be typed", || {
            !typed.0.lock().unwrap().is_empty()
        })
        .await;
        until("the clear to arrive", || c.stream(now_ms()).is_empty()).await;
        assert_eq!(typed.0.lock().unwrap().as_slice(), [("a1".to_string(), 1)]);
        // A second answer to the same card loses.
        assert!(matches!(
            c.answer(&card, "me", "me", "phone", "1", "i2", now_ms()),
            Err(AnswerError::Cleared(ClearedBy::Answer)) | Err(AnswerError::UnknownCard)
        ));
    }

    #[tokio::test]
    async fn conductor_surface_enrol_socket_hands_out_a_secret_that_works_once() {
        let c = Arc::new(conductor());
        let dir =
            std::env::temp_dir().join(format!("termenbox-conductor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = enrol_socket_path(&dir);
        tokio::spawn(serve_enrol_socket(c.clone(), path.clone()));
        until("the socket", || path.exists()).await;
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&path).unwrap().permissions(),
        );
        assert_eq!(mode & 0o777, 0o600);
        let secret = request_enrol(&path).await.unwrap();
        let n = Sim::new(1);
        let (_, adm) = c
            .admit(&n.hello("n", Some(&secret)), "n", crate::events::now_ms())
            .unwrap();
        assert_eq!(adm, Admission::Pending);
        // Burnt on use.
        let other = Sim::new(2);
        assert_eq!(
            c.admit(
                &other.hello("n", Some(&secret)),
                "n",
                crate::events::now_ms()
            )
            .unwrap_err(),
            Refusal::NotEnrolled
        );
        // Asking again retires the previous secret.
        let first = request_enrol(&path).await.unwrap();
        let second = request_enrol(&path).await.unwrap();
        assert_ne!(first, second);
        assert_eq!(
            c.admit(
                &other.hello("n", Some(&first)),
                "n",
                crate::events::now_ms()
            )
            .unwrap_err(),
            Refusal::NotEnrolled
        );
    }

    #[test]
    fn conductor_surface_state_is_pushed_on_change_and_carries_no_secret() {
        let c = conductor();
        let mut rx = c.subscribe();
        let mut n = Sim::new(1);
        let _keep = n.join(&c, T0);
        assert!(rx.has_changed().unwrap());
        let _ = rx.borrow_and_update();
        n.raise(&c, obligation("a1", T0), T0);
        assert!(rx.has_changed().unwrap(), "a raise wakes the pushers");
        let (nodes, cards) = c.state(T0);
        assert_eq!((nodes.len(), cards.len()), (1, 1));
        let wire = serde_json::to_string(&(nodes, cards)).unwrap();
        assert!(!wire.contains("sig"), "{wire}");
    }

    #[tokio::test]
    async fn conductor_serves_a_real_node_link_end_to_end() {
        use crate::conductor_link::{self, Node, Pin};
        use crate::events::{now_ms, EventHub};
        use termenbox_core::events::{AgentState, Confidence, EventKind, Source};

        let conductor_key = ServerKey::from_seed([9; 32]);
        let c = Arc::new(conductor());
        let secret = c.mint_enrol(now_ms());
        let port = spawn_transport(c.clone(), &conductor_key).await;

        let dir =
            std::env::temp_dir().join(format!("termenbox-conductor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pin = Pin {
            conductor: conductor_key.server_id(),
            urls: vec![format!("ws://127.0.0.1:{port}")],
            enrol: Some(secret),
            enabled: true,
            standby: None,
        };
        pin.save(&dir).unwrap();
        let hub = EventHub::new([7; 32]);
        let state = |s| {
            hub.push(
                "w",
                Source::Hook,
                Confidence::High,
                EventKind::State { state: s },
                now_ms(),
            );
        };
        state(AgentState::NeedsInput);
        let node_key = ServerKey::from_seed([5; 32]);
        tokio::spawn(conductor_link::run(
            dir.clone(),
            pin,
            Node {
                key: ServerKey::from_seed([5; 32]),
                name: "vps".into(),
                version: "t".into(),
                hub: hub.clone(),
                applier: None,
            },
            crate::signal::Backoff::new(
                std::time::Duration::from_millis(20),
                std::time::Duration::from_millis(50),
            ),
        ));

        // Enrolled on the secret, but not trusted until the owner taps: nothing in the stream.
        until("the node to appear as pending", || {
            c.nodes(now_ms())
                .iter()
                .any(|n| n.standing == Standing::Pending && n.connected)
        })
        .await;
        assert!(c.stream(now_ms()).is_empty());
        assert!(c.approve(&node_key.server_id()));
        until("the ask to reach the stream", || {
            c.stream(now_ms()).len() == 1
        })
        .await;
        let card = c.stream(now_ms()).remove(0);
        assert_eq!(card.node, node_key.server_id());
        assert_eq!(card.node_name, "vps");
        // The ask clears on the node and leaves the stream.
        state(AgentState::Idle);
        until("the clear to arrive", || c.stream(now_ms()).is_empty()).await;
        // The secret was burnt, so the node reconnects on its signature alone.
        assert_eq!(Pin::load(&dir).unwrap().enrol, None);
    }
}
