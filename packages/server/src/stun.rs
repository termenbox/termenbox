//! STUN discovery for the assisted connection (DECISION-0140, step 2.2): the public address a server behind a NAT is
//! seen at, found with free STUN servers (FACT-0058) from the very UDP socket the WebRTC transport serves. A mapping
//! only helps a punch if the Binding request leaves that socket, so the discovery shares it: `rtc.rs` hands every
//! Binding response it receives to the `Inbox`, and the request's random transaction id (not the source address)
//! decides which answers count.
//!
//! It runs only for a server `reach.rs` found not directly reachable, never under `TERMENBOX_LAN_ONLY` (GUARDRAIL
//! lan-addresses-never-reach-the-directory), and sends nothing when `TERMENBOX_STUN` is empty.
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info};

use crate::reach::{Report, Topo};

/// Tried in this order (FACT-0058); `TERMENBOX_STUN` replaces the list, and an empty value turns STUN off.
pub const DEFAULT_SERVERS: [&str; 2] = ["stun.cloudflare.com:3478", "stun.l.google.com:19302"];
/// How long one round waits for the servers to answer.
const TIMEOUT: Duration = Duration::from_secs(2);
/// The mapping is looked at again this often; a NAT may rebind it.
const REFRESH: Duration = Duration::from_secs(5 * 60);
const COOKIE: [u8; 4] = [0x21, 0x12, 0xA4, 0x42];

/// Whether two servers saw the same mapping of the one socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapping {
    /// Every answering server saw the same address (or only one answered): a peer can be sent the address.
    EndpointIndependent,
    /// Different servers saw different ports: the NAT maps per destination and a punch from this address fails.
    Symmetric,
}

/// What one round found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped {
    pub addr: SocketAddr,
    pub mapping: Mapping,
    /// How many servers answered.
    pub answers: usize,
}

impl Mapped {
    /// The address worth putting in a bundle: only a mapping that does not depend on the destination.
    pub fn candidate(&self) -> Option<(Ipv4Addr, u16)> {
        match (self.addr.ip(), self.mapping) {
            (IpAddr::V4(ip), Mapping::EndpointIndependent) => Some((ip, self.addr.port())),
            _ => None,
        }
    }

    /// The lines of the startup block.
    pub fn render(&self) -> String {
        let kind = match self.mapping {
            Mapping::EndpointIndependent if self.answers > 1 => {
                "endpoint-independent: servers agree, a punch can work"
            }
            Mapping::EndpointIndependent => "one server answered, mapping type unknown",
            Mapping::Symmetric => "symmetric: servers disagree, not advertised; a relay is needed",
        };
        format!("STUN\n  Mapped: {} ({kind})", self.addr)
    }
}

/// The servers to ask: none in LAN-only mode, otherwise `TERMENBOX_STUN` (comma list, empty = off) or the defaults.
pub fn servers(lan_only: bool, configured: Option<&str>) -> Vec<String> {
    if lan_only {
        return Vec::new();
    }
    match configured {
        Some(list) => list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        None => DEFAULT_SERVERS.map(String::from).to_vec(),
    }
}

/// Only a server that is not directly reachable needs a mapped address: a host with a public address, or one whose
/// router mapped the port, is dialed as it is.
pub fn needed(report: &Report) -> bool {
    !report.topology.contains(&Topo::PublicHost) && report.mapping.is_none()
}

pub fn binding_request(txid: &[u8; 12]) -> [u8; 20] {
    let mut out = [0u8; 20];
    out[1] = 1; // Binding request, no attributes
    out[4..8].copy_from_slice(&COOKIE);
    out[8..].copy_from_slice(txid);
    out
}

/// Whether `buf` is a Binding success response; `rtc.rs` hands these to the discovery instead of treating them as ICE.
pub fn is_binding_response(buf: &[u8]) -> bool {
    buf.len() >= 20 && buf[0] == 0x01 && buf[1] == 0x01 && buf[4..8] == COOKIE
}

/// The transaction id of a Binding response.
fn txid_of(buf: &[u8]) -> Option<[u8; 12]> {
    is_binding_response(buf).then(|| buf[8..20].try_into().unwrap())
}

/// The mapped address of a Binding success response (XOR-MAPPED-ADDRESS, or MAPPED-ADDRESS from an old server).
/// `None` for anything malformed, of another transaction, or not IPv4.
pub fn parse_binding_response(buf: &[u8], txid: &[u8; 12]) -> Option<SocketAddr> {
    if txid_of(buf)? != *txid {
        return None;
    }
    let len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let body = buf.get(20..20 + len)?;
    let (mut i, mut plain) = (0, None);
    while i + 4 <= body.len() {
        let t = u16::from_be_bytes([body[i], body[i + 1]]);
        let l = u16::from_be_bytes([body[i + 2], body[i + 3]]) as usize;
        let v = body.get(i + 4..i + 4 + l)?;
        // family 1 is IPv4: 1 reserved byte, family, port, address
        if matches!(t, 0x0020 | 0x0001) && l == 8 && v[1] == 1 {
            let port = u16::from_be_bytes([v[2], v[3]]);
            let ip: [u8; 4] = v[4..8].try_into().unwrap();
            if t == 0x0020 {
                let port = port ^ u16::from_be_bytes([COOKIE[0], COOKIE[1]]);
                let ip = Ipv4Addr::from(std::array::from_fn::<u8, 4, _>(|n| ip[n] ^ COOKIE[n]));
                return Some(SocketAddr::new(ip.into(), port));
            }
            plain = Some(SocketAddr::new(Ipv4Addr::from(ip).into(), port));
        }
        i += 4 + l + ((4 - l % 4) % 4);
    }
    plain
}

/// What `rtc.rs` holds: a way to pass Binding responses on to the discovery.
#[derive(Clone)]
pub struct Inbox(mpsc::UnboundedSender<Vec<u8>>);

impl Inbox {
    /// Offers a received packet; true when it was a Binding response and so is not for the ICE agent.
    pub fn offer(&self, packet: &[u8]) -> bool {
        is_binding_response(packet) && {
            let _ = self.0.send(packet.to_vec());
            true
        }
    }
}

/// One round: ask every server from `socket`, wait up to `timeout` for the answers, and compare them.
async fn round(
    socket: &UdpSocket,
    rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    servers: &[String],
    timeout: Duration,
) -> Option<Mapped> {
    while rx.try_recv().is_ok() {} // answers of an earlier round
    let mut pending: HashSet<[u8; 12]> = HashSet::new();
    for s in servers {
        let Some(to) = tokio::net::lookup_host(s)
            .await
            .ok()
            .and_then(|mut a| a.find(SocketAddr::is_ipv4))
        else {
            debug!("STUN server {s} did not resolve to IPv4");
            continue;
        };
        let txid: [u8; 12] = uuid::Uuid::new_v4().as_bytes()[..12].try_into().unwrap();
        if socket.send_to(&binding_request(&txid), to).await.is_ok() {
            pending.insert(txid);
        }
    }
    let mut seen: Vec<SocketAddr> = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    while !pending.is_empty() {
        let Ok(Some(packet)) = tokio::time::timeout_at(deadline, rx.recv()).await else {
            break;
        };
        if let Some(txid) = txid_of(&packet) {
            if pending.contains(&txid) {
                if let Some(addr) = parse_binding_response(&packet, &txid) {
                    pending.remove(&txid);
                    seen.push(addr);
                }
            }
        }
    }
    let first = *seen.first()?;
    let mapping = if seen.iter().all(|a| *a == first) {
        Mapping::EndpointIndependent
    } else {
        Mapping::Symmetric
    };
    Some(Mapped { addr: first, mapping, answers: seen.len() })
}

/// Starts the discovery on the transport socket. It waits for the reachability report, asks only when
/// `needed(report)`, prints the block when the result changes and publishes it for the bundle.
pub fn spawn(
    socket: Arc<UdpSocket>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    servers: Vec<String>,
    reach: watch::Receiver<Option<Report>>,
) -> watch::Receiver<Option<Mapped>> {
    spawn_with(socket, rx, servers, reach, TIMEOUT, REFRESH)
}

fn spawn_with(
    socket: Arc<UdpSocket>,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
    servers: Vec<String>,
    mut reach: watch::Receiver<Option<Report>>,
    timeout: Duration,
    refresh: Duration,
) -> watch::Receiver<Option<Mapped>> {
    let (tx, out) = watch::channel(None);
    tokio::spawn(async move {
        loop {
            let ask = reach.borrow_and_update().as_ref().is_some_and(needed);
            let found = if ask { round(&socket, &mut rx, &servers, timeout).await } else { None };
            if *tx.borrow() != found {
                match &found {
                    Some(m) => println!("{}", m.render()),
                    None if ask => info!("STUN: no server answered; no mapped address"),
                    None => {}
                }
                let _ = tx.send(found);
            }
            tokio::select! {
                _ = tokio::time::sleep(refresh) => {}
                changed = reach.changed() => if changed.is_err() { tokio::time::sleep(refresh).await },
            }
        }
    });
    out
}

/// The channel between `rtc.rs` and the discovery.
pub fn channel() -> (Inbox, mpsc::UnboundedReceiver<Vec<u8>>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Inbox(tx), rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A STUN server on loopback answering with `claim` (or the sender's address) in XOR-MAPPED-ADDRESS.
    async fn fake(claim: Option<SocketAddr>, mode: &'static str) -> SocketAddr {
        let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let at = s.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            while let Ok((n, from)) = s.recv_from(&mut buf).await {
                if mode == "silent" || n < 20 {
                    continue;
                }
                let seen = claim.unwrap_or(from);
                let reply = match mode {
                    "garbage" => b"not stun at all, but long enough to look like a packet".to_vec(),
                    "wrong-txid" => response(&[0xEE; 12], seen),
                    _ => response(buf[8..20].try_into().unwrap(), seen),
                };
                let _ = s.send_to(&reply, from).await;
            }
        });
        at
    }

    fn response(txid: &[u8; 12], seen: SocketAddr) -> Vec<u8> {
        let IpAddr::V4(ip) = seen.ip() else { panic!() };
        let mut v = vec![0x01, 0x01, 0, 12];
        v.extend(COOKIE);
        v.extend(txid);
        v.extend([0x00, 0x20, 0, 8, 0, 1]);
        v.extend((seen.port() ^ 0x2112).to_be_bytes());
        v.extend(ip.octets().iter().zip(COOKIE).map(|(a, b)| a ^ b));
        v
    }

    /// Stands in for `rtc.rs`: receives on the shared socket and hands Binding responses to the inbox.
    async fn pump(socket: Arc<UdpSocket>, inbox: Inbox) {
        tokio::spawn(async move {
            let mut buf = [0u8; 1500];
            while let Ok((n, _)) = socket.recv_from(&mut buf).await {
                inbox.offer(&buf[..n]);
            }
        });
    }

    struct Rig {
        socket: Arc<UdpSocket>,
        rx: mpsc::UnboundedReceiver<Vec<u8>>,
    }

    async fn rig() -> Rig {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let (inbox, rx) = channel();
        pump(socket.clone(), inbox).await;
        Rig { socket, rx }
    }

    fn list(a: &[SocketAddr]) -> Vec<String> {
        a.iter().map(|a| a.to_string()).collect()
    }

    #[tokio::test]
    async fn stun_maps_the_shared_socket_and_two_agreeing_servers_are_endpoint_independent() {
        let mut r = rig().await;
        let me = r.socket.local_addr().unwrap();
        let (a, b) = (fake(None, "ok").await, fake(None, "ok").await);
        let got = round(&r.socket, &mut r.rx, &list(&[a, b]), Duration::from_secs(2)).await.unwrap();
        assert_eq!(got, Mapped { addr: me, mapping: Mapping::EndpointIndependent, answers: 2 });
        assert_eq!(got.candidate(), Some((Ipv4Addr::LOCALHOST, me.port())));
        assert!(got.render().contains("a punch can work"));
    }

    #[tokio::test]
    async fn stun_second_server_disagreeing_is_symmetric_and_not_advertised() {
        let mut r = rig().await;
        let other: SocketAddr = "127.0.0.1:55555".parse().unwrap();
        let (a, b) = (fake(None, "ok").await, fake(Some(other), "ok").await);
        let got = round(&r.socket, &mut r.rx, &list(&[a, b]), Duration::from_secs(2)).await.unwrap();
        assert_eq!(got.mapping, Mapping::Symmetric);
        assert_eq!(got.candidate(), None);
    }

    #[tokio::test]
    async fn stun_timeout_malformed_and_foreign_replies_find_nothing() {
        for mode in ["silent", "garbage", "wrong-txid"] {
            let mut r = rig().await;
            let s = fake(None, mode).await;
            let got = round(&r.socket, &mut r.rx, &list(&[s]), Duration::from_millis(300)).await;
            assert_eq!(got, None, "{mode}");
        }
        // One silent server does not hide the other's answer.
        let mut r = rig().await;
        let (dead, live) = (fake(None, "silent").await, fake(None, "ok").await);
        let got = round(&r.socket, &mut r.rx, &list(&[dead, live]), Duration::from_millis(300)).await.unwrap();
        assert_eq!((got.answers, got.mapping), (1, Mapping::EndpointIndependent));
        assert!(got.render().contains("mapping type unknown"));
    }

    #[test]
    fn stun_parser_reads_old_mapped_address_and_refuses_truncation() {
        let tx = [7u8; 12];
        let mut v = vec![0x01, 0x01, 0, 12];
        v.extend(COOKIE);
        v.extend(tx);
        v.extend([0x00, 0x01, 0, 8, 0, 1, 0x1F, 0x90, 203, 0, 113, 9]);
        assert_eq!(parse_binding_response(&v, &tx), Some("203.0.113.9:8080".parse().unwrap()));
        for cut in 0..v.len() {
            assert_eq!(parse_binding_response(&v[..cut], &tx), None, "cut {cut}");
        }
        assert_eq!(parse_binding_response(&v, &[8u8; 12]), None);
    }

    #[test]
    fn stun_is_off_in_lan_only_mode_and_with_an_empty_list() {
        assert!(servers(true, None).is_empty());
        assert!(servers(true, Some("stun.example:3478")).is_empty(), "LAN-only wins");
        assert!(servers(false, Some("")).is_empty());
        assert_eq!(servers(false, Some(" a:1, b:2 ,")), ["a:1", "b:2"]);
        assert_eq!(servers(false, None), DEFAULT_SERVERS);
    }

    #[tokio::test]
    async fn stun_runs_only_for_a_server_that_is_not_directly_reachable() {
        use crate::reach::{Facts, Outcome};
        let nat = |facts: Facts| Report::new(facts, Outcome::default(), 1);
        assert!(needed(&nat(Facts { gateway: true, ..Facts::default() })));
        assert!(!needed(&nat(Facts { host_public4: true, ..Facts::default() })));

        // No report yet, and a reachable host, send no packet; a NAT'd one does.
        let mut r = rig().await;
        let probe = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let at = probe.local_addr().unwrap();
        let (tx, reach) = watch::channel(None);
        let out = spawn_with(r.socket.clone(), std::mem::replace(&mut r.rx, channel().1), list(&[at]), reach, Duration::from_millis(200), Duration::from_secs(60));
        let mut buf = [0u8; 64];
        let quiet = tokio::time::timeout(Duration::from_millis(300), probe.recv_from(&mut buf)).await;
        assert!(quiet.is_err(), "no report: nothing sent");
        tx.send(Some(nat(Facts { host_public4: true, ..Facts::default() }))).unwrap();
        let quiet = tokio::time::timeout(Duration::from_millis(300), probe.recv_from(&mut buf)).await;
        assert!(quiet.is_err(), "reachable host: nothing sent");
        tx.send(Some(nat(Facts { gateway: true, ..Facts::default() }))).unwrap();
        let (n, _) = tokio::time::timeout(Duration::from_secs(1), probe.recv_from(&mut buf)).await.unwrap().unwrap();
        assert_eq!(n, 20, "a Binding request");
        assert!(out.borrow().is_none());
    }
}
