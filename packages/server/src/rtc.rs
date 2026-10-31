//! WebRTC-direct transport (DECISION-0057): one UDP port, ICE-lite, no signaling, no STUN/TURN. An assisted session
//! (DECISION-0141) shares the port but is a full ICE agent that starts its own checks, so it can punch a NAT.
//!
//! The browser builds the remote workspace description itself from the server's IP, port and DTLS
//! fingerprint (from the signed directory record) and rewrites its own ICE ufrag/pwd to one random
//! string. A peer is created when an unknown source sends a STUN binding request whose username is
//! `termenbox:<client ufrag>`. The client's certificate is not verified (identity comes from the
//! GitHub login inside the channel); the client verifies ours against the pinned fingerprint.
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Serialize;
use sha2::{Digest, Sha256};
use str0m::channel::{ChannelConfig, ChannelId};
use str0m::config::{DtlsCert, Fingerprint};
use str0m::ice::IceCreds;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot, Notify};
use tracing::{debug, warn};

use crate::link::{self, Inbound, LinkEnd};
use crate::AppState;

/// Fixed by the protocol: the client puts these into the description it builds locally.
pub const SERVER_UFRAG: &str = "termenbox";
pub const SERVER_PWD: &str = "termenbox-direct-v1-0123456789";
/// Negotiated data channel id, the same on both sides (no in-band negotiation).
pub const CHANNEL_ID: u16 = 0;
/// The second negotiated channel, for terminal bytes (decision 0096); unused by a client that did not ask.
pub const TERM_CHANNEL_ID: u16 = 1;
/// The third negotiated channel, for control messages (decision 0139): ordered and reliable like the first, but with
/// its own queue and SCTP stream, so a cancel never waits behind agent events or terminal bytes. A client that does
/// not open it keeps control on the first channel.
pub const CONTROL_CHANNEL_ID: u16 = 2;
/// Queued terminal chunks are joined into messages of about this size (the SCTP limit is 64 KiB).
const TERM_MESSAGE: usize = 16 * 1024;
/// A single message above this closes the connection; clients send small JSON.
const MAX_MESSAGE: usize = 64 * 1024;
/// Sent in place of a reply longer than `MAX_MESSAGE`.
const OVERSIZE_REPLY: &str =
    r#"{"type":"error","message":"A reply was too large for this connection and was not sent."}"#;
/// Client messages the workspace has not read yet wait here, bounded by bytes: a terminal answers a palette query with
/// one message per colour (about 260 at once after an attach), which must not cost the peer its connection.
const BACKLOG_BYTES: usize = 256 * 1024;
const BACKLOG_MESSAGES: usize = 4096;

#[derive(Clone, Debug)]
pub struct Limits {
    pub max_peers: usize,
    /// Peers that have not opened the data channel yet (about 25 KB each).
    pub max_half_open: usize,
    pub max_per_ip: usize,
    pub half_open_ttl: Duration,
    /// No datagram at all for this long means the peer is gone.
    pub silence_ttl: Duration,
    pub new_peers_per_sec: u32,
    /// Assisted sessions (punched, started from an authenticated signal) alive at once.
    pub max_assisted: usize,
    /// A punch that has not opened the data channel by then is dropped (a direct peer gets `half_open_ttl`).
    pub assisted_ttl: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_peers: 256,
            max_half_open: 64,
            max_per_ip: 8,
            half_open_ttl: Duration::from_secs(10),
            silence_ttl: Duration::from_secs(30),
            new_peers_per_sec: 50,
            max_assisted: 8,
            assisted_ttl: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RtcInfo {
    pub ip: IpAddr,
    pub port: u16,
    pub fingerprint: String,
}

/// What the signal channel (step 2.5) hands over for a punched session: the client's ICE credentials and the addresses
/// it can be reached at, plus the server's own candidates beyond the transport socket's address (the STUN-mapped
/// address, a relay address).
pub struct AssistedOffer {
    pub ufrag: String,
    pub pwd: String,
    pub candidates: Vec<SocketAddr>,
    pub local: Vec<Candidate>,
    /// An allocation on the owner's TURN key (`turn::allocate_or_none`), offered as a relay candidate. The session
    /// owns it and releases it when it ends.
    pub relay: Option<crate::turn::Relay>,
}

/// The server's ICE credentials for the answer. `ufrag` also names the session in `candidate` and `cancel`.
#[derive(Clone, Debug)]
pub struct AssistedAnswer {
    pub ufrag: String,
    pub pwd: String,
}

#[allow(dead_code)] // cancel and active are used by the tests; a session that ends early will cancel (step 2.8)
enum Command {
    Start(AssistedOffer, oneshot::Sender<Result<AssistedAnswer>>),
    Candidate(String, SocketAddr),
    Cancel(String),
    Count(oneshot::Sender<usize>),
}

/// Starts and steers assisted sessions on the transport's socket. Unlike a direct peer (ICE-lite, created by an
/// unknown source's first check), an assisted session is a full ICE agent that starts its own checks toward the
/// client's candidates, which is what a NAT punch needs.
#[derive(Clone, Debug)]
pub struct Assisted {
    tx: mpsc::UnboundedSender<Command>,
    relay_inbox: mpsc::UnboundedSender<crate::turn::Packet>,
}

impl Assisted {
    /// A handle with no transport behind it: `start` fails, everything else is ignored.
    #[cfg(test)]
    pub fn detached() -> Assisted {
        let (tx, _) = mpsc::unbounded_channel();
        let (relay_inbox, _) = mpsc::unbounded_channel();
        Assisted { tx, relay_inbox }
    }

    /// Where a relay allocation delivers what peers send to it: pass this to `turn::allocate`.
    pub fn relay_inbox(&self) -> mpsc::UnboundedSender<crate::turn::Packet> {
        self.relay_inbox.clone()
    }

    pub async fn start(&self, offer: AssistedOffer) -> Result<AssistedAnswer> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Start(offer, reply))
            .map_err(|_| anyhow!("the transport is stopped"))?;
        rx.await.map_err(|_| anyhow!("the transport is stopped"))?
    }

    /// A candidate the client found after its offer.
    pub fn candidate(&self, session: &str, addr: SocketAddr) {
        let _ = self.tx.send(Command::Candidate(session.into(), addr));
    }

    /// Ends a session; unknown names are ignored.
    #[allow(dead_code)] // cancel and active are used by the tests; a session that ends early will cancel (step 2.8)
    pub fn cancel(&self, session: &str) {
        let _ = self.tx.send(Command::Cancel(session.into()));
    }

    #[allow(dead_code)] // cancel and active are used by the tests; a session that ends early will cancel (step 2.8)
    pub async fn active(&self) -> usize {
        let (reply, rx) = oneshot::channel();
        if self.tx.send(Command::Count(reply)).is_err() {
            return 0;
        }
        rx.await.unwrap_or(0)
    }
}

/// Loads the DTLS certificate from `dir`, creating it on first start. The fingerprint must stay the
/// same across restarts because clients pin it.
pub fn load_or_create_cert(dir: &Path) -> Result<DtlsCert> {
    let (cert_path, key_path) = (dir.join("dtls_cert.der"), dir.join("dtls_key.der"));
    if let (Ok(certificate), Ok(private_key)) =
        (std::fs::read(&cert_path), std::fs::read(&key_path))
    {
        return Ok(DtlsCert {
            certificate,
            private_key,
        });
    }
    let cert = str0m::crypto::from_feature_flags()
        .dtls_provider
        .generate_certificate()
        .ok_or_else(|| anyhow!("cannot generate a DTLS certificate"))?;
    write_private(&cert_path, &cert.certificate)?;
    write_private(&key_path, &cert.private_key)?;
    Ok(cert)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    opts.mode(0o600);
    opts.open(path)?.write_all(bytes)?;
    Ok(())
}

pub fn fingerprint(cert: &DtlsCert) -> String {
    let digest = Sha256::digest(&cert.certificate);
    let hex: Vec<String> = digest.iter().map(|b| format!("{b:02X}")).collect();
    format!("sha-256 {}", hex.join(":"))
}

/// The UDP range user installs share (DECISION-0118): one firewall rule serves every instance on a
/// machine, and the server, which owns the socket, picks the port.
pub const UDP_PORT_RANGE: RangeInclusive<u16> = 4443..=4542;

/// Binds the socket the transport will use, holding it so nothing can take the port in between
/// (DECISION-0118). A port the owner chose (`explicit`) or that an earlier start chose (`saved`) is the
/// port: it is used as given and refused clearly when taken, never swapped for another, because the
/// firewall rule and the directory record are for that one. Only a first start scans `ports` for a free
/// one. Returns the socket and the port it bound.
pub async fn bind_transport(
    ip: IpAddr,
    explicit: Option<u16>,
    ports: RangeInclusive<u16>,
    saved: Option<u16>,
) -> Result<(UdpSocket, u16)> {
    let fixed = explicit
        .map(|p| (p, "requested (--udp-port/TERMENBOX_UDP_PORT)"))
        .or(saved.map(|p| (p, "saved by the first start")));
    if let Some((port, source)) = fixed {
        let socket = UdpSocket::bind(SocketAddr::new(ip, port))
            .await
            .map_err(|e| {
                anyhow!(
                    "UDP port {port} ({source}) is not available: {e}. Free it, or choose another \
                     with TERMENBOX_UDP_PORT (then open that port in the firewall) and restart"
                )
            })?;
        let bound = socket.local_addr()?.port();
        return Ok((socket, bound));
    }
    let (first, last) = (*ports.start(), *ports.end());
    for port in ports {
        match UdpSocket::bind(SocketAddr::new(ip, port)).await {
            Ok(socket) => return Ok((socket, port)),
            // Only "someone else has it" means try the next one. Any other error (a permission or
            // resource problem) would repeat for every port, so report its cause instead of hiding it
            // behind "no free UDP port".
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(e) => return Err(anyhow!("UDP port {port} could not be bound: {e}")),
        }
    }
    Err(anyhow!(
        "no free UDP port in {first}..={last}; free one, or choose a port with TERMENBOX_UDP_PORT \
         (and open it in the firewall)"
    ))
}

/// Starts on an already-bound socket; its local port is what clients and the directory record get.
pub async fn start_with_socket(
    socket: UdpSocket,
    public_ip: IpAddr,
    cert: DtlsCert,
    limits: Limits,
    state: AppState,
) -> Result<RtcInfo> {
    start_shared(Arc::new(socket), public_ip, cert, limits, state, None).await
}

/// `start_with_socket` on a socket something else also uses: with a `stun` inbox, STUN Binding responses received on
/// it go to the STUN discovery (`stun.rs`) instead of the ICE agents.
pub async fn start_shared(
    socket: Arc<UdpSocket>,
    public_ip: IpAddr,
    cert: DtlsCert,
    limits: Limits,
    state: AppState,
    stun: Option<crate::stun::Inbox>,
) -> Result<RtcInfo> {
    Ok(start_assisted(socket, public_ip, cert, limits, state, stun)
        .await?
        .0)
}

/// `start_shared` that also returns the handle for assisted sessions.
pub async fn start_assisted(
    socket: Arc<UdpSocket>,
    public_ip: IpAddr,
    cert: DtlsCert,
    limits: Limits,
    state: AppState,
    stun: Option<crate::stun::Inbox>,
) -> Result<(RtcInfo, Assisted)> {
    let port = socket.local_addr()?.port();
    let (tx, cmds) = mpsc::unbounded_channel();
    let (relay_inbox, relay_rx) = mpsc::unbounded_channel();
    let info = RtcInfo {
        ip: public_ip,
        port,
        fingerprint: fingerprint(&cert),
    };
    let server = Server {
        socket,
        public_addr: SocketAddr::new(public_ip, port),
        cert,
        limits,
        state,
        wake: Arc::new(Notify::new()),
        stun,
        cmds,
        relay_rx,
    };
    tokio::spawn(server.run());
    Ok((info, Assisted { tx, relay_inbox }))
}

struct Server {
    socket: Arc<UdpSocket>,
    public_addr: SocketAddr,
    cert: DtlsCert,
    limits: Limits,
    state: AppState,
    wake: Arc<Notify>,
    stun: Option<crate::stun::Inbox>,
    cmds: mpsc::UnboundedReceiver<Command>,
    relay_rx: mpsc::UnboundedReceiver<crate::turn::Packet>,
}

struct Peer {
    rtc: Rtc,
    /// Present once the data channel is open (the workspace task starts then).
    end: Option<LinkEnd>,
    cid: Option<ChannelId>,
    term_cid: Option<ChannelId>,
    ctl_cid: Option<ChannelId>,
    /// Control replies taken from the workspace that the channel had no room for yet.
    control_queue: VecDeque<String>,
    /// Terminal bytes taken from the workspace that the channel had no room for yet.
    term_pending: Option<Vec<u8>>,
    created: Instant,
    /// How long the data channel may stay unopened.
    open_ttl: Duration,
    /// A full ICE agent starts its own checks, so it needs the clock advanced; an ICE-lite one only answers.
    full_ice: bool,
    /// The TURN allocation of an assisted session, if it has one; its packets are sent through it.
    relay: Option<crate::turn::Relay>,
    last_rx: Instant,
    queue: VecDeque<String>,
    /// Inbound messages the workspace had no room for yet.
    backlog: VecDeque<Inbound>,
    /// Inbound control messages (the control channel) the workspace had no room for yet; handed over first.
    control_backlog: VecDeque<Inbound>,
    backlog_bytes: usize,
    closing: bool,
    dirty: bool,
    timeout: Instant,
}

/// Fixed-rate token bucket for creating peers (a flood of STUN requests must not cost memory).
struct Bucket {
    tokens: f64,
    rate: f64,
    last: Instant,
}

impl Bucket {
    fn new(rate: u32) -> Self {
        Self {
            tokens: rate as f64,
            rate: rate as f64,
            last: Instant::now(),
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        self.tokens =
            (self.tokens + now.duration_since(self.last).as_secs_f64() * self.rate).min(self.rate);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// USERNAME attribute of a STUN binding request, if `buf` is one.
fn stun_username(buf: &[u8]) -> Option<&str> {
    if buf.len() < 20 || buf[0] != 0 || buf[1] != 1 || buf[4..8] != [0x21, 0x12, 0xA4, 0x42] {
        return None;
    }
    let mut i = 20;
    while i + 4 <= buf.len() {
        let t = u16::from_be_bytes([buf[i], buf[i + 1]]);
        let l = u16::from_be_bytes([buf[i + 2], buf[i + 3]]) as usize;
        if i + 4 + l > buf.len() {
            return None;
        }
        if t == 0x0006 {
            return std::str::from_utf8(&buf[i + 4..i + 4 + l]).ok();
        }
        i += 4 + l + ((4 - l % 4) % 4);
    }
    None
}

fn valid_client_ufrag(u: &str) -> bool {
    (16..=64).contains(&u.len()) && u.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl Server {
    async fn run(mut self) {
        let mut peers: HashMap<SocketAddr, Peer> = HashMap::new();
        // Assisted sessions by the server's ICE ufrag; `routes` learns which source address belongs to which.
        let mut assisted: HashMap<String, Peer> = HashMap::new();
        let mut routes: HashMap<SocketAddr, String> = HashMap::new();
        let mut commands_open = true;
        let mut bucket = Bucket::new(self.limits.new_peers_per_sec);
        let mut buf = vec![0u8; 2000];
        loop {
            let now = Instant::now();
            let mut next = now + Duration::from_secs(1);
            peers.retain(|addr, p| {
                if p.dirty || now >= p.timeout {
                    p.dirty = false;
                    if !p.step(&self.socket, now, &self.limits, &self.state, &self.wake) {
                        debug!("peer {addr} gone");
                        return false;
                    }
                }
                next = next.min(p.timeout);
                true
            });
            assisted.retain(|name, p| {
                if p.dirty || now >= p.timeout {
                    p.dirty = false;
                    if !p.step(&self.socket, now, &self.limits, &self.state, &self.wake) {
                        debug!("assisted session {name} gone");
                        return false;
                    }
                }
                next = next.min(p.timeout);
                true
            });
            routes.retain(|_, name| assisted.contains_key(name));

            tokio::select! {
                r = self.socket.recv_from(&mut buf) => {
                    let Ok((n, src)) = r else { continue };
                    let now = Instant::now();
                    let data = &buf[..n];
                    // A STUN server's answer to our own Binding request is not an ICE check.
                    if self.stun.as_ref().is_some_and(|s| s.offer(data)) {
                        continue;
                    }
                    if !peers.contains_key(&src) {
                        // A source an assisted session has already heard from, or whose first check names one.
                        let session = routes.get(&src).cloned().or_else(|| {
                            let local = stun_username(data)?.split_once(':')?.0;
                            assisted.contains_key(local).then(|| local.to_string())
                        });
                        if let Some(name) = session {
                            routes.insert(src, name.clone());
                            if let Some(p) = assisted.get_mut(&name) {
                                p.receive(now, src, self.public_addr, data);
                            }
                            continue;
                        }
                        if let Some(p) = self.admit(&peers, &mut bucket, src, data, now) {
                            peers.insert(src, p);
                        }
                    }
                    if let Some(p) = peers.get_mut(&src) {
                        p.receive(now, src, self.public_addr, data);
                    }
                }
                Some(pkt) = self.relay_rx.recv() => {
                    let now = Instant::now();
                    if let Some(p) = assisted.values_mut().find(|p| p.relay.as_ref().is_some_and(|r| r.addr() == pkt.relay)) {
                        p.receive(now, pkt.peer, pkt.relay, &pkt.data);
                    }
                }
                c = self.cmds.recv(), if commands_open => match c {
                    Some(c) => self.command(c, &mut assisted),
                    None => commands_open = false,
                },
                _ = self.wake.notified() => {
                    for p in peers.values_mut() { p.dirty = true; }
                    for p in assisted.values_mut() { p.dirty = true; }
                }
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(next)) => {}
            }
        }
    }

    fn command(&self, c: Command, assisted: &mut HashMap<String, Peer>) {
        match c {
            Command::Start(offer, reply) => {
                let _ = reply.send(self.start_assisted(offer, assisted));
            }
            Command::Candidate(name, addr) => {
                if let Some(p) = assisted.get_mut(&name) {
                    if let Ok(c) = Candidate::host(addr, "udp") {
                        p.rtc.add_remote_candidate(c);
                        p.dirty = true;
                    }
                }
            }
            Command::Cancel(name) => {
                if let Some(mut p) = assisted.remove(&name) {
                    p.rtc.disconnect();
                }
            }
            Command::Count(reply) => {
                let _ = reply.send(assisted.len());
            }
        }
    }

    /// A full ICE agent for one punched session. It sends its own checks to the client's candidates as soon as it is
    /// polled; the half-open limits do not apply because only an authenticated signal reaches here.
    fn start_assisted(
        &self,
        offer: AssistedOffer,
        assisted: &mut HashMap<String, Peer>,
    ) -> Result<AssistedAnswer> {
        if !valid_client_ufrag(&offer.ufrag) || offer.pwd.len() < 22 || offer.pwd.len() > 256 {
            return Err(anyhow!("the offer's ICE credentials are not valid"));
        }
        if assisted.len() >= self.limits.max_assisted {
            return Err(anyhow!("too many assisted sessions"));
        }
        let word = |n: usize| -> String {
            rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(n)
                .map(char::from)
                .collect()
        };
        let answer = AssistedAnswer {
            ufrag: word(24),
            pwd: word(32),
        };
        let now = Instant::now();
        let mut rtc = Rtc::builder()
            .set_fingerprint_verification(false)
            .set_dtls_cert(self.cert.clone())
            .set_local_ice_credentials(IceCreds {
                ufrag: answer.ufrag.clone(),
                pass: answer.pwd.clone(),
            })
            .build(now);
        rtc.add_local_candidate(Candidate::host(self.public_addr, "udp")?);
        for c in offer.local {
            rtc.add_local_candidate(c);
        }
        if let Some(r) = &offer.relay {
            rtc.add_local_candidate(Candidate::relayed(r.addr(), r.local(), "udp")?);
        }
        for addr in offer.candidates {
            rtc.add_remote_candidate(Candidate::host(addr, "udp")?);
        }
        {
            let mut d = rtc.direct_api();
            d.set_remote_ice_credentials(IceCreds {
                ufrag: offer.ufrag,
                pass: offer.pwd,
            });
            d.set_ice_controlling(false);
            d.set_remote_fingerprint(Fingerprint {
                hash_func: "sha-256".into(),
                bytes: vec![0; 32],
            });
            d.start_dtls(false)?;
            d.start_sctp(false);
            for (label, id) in [
                ("t", CHANNEL_ID),
                ("term", TERM_CHANNEL_ID),
                ("ctl", CONTROL_CHANNEL_ID),
            ] {
                d.create_data_channel(ChannelConfig {
                    label: label.into(),
                    negotiated: Some(id),
                    ..Default::default()
                });
            }
        }
        debug!("new assisted session {}", answer.ufrag);
        let mut peer = Peer::new(rtc, now, self.limits.assisted_ttl);
        peer.full_ice = true;
        peer.relay = offer.relay;
        assisted.insert(answer.ufrag.clone(), peer);
        Ok(answer)
    }

    /// Creates a peer for a first STUN request, or refuses.
    fn admit(
        &self,
        peers: &HashMap<SocketAddr, Peer>,
        bucket: &mut Bucket,
        src: SocketAddr,
        data: &[u8],
        now: Instant,
    ) -> Option<Peer> {
        let user = stun_username(data)?;
        let (local, remote) = user.split_once(':')?;
        if local != SERVER_UFRAG || !valid_client_ufrag(remote) {
            return None;
        }
        let half_open = peers.values().filter(|p| p.end.is_none()).count();
        let from_ip = peers.keys().filter(|a| a.ip() == src.ip()).count();
        if peers.len() >= self.limits.max_peers
            || half_open >= self.limits.max_half_open
            || from_ip >= self.limits.max_per_ip
            || !bucket.take(now)
        {
            debug!("refusing new peer from {src}");
            return None;
        }

        let mut rtc = Rtc::builder()
            .set_ice_lite(true)
            .set_fingerprint_verification(false)
            .set_dtls_cert(self.cert.clone())
            .set_local_ice_credentials(IceCreds {
                ufrag: SERVER_UFRAG.into(),
                pass: SERVER_PWD.into(),
            })
            .build(now);
        rtc.add_local_candidate(Candidate::host(self.public_addr, "udp").ok()?);
        {
            let mut d = rtc.direct_api();
            d.set_remote_ice_credentials(IceCreds {
                ufrag: remote.into(),
                pass: remote.into(),
            });
            d.set_ice_controlling(false);
            // The client certificate is unknown up front; str0m wants a value even with checks off.
            d.set_remote_fingerprint(Fingerprint {
                hash_func: "sha-256".into(),
                bytes: vec![0; 32],
            });
            d.start_dtls(false).ok()?;
            d.start_sctp(false);
            d.create_data_channel(ChannelConfig {
                label: "t".into(),
                negotiated: Some(CHANNEL_ID),
                ..Default::default()
            });
            d.create_data_channel(ChannelConfig {
                label: "term".into(),
                negotiated: Some(TERM_CHANNEL_ID),
                ..Default::default()
            });
            d.create_data_channel(ChannelConfig {
                label: "ctl".into(),
                negotiated: Some(CONTROL_CHANNEL_ID),
                ..Default::default()
            });
        }
        debug!("new peer {src}");
        Some(Peer::new(rtc, now, self.limits.half_open_ttl))
    }
}

fn inbound_len(m: &Inbound) -> usize {
    match m {
        Inbound::Json(t) => t.len(),
        Inbound::Term(b) => b.len(),
    }
}

impl Peer {
    fn new(rtc: Rtc, now: Instant, open_ttl: Duration) -> Self {
        Peer {
            rtc,
            end: None,
            cid: None,
            term_cid: None,
            ctl_cid: None,
            control_queue: VecDeque::new(),
            term_pending: None,
            created: now,
            open_ttl,
            full_ice: false,
            relay: None,
            last_rx: now,
            queue: VecDeque::new(),
            backlog: VecDeque::new(),
            control_backlog: VecDeque::new(),
            backlog_bytes: 0,
            closing: false,
            dirty: true,
            timeout: now,
        }
    }

    /// Hands a datagram to the ICE agent.
    fn receive(&mut self, now: Instant, src: SocketAddr, destination: SocketAddr, data: &[u8]) {
        self.last_rx = now;
        self.dirty = true;
        if let Ok(contents) = data.try_into() {
            let input = Input::Receive(
                now,
                Receive {
                    proto: Protocol::Udp,
                    source: src,
                    destination,
                    contents,
                },
            );
            if self.rtc.handle_input(input).is_err() {
                self.rtc.disconnect();
            }
        }
    }

    /// Moves data both ways and polls the connection. False when the peer is finished.
    fn step(
        &mut self,
        socket: &UdpSocket,
        now: Instant,
        limits: &Limits,
        state: &AppState,
        wake: &Arc<Notify>,
    ) -> bool {
        if !self.rtc.is_alive() {
            return false;
        }
        if self.end.is_none() && now.duration_since(self.created) > self.open_ttl {
            self.rtc.disconnect();
            return false;
        }
        if now.duration_since(self.last_rx) > limits.silence_ttl {
            self.rtc.disconnect();
            return false;
        }

        self.hand_over();

        // Control replies first, on their own channel; a client without that channel gets them on the first one,
        // ahead of everything queued there.
        if let Some(end) = self.end.as_mut() {
            while self.control_queue.len() < 32 {
                match end.control_from_workspace.try_recv() {
                    Ok(text) => self.control_queue.push_back(text),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        self.closing = true;
                        break;
                    }
                }
            }
            let via = self.ctl_cid.or(self.cid);
            if let Some(mut ch) = via.and_then(|c| self.rtc.channel(c)) {
                while let Some(front) = self.control_queue.front() {
                    match ch.write(false, front.as_bytes()) {
                        Ok(true) => {
                            self.control_queue.pop_front();
                        }
                        Ok(false) => break,
                        Err(_) => {
                            self.rtc.disconnect();
                            return false;
                        }
                    }
                }
            }
        }

        // Workspace -> client, with backpressure: a full SCTP buffer leaves messages queued.
        if let (Some(cid), Some(end)) = (self.cid, self.end.as_mut()) {
            while self.queue.len() < 32 {
                match end.from_workspace.try_recv() {
                    // A message the channel cannot carry would fail the write and take the connection down with it; the
                    // client is told instead, and everything else keeps flowing.
                    Ok(text) if text.len() > MAX_MESSAGE => {
                        tracing::warn!(
                            bytes = text.len(),
                            "a reply is too large for the direct transport and was not sent"
                        );
                        self.queue.push_back(OVERSIZE_REPLY.to_string());
                    }
                    Ok(text) => self.queue.push_back(text),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        self.closing = true;
                        break;
                    }
                }
            }
            if let Some(mut ch) = self.rtc.channel(cid) {
                while let Some(front) = self.queue.front() {
                    match ch.write(false, front.as_bytes()) {
                        Ok(true) => {
                            self.queue.pop_front();
                        }
                        Ok(false) => break,
                        Err(_) => {
                            self.rtc.disconnect();
                            return false;
                        }
                    }
                }
            }
        }

        // Terminal bytes go after the status messages and are joined under pressure.
        if let (Some(tcid), Some(end)) = (self.term_cid, self.end.as_mut()) {
            loop {
                if self.term_pending.is_none() {
                    let mut buf = Vec::new();
                    while buf.len() < TERM_MESSAGE {
                        match end.term_from_workspace.try_recv() {
                            Ok(chunk) => buf.extend(chunk),
                            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                                self.closing = true;
                                break;
                            }
                        }
                    }
                    if buf.is_empty() {
                        break;
                    }
                    self.term_pending = Some(buf);
                }
                let Some(mut ch) = self.rtc.channel(tcid) else {
                    break;
                };
                match ch.write(true, self.term_pending.as_deref().unwrap_or_default()) {
                    Ok(true) => self.term_pending = None,
                    Ok(false) => break,
                    Err(_) => {
                        self.rtc.disconnect();
                        return false;
                    }
                }
            }
        }

        if self.full_ice && now >= self.timeout {
            let _ = self.rtc.handle_input(Input::Timeout(now));
        }
        let mut transmits = 0;
        loop {
            match self.rtc.poll_output() {
                Ok(Output::Transmit(t)) => {
                    transmits += 1;
                    match &self.relay {
                        Some(r) if t.source == r.addr() => r.send(t.destination, &t.contents),
                        _ => {
                            let _ = socket.try_send_to(&t.contents, t.destination);
                        }
                    }
                }
                Ok(Output::Timeout(t)) => {
                    self.timeout = t;
                    break;
                }
                Ok(Output::Event(e)) => {
                    if !self.on_event(e, state, wake) {
                        self.rtc.disconnect();
                        return false;
                    }
                }
                Err(e) => {
                    warn!("peer poll failed: {e:?}");
                    self.rtc.disconnect();
                    return false;
                }
            }
        }
        let _ = transmits;
        if !self.backlog.is_empty() || !self.control_backlog.is_empty() {
            // Look again soon: the workspace is reading, and these are waiting for it.
            self.timeout = self.timeout.min(now + Duration::from_millis(5));
        }

        // The workspace finished (for example after an auth error): let the last messages out first.
        if self.closing
            && self.queue.is_empty()
            && self.control_queue.is_empty()
            && self.term_pending.is_none()
        {
            let drained = match self.cid.and_then(|c| self.rtc.channel(c)) {
                Some(mut ch) => ch.buffered_amount() == 0,
                None => true,
            };
            if drained || now.duration_since(self.last_rx) > Duration::from_secs(2) {
                self.rtc.disconnect();
                return false;
            }
            self.dirty = true;
            self.timeout = now + Duration::from_millis(20);
        }
        true
    }

    /// Gives the workspace as many waiting client messages as it has room for.
    fn hand_over(&mut self) {
        let Some(end) = &self.end else { return };
        while let Some(front) = self.control_backlog.pop_front() {
            match end.to_workspace.try_send(front) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(text)) => {
                    self.control_backlog.push_front(text);
                    break;
                }
                Err(_) => {
                    self.control_backlog.clear();
                    break;
                }
            }
        }
        if !self.control_backlog.is_empty() {
            // Nothing else goes ahead of a waiting control message.
            self.backlog_bytes = self
                .backlog
                .iter()
                .chain(&self.control_backlog)
                .map(inbound_len)
                .sum();
            return;
        }
        while let Some(front) = self.backlog.pop_front() {
            match end.to_workspace.try_send(front) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(text)) => {
                    self.backlog.push_front(text);
                    break;
                }
                Err(_) => {
                    // The workspace ended: nothing will read these.
                    self.backlog.clear();
                    self.backlog_bytes = 0;
                    return;
                }
            }
        }
        self.backlog_bytes = self
            .backlog
            .iter()
            .chain(&self.control_backlog)
            .map(inbound_len)
            .sum();
    }

    /// False means drop this peer.
    fn on_event(&mut self, e: Event, state: &AppState, wake: &Arc<Notify>) -> bool {
        match e {
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => return false,
            Event::ChannelOpen(cid, label) if label == "term" => {
                self.term_cid = Some(cid);
                if let Some(mut ch) = self.rtc.channel(cid) {
                    ch.set_buffered_amount_low_threshold(32 * 1024);
                }
            }
            Event::ChannelOpen(cid, label) if label == "ctl" => {
                self.ctl_cid = Some(cid);
                if let Some(mut ch) = self.rtc.channel(cid) {
                    ch.set_buffered_amount_low_threshold(32 * 1024);
                }
            }
            Event::ChannelOpen(cid, _) => {
                self.cid = Some(cid);
                if let Some(mut ch) = self.rtc.channel(cid) {
                    ch.set_buffered_amount_low_threshold(32 * 1024);
                }
                let (l, end) = link::pair(None, Some(wake.clone()));
                tokio::spawn(crate::connection::run(l, state.clone()));
                self.end = Some(end);
            }
            Event::ChannelData(d) => {
                if d.data.len() > MAX_MESSAGE {
                    return false;
                }
                // The status channel carries text, the term channel bytes; anything else is a broken client.
                let msg = if Some(d.id) == self.term_cid {
                    if !d.binary {
                        return false;
                    }
                    Inbound::Term(d.data.to_vec())
                } else {
                    if d.binary {
                        return false;
                    }
                    let Ok(text) = String::from_utf8(d.data.to_vec()) else {
                        return false;
                    };
                    Inbound::Json(text)
                };
                self.backlog_bytes += inbound_len(&msg);
                if Some(d.id) == self.ctl_cid {
                    self.control_backlog.push_back(msg);
                } else {
                    self.backlog.push_back(msg);
                }
                // More than this waiting means the client floods faster than the workspace reads.
                if self.backlog_bytes > BACKLOG_BYTES
                    || self.backlog.len() + self.control_backlog.len() > BACKLOG_MESSAGES
                {
                    return false;
                }
                self.hand_over();
            }
            Event::ChannelBufferedAmountLow(_) => self.dirty = true,
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, SocketAddr};
    use std::ops::RangeInclusive;
    use tokio::net::UdpSocket;

    /// Each port test uses its own slice, below the OS ephemeral range (32768-60999 on Linux), so tests
    /// that run in parallel never take a port another one is about to rebind. The production range is
    /// 4443-4542; these are only for tests.
    const TWO_SERVERS_RANGE: RangeInclusive<u16> = 20_000..=20_010;
    const RESTART_RANGE: RangeInclusive<u16> = 20_020..=20_030;
    const REMEMBERED_RANGE: RangeInclusive<u16> = 20_040..=20_050;
    const OUTSIDE_RANGE: RangeInclusive<u16> = 20_060..=20_070;
    /// Single ports for the tests that free a port and bind it again. They must not come from the OS (port 0):
    /// between the free and the second bind any other UDP socket of the process can be given that ephemeral port.
    const SAVED_OUTSIDE_PORT: u16 = 20_080;
    const EXPLICIT_PORT: u16 = 20_090;

    fn localhost() -> IpAddr {
        std::net::Ipv4Addr::LOCALHOST.into()
    }

    #[tokio::test]
    async fn two_servers_take_different_ports_from_the_range() {
        let (a, pa) = bind_transport(localhost(), None, TWO_SERVERS_RANGE, None)
            .await
            .unwrap();
        let (b, pb) = bind_transport(localhost(), None, TWO_SERVERS_RANGE, None)
            .await
            .unwrap();
        assert!(TWO_SERVERS_RANGE.contains(&pa), "{pa} is outside the range");
        assert!(TWO_SERVERS_RANGE.contains(&pb), "{pb} is outside the range");
        assert_ne!(pa, pb, "the second server reused the first's port");
        drop((a, b));
    }

    #[tokio::test]
    async fn a_restart_keeps_the_port_the_first_start_chose() {
        let (first, port) = bind_transport(localhost(), None, RESTART_RANGE, None)
            .await
            .unwrap();
        drop(first);
        let (_again, again) = bind_transport(localhost(), None, RESTART_RANGE, Some(port))
            .await
            .unwrap();
        assert_eq!(again, port);
    }

    #[tokio::test]
    async fn a_taken_saved_port_fails_clearly_instead_of_moving() {
        let (held, saved) = bind_transport(localhost(), None, REMEMBERED_RANGE, None)
            .await
            .unwrap();
        let err = bind_transport(localhost(), None, REMEMBERED_RANGE, Some(saved))
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(&saved.to_string()), "{msg}");
        assert!(msg.contains("TERMENBOX_UDP_PORT"), "{msg}");
        drop(held);
    }

    #[tokio::test]
    async fn a_saved_port_outside_the_range_is_still_the_port() {
        let port = SAVED_OUTSIDE_PORT;
        let (_socket, got) = bind_transport(localhost(), None, OUTSIDE_RANGE, Some(port))
            .await
            .unwrap();
        assert_eq!(
            got, port,
            "the owner's port is not replaced by one from the range"
        );
    }

    #[tokio::test]
    async fn an_explicit_port_is_used_as_given_and_a_taken_one_fails_clearly() {
        let port = EXPLICIT_PORT;
        let held = UdpSocket::bind(SocketAddr::new(localhost(), port))
            .await
            .unwrap();
        let err = bind_transport(localhost(), Some(port), TWO_SERVERS_RANGE, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains(&port.to_string()), "{err}");

        drop(held);
        let (_socket, got) = bind_transport(localhost(), Some(port), TWO_SERVERS_RANGE, None)
            .await
            .unwrap();
        assert_eq!(got, port);
    }

    #[tokio::test]
    async fn a_full_range_is_a_clear_startup_error() {
        // One free port, held for the whole test, so the single-port range has no room left.
        let held = UdpSocket::bind(SocketAddr::new(localhost(), 0))
            .await
            .unwrap();
        let port = held.local_addr().unwrap().port();
        let err = bind_transport(localhost(), None, port..=port, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no free UDP port"), "{err}");
    }

    fn stun(user: &str) -> Vec<u8> {
        let u = user.as_bytes();
        let pad = (4 - u.len() % 4) % 4;
        let mut attr = vec![0, 6, (u.len() >> 8) as u8, u.len() as u8];
        attr.extend_from_slice(u);
        attr.extend(std::iter::repeat(0).take(pad));
        let mut m = vec![
            0,
            1,
            (attr.len() >> 8) as u8,
            attr.len() as u8,
            0x21,
            0x12,
            0xA4,
            0x42,
        ];
        m.extend([7u8; 12]);
        m.extend(attr);
        m
    }

    #[test]
    fn reads_the_username_of_a_stun_binding_request() {
        assert_eq!(
            stun_username(&stun("termenbox:abcdefghijklmnop")),
            Some("termenbox:abcdefghijklmnop")
        );
        assert_eq!(stun_username(&stun("x")), Some("x"));
    }

    #[test]
    fn ignores_everything_that_is_not_a_binding_request() {
        assert_eq!(stun_username(b"hello"), None);
        assert_eq!(stun_username(&[0u8; 40]), None);
        let mut truncated = stun("termenbox:abcdefghijklmnop");
        truncated.truncate(30);
        assert_eq!(stun_username(&truncated), None);
    }

    #[test]
    fn client_ufrags_are_alphanumeric_and_sized() {
        assert!(valid_client_ufrag("abcdefghijklmnop"));
        assert!(!valid_client_ufrag("short"));
        assert!(!valid_client_ufrag("abcdefghijklmnop:"));
        assert!(!valid_client_ufrag(&"a".repeat(65)));
    }

    #[test]
    fn token_bucket_limits_the_creation_rate() {
        let t0 = Instant::now();
        let mut b = Bucket::new(5);
        let allowed = (0..20).filter(|_| b.take(t0)).count();
        assert_eq!(allowed, 5);
        assert!(b.take(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn certificate_and_fingerprint_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("termenbox-cert-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = load_or_create_cert(&dir).unwrap();
        let b = load_or_create_cert(&dir).unwrap();
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert!(fingerprint(&a).starts_with("sha-256 "));
    }
}
