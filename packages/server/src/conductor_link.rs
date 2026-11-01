//! The node's outbound link to a conductor (decision 0170, `docs/plans/assets/control-plane/conductor.md`).
//!
//! One connection the node dials out, so a node behind NAT needs no open port. It carries attention metadata only
//! (`conductor-links-carry-attention-metadata-only`): this module sends text frames of `NodeMessage` and never
//! touches a terminal. While the conductor is away the node keeps a bounded queue and counts what it drops. Off
//! unless a pairing was given (`--conductor <link>`); `--conductor off` stops it and keeps the pin.
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use termenbox_core::bundle::{Bundle, CandidateKind};
use termenbox_core::conductor::{ConductorMessage, NodeMessage, Obligation, MAX_USAGE_ROWS};
use termenbox_core::events::{AgentState, Attention};
use termenbox_core::protocol::{ClientMessage, ServerMessage};
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::tungstenite::Message;

use crate::directory::ServerKey;
use crate::events::{now_ms, EventHub, StatusChange};
use crate::signal::Backoff;

/// The queue while the conductor is away: whichever limit is hit first.
pub const QUEUE_MESSAGES: usize = 256;
pub const QUEUE_BYTES: usize = 64 * 1024;
const HEARTBEAT: Duration = Duration::from_secs(30);
const STEP: Duration = Duration::from_secs(10);
const PROTOCOL: u32 = 1;
const FILE: &str = "conductor.json";

// --- the pin -------------------------------------------------------------------------------------------------------

/// The conductor this node is enrolled to, kept next to the server key (mode 0600: it holds the enrolment secret
/// until the first welcome).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// The conductor's server id; the challenge it sends must name it.
    pub conductor: String,
    /// Where to dial, in order.
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrol: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Pin {
    /// From a conductor pairing link: `<client>/#/pair/<signed bundle>[?enrol=<secret>]`, or the bare bundle.
    pub fn from_link(link: &str, now_secs: u64) -> Result<Pin> {
        let rest = link.rsplit_once("/pair/").map_or(link, |(_, r)| r);
        let (wire, query) = rest.split_once('?').unwrap_or((rest, ""));
        let enrol = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("enrol="))
            .map(str::to_string);
        let bundle = Bundle::verify(wire.trim(), now_secs, None, None)
            .map_err(|e| anyhow::anyhow!("the conductor link is not valid: {e:?}"))?;
        let urls = urls_of(&bundle);
        anyhow::ensure!(!urls.is_empty(), "the conductor link has no address");
        Ok(Pin {
            conductor: bundle.server_id,
            urls,
            enrol,
            enabled: true,
        })
    }

    pub fn load(dir: &Path) -> Option<Pin> {
        serde_json::from_slice(&std::fs::read(dir.join(FILE)).ok()?).ok()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        crate::auth::write_private(&dir.join(FILE), &serde_json::to_vec(self)?)
    }
}

/// The WebSocket URLs of a bundle's addresses. A tunnel is a hostname behind TLS; the rest are the server's own port.
fn urls_of(bundle: &Bundle) -> Vec<String> {
    bundle
        .candidates
        .iter()
        .map(|c| {
            let host = if c.addr.contains(':') && !c.addr.starts_with('[') {
                format!("[{}]", c.addr)
            } else {
                c.addr.clone()
            };
            match c.kind {
                CandidateKind::Tunnel => format!("wss://{host}/ws"),
                _ => format!("ws://{host}:{}/ws", c.port),
            }
        })
        .collect()
}

/// `--conductor`: `off` disables a pinned conductor, anything else is a pairing link. Returns the pin to run, if any.
pub fn configure(dir: &Path, arg: Option<&str>) -> Result<Option<Pin>> {
    match arg {
        Some("off") => {
            if let Some(mut pin) = Pin::load(dir) {
                pin.enabled = false;
                pin.save(dir)?;
            }
            Ok(None)
        }
        Some(link) => {
            let secs = now_ms() / 1000;
            let pin = Pin::from_link(link, secs)?;
            pin.save(dir)?;
            Ok(Some(pin))
        }
        None => Ok(Pin::load(dir).filter(|p| p.enabled)),
    }
}

// --- the queue -----------------------------------------------------------------------------------------------------

/// Messages waiting for the conductor. Sequence numbers are stamped when a message leaves, so what the queue
/// collapses or drops never leaves a gap.
#[derive(Default)]
pub struct Outbox {
    queue: VecDeque<(NodeMessage, usize)>,
    bytes: usize,
    dropped: u32,
}

impl Outbox {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Messages dropped since the last time this was asked, to report in the next `health`.
    pub fn take_dropped(&mut self) -> u32 {
        std::mem::take(&mut self.dropped)
    }

    pub fn push(&mut self, msg: NodeMessage) {
        match &msg {
            // A raise followed by its clear while the conductor was away is nothing it needs to hear: its snapshot
            // on reconnect already lacks the item.
            NodeMessage::Cleared { id, .. } => {
                if let Some(at) = self.queue.iter().position(
                    |(m, _)| matches!(m, NodeMessage::Raised { obligation, .. } if &obligation.id == id),
                ) {
                    self.remove(at);
                    return;
                }
            }
            // Usage rows are changes since the last report: they add up, they do not replace.
            NodeMessage::Usage { rows, .. } => {
                if let Some((NodeMessage::Usage { rows: held, .. }, _)) = self
                    .queue
                    .iter_mut()
                    .find(|(m, _)| matches!(m, NodeMessage::Usage { .. }))
                {
                    held.extend(rows.iter().cloned());
                    held.truncate(MAX_USAGE_ROWS);
                    let size = size_of(&NodeMessage::Usage {
                        epoch: 0,
                        seq: 0,
                        rows: held.clone(),
                    });
                    let at = self
                        .queue
                        .iter()
                        .position(|(m, _)| matches!(m, NodeMessage::Usage { .. }))
                        .unwrap();
                    self.bytes = self.bytes - self.queue[at].1 + size;
                    self.queue[at].1 = size;
                    return;
                }
            }
            _ => {}
        }
        let size = size_of(&msg);
        self.bytes += size;
        self.queue.push_back((msg, size));
        while self.queue.len() > QUEUE_MESSAGES || self.bytes > QUEUE_BYTES {
            // Oldest health first, then usage, then raises; only then whatever is oldest.
            let at = [
                |m: &NodeMessage| matches!(m, NodeMessage::Health { .. }),
                |m: &NodeMessage| matches!(m, NodeMessage::Usage { .. }),
                |m: &NodeMessage| matches!(m, NodeMessage::Raised { .. }),
            ]
            .iter()
            .find_map(|is| self.queue.iter().position(|(m, _)| is(m)))
            .unwrap_or(0);
            self.remove(at);
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    fn remove(&mut self, at: usize) {
        if let Some((_, size)) = self.queue.remove(at) {
            self.bytes -= size;
        }
    }

    fn pop(&mut self) -> Option<NodeMessage> {
        let (m, size) = self.queue.pop_front()?;
        self.bytes -= size;
        Some(m)
    }
}

fn size_of(m: &NodeMessage) -> usize {
    serde_json::to_string(m).map_or(0, |s| s.len())
}

// --- what the node tells ----------------------------------------------------------------------------------------

fn obligation_of(a: &Attention) -> Obligation {
    Obligation {
        id: a.id.clone(),
        workspace: a.workspace.clone(),
        tab: a.tab.clone(),
        kind: a.kind,
        raised_at: a.raised_at,
        urgency: None,
        class_features: Vec::new(),
        task: a.task.as_ref().map(|t| t.title.clone()),
        prompt: a.prompt.clone(),
    }
    .capped()
}

/// Turns the open list into changes: what appeared is raised, what left is cleared.
#[derive(Default)]
struct Differ {
    known: HashSet<String>,
}

impl Differ {
    fn changes(&mut self, epoch: u64, items: &[Attention]) -> Vec<NodeMessage> {
        let mut out = Vec::new();
        for a in items {
            if self.known.insert(a.id.clone()) {
                out.push(NodeMessage::Raised {
                    epoch,
                    seq: 0,
                    obligation: obligation_of(a),
                });
            }
        }
        let open: HashSet<&str> = items.iter().map(|a| a.id.as_str()).collect();
        let gone: Vec<String> = self
            .known
            .iter()
            .filter(|id| !open.contains(id.as_str()))
            .cloned()
            .collect();
        for id in gone {
            self.known.remove(&id);
            out.push(NodeMessage::Cleared {
                epoch,
                seq: 0,
                id,
                by: termenbox_core::conductor::ClearedBy::Fact,
            });
        }
        out
    }

    /// After a snapshot the conductor knows exactly this list.
    fn reset(&mut self, items: &[Attention]) {
        self.known = items.iter().map(|a| a.id.clone()).collect();
    }
}

fn stamp(m: NodeMessage, seq: u64) -> NodeMessage {
    match m {
        NodeMessage::Raised {
            epoch, obligation, ..
        } => NodeMessage::Raised {
            epoch,
            seq,
            obligation,
        },
        NodeMessage::Cleared { epoch, id, by, .. } => NodeMessage::Cleared { epoch, seq, id, by },
        NodeMessage::Acked {
            epoch,
            id,
            actor,
            owner,
            ..
        } => NodeMessage::Acked {
            epoch,
            seq,
            id,
            actor,
            owner,
        },
        NodeMessage::Answered {
            epoch,
            id,
            actor,
            owner,
            device,
            option,
            ..
        } => NodeMessage::Answered {
            epoch,
            seq,
            id,
            actor,
            owner,
            device,
            option,
        },
        NodeMessage::Usage { epoch, rows, .. } => NodeMessage::Usage { epoch, seq, rows },
        NodeMessage::Incident {
            epoch,
            cause,
            workspace,
            detail,
            ..
        } => NodeMessage::Incident {
            epoch,
            seq,
            cause,
            workspace,
            detail,
        },
        NodeMessage::Health {
            epoch,
            agents,
            working,
            waiting,
            load,
            queue_dropped,
            ..
        } => NodeMessage::Health {
            epoch,
            seq,
            agents,
            working,
            waiting,
            load,
            queue_dropped,
        },
        other => other,
    }
}

// --- the loop --------------------------------------------------------------------------------------------------------

pub struct Node {
    pub key: ServerKey,
    pub name: String,
    pub version: String,
    pub hub: EventHub,
}

struct State {
    epoch: u64,
    seq: u64,
    outbox: Outbox,
    differ: Differ,
}

#[derive(PartialEq, Eq, Debug)]
enum Ended {
    /// The connection went away; dial again.
    Lost,
    /// The conductor removed this node: stop.
    Disabled,
}

/// Keeps the link up for as long as the process runs, unless the conductor removes the node.
pub async fn run(dir: PathBuf, mut pin: Pin, node: Node, mut backoff: Backoff) {
    let mut rx = node.hub.subscribe_status();
    let mut st = State {
        epoch: rand::random::<u64>() >> 1,
        seq: 0,
        outbox: Outbox::default(),
        differ: Differ::default(),
    };
    // Whatever is open now reaches the conductor in the first snapshot.
    st.differ.reset(&node.hub.attention_items());
    loop {
        let started = tokio::time::Instant::now();
        for url in pin.urls.clone() {
            let Ok(Ok((ws, _))) =
                tokio::time::timeout(STEP, tokio_tungstenite::connect_async(url.as_str())).await
            else {
                continue;
            };
            match session(ws, &mut pin, &dir, &node, &mut st, &mut rx).await {
                Ok(Ended::Disabled) => {
                    tracing::info!("conductor removed this node; the link stays off");
                    pin.enabled = false;
                    let _ = pin.save(&dir);
                    return;
                }
                Ok(Ended::Lost) => {}
                Err(e) => tracing::debug!("conductor link: {e:#}"),
            }
            break;
        }
        if started.elapsed() >= Duration::from_secs(30) {
            backoff.reset();
        }
        // Away: keep noting changes, bounded, until it is time to dial again.
        let until = tokio::time::Instant::now() + backoff.next();
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(until) => break,
                got = rx.recv() => note(&node, &mut st, got),
            }
        }
    }
}

fn note(node: &Node, st: &mut State, got: Result<StatusChange, RecvError>) {
    let items = match got {
        Ok(StatusChange::Attention(items)) => items,
        Ok(_) => return,
        Err(RecvError::Lagged(_)) => node.hub.attention_items(),
        Err(RecvError::Closed) => return,
    };
    for m in st.differ.changes(st.epoch, &items) {
        st.outbox.push(m);
    }
}

fn health(node: &Node, st: &mut State) -> NodeMessage {
    let states = node.hub.snapshot();
    NodeMessage::Health {
        epoch: st.epoch,
        seq: 0,
        agents: states.len() as u32,
        working: states
            .iter()
            .filter(|s| s.state == AgentState::Working)
            .count() as u32,
        waiting: states
            .iter()
            .filter(|s| s.state == AgentState::NeedsInput)
            .count() as u32,
        load: 0,
        queue_dropped: st.outbox.take_dropped(),
    }
}

async fn session<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    pin: &mut Pin,
    dir: &Path,
    node: &Node,
    st: &mut State,
    rx: &mut tokio::sync::broadcast::Receiver<StatusChange>,
) -> Result<Ended>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut tx, mut inbound) = ws.split();
    send_json(&mut tx, &ClientMessage::Hello { protocol: PROTOCOL }).await?;
    let nonce = match recv::<ServerMessage, _>(&mut inbound).await? {
        ServerMessage::Challenge { nonce, server_id } => {
            anyhow::ensure!(
                server_id == pin.conductor,
                "the peer is not the pinned conductor"
            );
            nonce
        }
        _ => anyhow::bail!("no challenge"),
    };
    let sig = URL_SAFE_NO_PAD.encode(node.key.sign(nonce.as_bytes()));
    send_json(
        &mut tx,
        &NodeMessage::Hello {
            v: PROTOCOL,
            node: node.key.server_id(),
            name: node.name.clone(),
            epoch: st.epoch,
            version: node.version.clone(),
            caps: Vec::new(),
            enrol: pin.enrol.clone(),
            sig,
        }
        .capped(),
    )
    .await?;
    match recv::<ConductorMessage, _>(&mut inbound).await? {
        ConductorMessage::Welcome { .. } => {}
        ConductorMessage::Disable => return Ok(Ended::Disabled),
        _ => anyhow::bail!("no welcome"),
    }
    // The secret is single use: once welcomed, forget it.
    if pin.enrol.take().is_some() {
        pin.save(dir)?;
    }
    tracing::info!("conductor link up");

    // A snapshot replaces the conductor's view, so queued raises and clears it already reflects are dropped.
    let snapshot = |node: &Node, st: &mut State| {
        let items = node.hub.attention_items();
        st.differ.reset(&items);
        NodeMessage::Snapshot {
            epoch: st.epoch,
            items: items.iter().map(obligation_of).collect(),
            workspaces: Vec::new(),
        }
    };
    st.outbox
        .queue
        .retain(|(m, _)| !matches!(m, NodeMessage::Raised { .. } | NodeMessage::Cleared { .. }));
    st.outbox.bytes = st.outbox.queue.iter().map(|(_, s)| s).sum();
    let first = snapshot(node, st);
    send_json(&mut tx, &first).await?;
    flush(&mut tx, st).await?;

    let mut beat = tokio::time::interval(HEARTBEAT);
    loop {
        tokio::select! {
            got = rx.recv() => {
                note(node, st, got);
                flush(&mut tx, st).await?;
            }
            _ = beat.tick() => {
                let h = health(node, st);
                st.outbox.push(h);
                flush(&mut tx, st).await?;
            }
            msg = inbound.next() => match msg {
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<ConductorMessage>(&text) {
                    Ok(ConductorMessage::Resync) => {
                        let s = snapshot(node, st);
                        send_json(&mut tx, &s).await?;
                    }
                    Ok(ConductorMessage::Disable) => return Ok(Ended::Disabled),
                    // Answers, acks and terminal opens arrive with steps 2.4 and 2.6.
                    Ok(_) | Err(_) => {}
                },
                Some(Ok(Message::Ping(p))) => tx.send(Message::Pong(p)).await?,
                Some(Ok(Message::Close(_))) | None => return Ok(Ended::Lost),
                // A conductor sends no binary frames; ignore them rather than act on them.
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
            },
        }
    }
}

async fn flush<T>(tx: &mut T, st: &mut State) -> Result<()>
where
    T: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    while let Some(m) = st.outbox.pop() {
        st.seq += 1;
        let m = stamp(m, st.seq).capped();
        send_json(tx, &m).await?;
    }
    Ok(())
}

async fn send_json<T, M>(tx: &mut T, m: &M) -> Result<()>
where
    T: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    M: Serialize,
{
    tx.send(Message::Text(serde_json::to_string(m)?)).await?;
    Ok(())
}

async fn recv<M, R>(inbound: &mut R) -> Result<M>
where
    M: for<'a> Deserialize<'a>,
    R: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let next = tokio::time::timeout(STEP, inbound.next())
            .await
            .context("timed out")?;
        match next {
            Some(Ok(Message::Text(t))) => {
                return serde_json::from_str(&t).context("unreadable message")
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            Some(Ok(_)) | None => anyhow::bail!("closed"),
            Some(Err(e)) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termenbox_core::bundle::Candidate;
    use termenbox_core::conductor::UsageRow;
    use termenbox_core::events::{Confidence, EventKind, Source};
    use tokio::net::TcpListener;

    fn tmpdir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("termenbox-conductor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn raised(id: &str) -> NodeMessage {
        NodeMessage::Raised {
            epoch: 1,
            seq: 0,
            obligation: Obligation {
                id: id.into(),
                workspace: "w".into(),
                tab: None,
                kind: termenbox_core::events::AttentionKind::NeedsInput,
                raised_at: 1,
                urgency: None,
                class_features: vec![],
                task: None,
                prompt: Some("p".into()),
            },
        }
    }

    fn health_msg() -> NodeMessage {
        NodeMessage::Health {
            epoch: 1,
            seq: 0,
            agents: 1,
            working: 0,
            waiting: 0,
            load: 0,
            queue_dropped: 0,
        }
    }

    #[test]
    fn conductor_link_queue_is_bounded_by_messages_and_counts_drops() {
        let mut q = Outbox::default();
        for i in 0..(QUEUE_MESSAGES + 40) {
            q.push(raised(&format!("a{i}")));
        }
        assert_eq!(q.len(), QUEUE_MESSAGES);
        assert_eq!(q.take_dropped(), 40);
        assert_eq!(q.take_dropped(), 0);
    }

    #[test]
    fn conductor_link_queue_is_bounded_by_bytes() {
        let mut q = Outbox::default();
        for i in 0..400 {
            let mut m = raised(&format!("a{i}"));
            if let NodeMessage::Raised { obligation, .. } = &mut m {
                obligation.prompt = Some("x".repeat(500));
            }
            q.push(m);
        }
        assert!(q.bytes() <= QUEUE_BYTES, "{}", q.bytes());
        assert!(q.take_dropped() > 0);
    }

    #[test]
    fn conductor_link_drops_health_before_raises() {
        let mut q = Outbox::default();
        q.push(health_msg());
        for i in 0..QUEUE_MESSAGES {
            q.push(raised(&format!("a{i}")));
        }
        assert!(!q
            .queue
            .iter()
            .any(|(m, _)| matches!(m, NodeMessage::Health { .. })));
        assert!(q
            .queue
            .iter()
            .all(|(m, _)| matches!(m, NodeMessage::Raised { .. })));
    }

    #[test]
    fn conductor_link_a_raise_and_its_clear_cancel_while_away() {
        let mut q = Outbox::default();
        q.push(raised("a1"));
        q.push(NodeMessage::Cleared {
            epoch: 1,
            seq: 0,
            id: "a1".into(),
            by: termenbox_core::conductor::ClearedBy::Fact,
        });
        assert_eq!(q.len(), 0);
        assert_eq!(q.bytes(), 0);
    }

    #[test]
    fn conductor_link_usage_rows_add_up_in_the_queue() {
        let mut q = Outbox::default();
        let row = UsageRow {
            workspace: "w".into(),
            ..Default::default()
        };
        q.push(NodeMessage::Usage {
            epoch: 1,
            seq: 0,
            rows: vec![row.clone()],
        });
        q.push(NodeMessage::Usage {
            epoch: 1,
            seq: 0,
            rows: vec![row],
        });
        assert_eq!(q.len(), 1);
        let NodeMessage::Usage { rows, .. } = &q.queue[0].0 else {
            panic!()
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(q.bytes(), q.queue[0].1);
    }

    fn signed_link(key: &ServerKey, port: u16, extra: &str) -> String {
        let bundle = Bundle {
            server_id: key.server_id(),
            seq: 1,
            expires: now_ms() / 1000 + 600,
            published: false,
            fingerprint: "sha-256 AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB:AB".into(),
            name: "conductor".into(),
            candidates: vec![Candidate {
                kind: CandidateKind::Lan,
                addr: "127.0.0.1".into(),
                port,
            }],
            signal: None,
            relay: false,
        };
        let wire = bundle
            .encode(&key.sign(&bundle.signed_bytes().unwrap()))
            .unwrap();
        format!("https://client.example/#/pair/{wire}{extra}")
    }

    #[test]
    fn conductor_link_pin_comes_from_a_signed_link_and_keeps_the_secret_out_of_the_bundle() {
        let key = ServerKey::from_seed([3; 32]);
        let pin =
            Pin::from_link(&signed_link(&key, 4000, "?enrol=s3cret"), now_ms() / 1000).unwrap();
        assert_eq!(pin.conductor, key.server_id());
        assert_eq!(pin.urls, vec!["ws://127.0.0.1:4000/ws".to_string()]);
        assert_eq!(pin.enrol.as_deref(), Some("s3cret"));
        let mut forged = signed_link(&key, 4000, "");
        forged.insert(forged.len() - 3, 'A');
        assert!(Pin::from_link(&forged, now_ms() / 1000).is_err());
    }

    #[test]
    fn conductor_link_is_off_unless_configured_and_off_disables_it() {
        let dir = tmpdir();
        assert!(configure(dir.as_path(), None).unwrap().is_none());
        let key = ServerKey::from_seed([3; 32]);
        let on = configure(dir.as_path(), Some(&signed_link(&key, 4000, ""))).unwrap();
        assert!(on.is_some());
        assert!(configure(dir.as_path(), None).unwrap().is_some());
        assert!(configure(dir.as_path(), Some("off")).unwrap().is_none());
        assert!(configure(dir.as_path(), None).unwrap().is_none());
    }

    /// Accepts one node: runs the challenge, welcomes it, and hands back the frames it sent, up to `until`.
    async fn fake_conductor(
        listener: &TcpListener,
        key: &ServerKey,
        take: usize,
    ) -> (
        Vec<Message>,
        tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        let Some(Ok(Message::Text(hello))) = ws.next().await else {
            panic!("no hello")
        };
        assert!(matches!(
            serde_json::from_str::<ClientMessage>(&hello).unwrap(),
            ClientMessage::Hello { .. }
        ));
        let challenge = ServerMessage::Challenge {
            nonce: "n0nce-n0nce-n0nce-n0nce".into(),
            server_id: key.server_id(),
        };
        ws.send(Message::Text(serde_json::to_string(&challenge).unwrap()))
            .await
            .unwrap();
        let mut frames = Vec::new();
        let Some(Ok(first)) = ws.next().await else {
            panic!("no node hello")
        };
        frames.push(first);
        let welcome = ConductorMessage::Welcome {
            epoch_ack: 0,
            heartbeat_s: 30,
            conductor_epoch: 1,
        };
        ws.send(Message::Text(serde_json::to_string(&welcome).unwrap()))
            .await
            .unwrap();
        while frames.len() < take {
            let got = tokio::time::timeout(Duration::from_secs(5), ws.next())
                .await
                .expect("frames");
            let Some(Ok(m)) = got else { break };
            frames.push(m);
        }
        (frames, ws)
    }

    fn parse(m: &Message) -> NodeMessage {
        match m {
            Message::Text(t) => serde_json::from_str(t).unwrap(),
            other => panic!("the link sent a non-text frame: {other:?}"),
        }
    }

    #[tokio::test]
    async fn conductor_link_reconnects_and_sends_no_terminal_bytes() {
        let conductor_key = ServerKey::from_seed([9; 32]);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = tmpdir();
        let pin = Pin::from_link(
            &signed_link(&conductor_key, port, "?enrol=once"),
            now_ms() / 1000,
        )
        .unwrap();
        pin.save(dir.as_path()).unwrap();

        let node_key = ServerKey::from_seed([5; 32]);
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
        let node = Node {
            key: ServerKey::from_seed([5; 32]),
            name: "vps".into(),
            version: "t".into(),
            hub: hub.clone(),
        };
        tokio::spawn(run(
            dir.clone(),
            pin,
            node,
            Backoff::new(Duration::from_millis(20), Duration::from_millis(50)),
        ));

        // First connection: hello (signed, with the secret), then a snapshot holding the open ask.
        let (frames, ws) = fake_conductor(&listener, &conductor_key, 2).await;
        let NodeMessage::Hello {
            node,
            enrol,
            sig,
            epoch,
            ..
        } = parse(&frames[0])
        else {
            panic!("hello first")
        };
        assert_eq!(node, node_key.server_id());
        assert_eq!(enrol.as_deref(), Some("once"));
        assert!(!sig.is_empty());
        let NodeMessage::Snapshot {
            items, epoch: e1, ..
        } = parse(&frames[1])
        else {
            panic!("snapshot second")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(e1, epoch);
        // The secret is single use: gone from the pin once welcomed.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(Pin::load(dir.as_path()).unwrap().enrol, None);

        // The conductor goes away; the ask clears meanwhile. On return the snapshot is empty and nothing stale is replayed.
        drop(ws);
        tokio::time::sleep(Duration::from_millis(30)).await;
        hub.push(
            "w",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::Idle,
            },
            now_ms(),
        );
        let (frames, _ws) = fake_conductor(&listener, &conductor_key, 2).await;
        let NodeMessage::Hello {
            enrol, epoch: e2, ..
        } = parse(&frames[0])
        else {
            panic!("hello first")
        };
        assert_eq!(enrol, None);
        assert_eq!(e2, epoch, "the epoch lasts for the process");
        let NodeMessage::Snapshot { items, .. } = parse(&frames[1]) else {
            panic!("snapshot second")
        };
        assert!(items.is_empty());
        // Every frame is text that parses as a NodeMessage: no terminal bytes cross the link.
        assert!(frames.iter().all(|f| matches!(f, Message::Text(_))));
    }

    #[tokio::test]
    async fn conductor_link_refuses_a_peer_that_is_not_the_pinned_conductor() {
        let pinned = ServerKey::from_seed([9; 32]);
        let impostor = ServerKey::from_seed([8; 32]);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = tmpdir();
        let pin = Pin::from_link(&signed_link(&pinned, port, ""), now_ms() / 1000).unwrap();
        let node = Node {
            key: ServerKey::from_seed([5; 32]),
            name: "vps".into(),
            version: "t".into(),
            hub: EventHub::new([7; 32]),
        };
        tokio::spawn(run(
            dir.clone(),
            pin,
            node,
            Backoff::new(Duration::from_millis(20), Duration::from_millis(50)),
        ));
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        let _hello = ws.next().await;
        let challenge = ServerMessage::Challenge {
            nonce: "n".into(),
            server_id: impostor.server_id(),
        };
        ws.send(Message::Text(serde_json::to_string(&challenge).unwrap()))
            .await
            .unwrap();
        // The node closes without sending its identity.
        let next = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .unwrap();
        assert!(!matches!(next, Some(Ok(Message::Text(_)))), "{next:?}");
    }
}
