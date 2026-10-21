//! Reachability self-check and automatic port opening (DECISION-0123 rung 1, R64).
//!
//! At start the server works out where it sits (S1-S7 in the decision), asks the router to open its UDP port with
//! PCP, NAT-PMP or UPnP IGD (in that order), keeps the mapping alive at half its lease, removes it on exit, and
//! prints the topology and the next step as one short block. The mapping's external address goes into the signed
//! candidate bundle (`candidates.rs`) as `wan4`; a global IPv6 address as `wan6`.
//!
//! There is no probe from outside: a mapping the router accepted is reported as "mapped, not verified from
//! outside", never as reachable, because proving it would need a service that sends UDP to this host (a directory
//! change that is not part of this step). The protocols are hand-rolled on tokio (and `reqwest` for the UPnP
//! SOAP calls) to keep the binary and its memory small.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use rand::RngCore;
use tokio::net::UdpSocket;
use tokio::sync::{watch, Mutex};
use tracing::{debug, warn};

use crate::netutil::is_public_unicast;

pub const LEASE_SECS: u32 = 3600;
pub const PMP_PORT: u16 = 5351;
pub const SSDP_ADDR: &str = "239.255.255.250:1900";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Pcp,
    NatPmp,
    Upnp,
}

impl Proto {
    pub fn name(self) -> &'static str {
        match self {
            Proto::Pcp => "PCP",
            Proto::NatPmp => "NAT-PMP",
            Proto::Upnp => "UPnP",
        }
    }
}

/// Where the router is and how long to wait for it. The defaults are the real ones; tests point them at fakes.
#[derive(Debug, Clone)]
pub struct Gateway {
    pub addr: Ipv4Addr,
    /// This host's address towards the router (what the router maps the port to).
    pub local_ip: Ipv4Addr,
    pub pmp_port: u16,
    pub ssdp: SocketAddr,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
enum Detail {
    Pcp { nonce: [u8; 12] },
    NatPmp,
    Upnp { control: String, service: String },
}

#[derive(Debug, Clone)]
pub struct Mapping {
    pub proto: Proto,
    /// The router's WAN address when it said so.
    pub external_ip: Option<Ipv4Addr>,
    pub external_port: u16,
    pub internal_port: u16,
    pub lease_secs: u32,
    gw: Gateway,
    detail: Detail,
}

/// What opening the port found out. `wan` is the router's own idea of its WAN address, known even when it refused
/// the mapping if it answered the question at all.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub mapping: Option<Mapping>,
    pub refused: Vec<(Proto, String)>,
    pub wan: Option<Ipv4Addr>,
}

/// Renew at half the lease, never more often than every 30 s.
pub fn renew_after(lease_secs: u32) -> Duration {
    Duration::from_secs((lease_secs / 2).max(30) as u64)
}

// --- PCP (RFC 6887) and NAT-PMP (RFC 6886): both on the gateway's UDP 5351 --------------------------------------

fn v4_mapped(ip: Ipv4Addr) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[10] = 0xff;
    out[11] = 0xff;
    out[12..].copy_from_slice(&ip.octets());
    out
}

/// A PCP MAP request for UDP `internal_port`; `lifetime` 0 deletes the mapping.
fn pcp_request(
    client: Ipv4Addr,
    nonce: &[u8; 12],
    internal_port: u16,
    suggested: u16,
    lifetime: u32,
) -> Vec<u8> {
    let mut r = vec![0u8; 60];
    r[0] = 2; // version
    r[1] = 1; // request, opcode MAP
    r[4..8].copy_from_slice(&lifetime.to_be_bytes());
    r[8..24].copy_from_slice(&v4_mapped(client));
    r[24..36].copy_from_slice(nonce);
    r[36] = 17; // UDP
    r[40..42].copy_from_slice(&internal_port.to_be_bytes());
    r[42..44].copy_from_slice(&suggested.to_be_bytes());
    r
}

fn pcp_result_name(code: u8) -> &'static str {
    match code {
        1 => "unsupported version",
        2 => "not authorized",
        3 => "malformed request",
        4 => "unsupported opcode",
        7 => "network failure",
        8 => "out of resources",
        11 => "unsupported protocol",
        12 => "user quota exceeded",
        13 => "cannot provide the suggested port",
        14 => "address mismatch",
        _ => "refused",
    }
}

/// (external port, external address, lifetime) from a PCP MAP answer.
fn parse_pcp_map(resp: &[u8], nonce: &[u8; 12]) -> Result<(u16, Option<Ipv4Addr>, u32), String> {
    if resp.len() >= 4 && resp[0] == 0 {
        return Err("not a PCP server (it speaks NAT-PMP only)".into());
    }
    if resp.len() < 60 || resp[0] != 2 || resp[1] != 0x81 {
        return Err("unexpected answer".into());
    }
    if resp[3] != 0 {
        return Err(pcp_result_name(resp[3]).into());
    }
    if resp[24..36] != nonce[..] {
        return Err("answer to another request".into());
    }
    let lifetime = u32::from_be_bytes(resp[4..8].try_into().unwrap());
    let port = u16::from_be_bytes(resp[42..44].try_into().unwrap());
    let ext: [u8; 16] = resp[44..60].try_into().unwrap();
    let ip = (ext[..10] == [0; 10] && ext[10..12] == [0xff, 0xff])
        .then(|| Ipv4Addr::new(ext[12], ext[13], ext[14], ext[15]));
    Ok((port, ip, lifetime))
}

fn pmp_map_request(internal_port: u16, suggested: u16, lifetime: u32) -> [u8; 12] {
    let mut r = [0u8; 12];
    r[1] = 1; // map UDP
    r[4..6].copy_from_slice(&internal_port.to_be_bytes());
    r[6..8].copy_from_slice(&suggested.to_be_bytes());
    r[8..12].copy_from_slice(&lifetime.to_be_bytes());
    r
}

fn pmp_result_name(code: u16) -> &'static str {
    match code {
        1 => "unsupported version",
        2 => "not authorized or refused",
        3 => "network failure",
        4 => "out of resources",
        5 => "unsupported opcode",
        _ => "refused",
    }
}

/// Sends `req` and waits for an answer from `dest` that `accept` takes; one retry. `None` when the router is silent.
async fn exchange(
    sock: &UdpSocket,
    dest: SocketAddr,
    req: &[u8],
    timeout: Duration,
    accept: impl Fn(&[u8]) -> bool,
) -> Option<Vec<u8>> {
    let mut buf = [0u8; 1100];
    for _ in 0..2 {
        sock.send_to(req, dest).await.ok()?;
        let deadline = tokio::time::Instant::now() + timeout;
        while let Ok(Ok((n, from))) =
            tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await
        {
            if from.ip() == dest.ip() && accept(&buf[..n]) {
                return Some(buf[..n].to_vec());
            }
        }
    }
    None
}

async fn bind_local(gw: &Gateway) -> Result<UdpSocket, String> {
    UdpSocket::bind((gw.local_ip, 0))
        .await
        .map_err(|e| format!("cannot open a socket: {e}"))
}

async fn pcp_map(
    gw: &Gateway,
    internal: u16,
    suggested: u16,
    lifetime: u32,
    nonce: [u8; 12],
) -> Result<Mapping, String> {
    let sock = bind_local(gw).await?;
    let dest = SocketAddr::new(gw.addr.into(), gw.pmp_port);
    let req = pcp_request(gw.local_ip, &nonce, internal, suggested, lifetime);
    let resp = exchange(&sock, dest, &req, gw.timeout, |r| !r.is_empty())
        .await
        .ok_or("no answer")?;
    let (port, ip, lease) = parse_pcp_map(&resp, &nonce)?;
    Ok(Mapping {
        proto: Proto::Pcp,
        external_ip: ip,
        external_port: port,
        internal_port: internal,
        lease_secs: lease,
        gw: gw.clone(),
        detail: Detail::Pcp { nonce },
    })
}

/// The router's WAN address by NAT-PMP, then the mapping.
async fn pmp_open(
    gw: &Gateway,
    internal: u16,
    suggested: u16,
    lifetime: u32,
) -> (Result<Mapping, String>, Option<Ipv4Addr>) {
    let sock = match bind_local(gw).await {
        Ok(s) => s,
        Err(e) => return (Err(e), None),
    };
    let dest = SocketAddr::new(gw.addr.into(), gw.pmp_port);
    let is_ans = |op: u8| move |r: &[u8]| r.len() >= 8 && r[0] == 0 && r[1] == op;
    let Some(ext) = exchange(&sock, dest, &[0, 0], gw.timeout, is_ans(128)).await else {
        return (Err("no answer".into()), None);
    };
    let code = u16::from_be_bytes([ext[2], ext[3]]);
    if code != 0 || ext.len() < 12 {
        return (Err(pmp_result_name(code).into()), None);
    }
    let wan = Ipv4Addr::new(ext[8], ext[9], ext[10], ext[11]);
    let req = pmp_map_request(internal, suggested, lifetime);
    let Some(resp) = exchange(&sock, dest, &req, gw.timeout, |r| {
        r.len() >= 16 && r[0] == 0 && r[1] == 129
    })
    .await
    else {
        return (Err("no answer to the mapping request".into()), Some(wan));
    };
    let code = u16::from_be_bytes([resp[2], resp[3]]);
    if code != 0 {
        return (Err(pmp_result_name(code).into()), Some(wan));
    }
    let port = u16::from_be_bytes([resp[10], resp[11]]);
    let lease = u32::from_be_bytes(resp[12..16].try_into().unwrap());
    (
        Ok(Mapping {
            proto: Proto::NatPmp,
            external_ip: Some(wan),
            external_port: port,
            internal_port: internal,
            lease_secs: lease,
            gw: gw.clone(),
            detail: Detail::NatPmp,
        }),
        Some(wan),
    )
}

// --- UPnP IGD: SSDP discovery, then SOAP over HTTP ---------------------------------------------------------------

const IGD_ST: &str = "urn:schemas-upnp-org:device:InternetGatewayDevice:1";

/// The text between `<name>` and `</name>`.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].trim())
}

/// The WAN connection service of an IGD description: its type and absolute control URL.
fn igd_service(desc: &str, location: &str) -> Option<(String, String)> {
    let mut best: Option<(String, String)> = None;
    for block in desc.split("<service>").skip(1) {
        let block = block.split("</service>").next().unwrap_or("");
        let (Some(kind), Some(control)) = (tag(block, "serviceType"), tag(block, "controlURL"))
        else {
            continue;
        };
        if !(kind.contains(":WANIPConnection:") || kind.contains(":WANPPPConnection:")) {
            continue;
        }
        let base = tag(desc, "URLBase")
            .filter(|b| !b.is_empty())
            .unwrap_or(location);
        let url = reqwest::Url::parse(base).ok()?.join(control).ok()?;
        // Prefer an IP connection over PPP when a device lists both.
        if best.is_none() || kind.contains(":WANIPConnection:") {
            best = Some((kind.to_string(), url.to_string()));
        }
    }
    best
}

fn soap_envelope(service: &str, action: &str, args: &[(&str, String)]) -> String {
    let body: String = args
        .iter()
        .map(|(k, v)| format!("<{k}>{v}</{k}>"))
        .collect();
    format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
         <u:{action} xmlns:u=\"{service}\">{body}</u:{action}></s:Body></s:Envelope>"
    )
}

/// One SOAP call. `Err((code, text))` carries the UPnP error code when the router sent one.
async fn soap(
    http: &reqwest::Client,
    control: &str,
    service: &str,
    action: &str,
    args: &[(&str, String)],
) -> Result<String, (Option<u32>, String)> {
    let resp = http
        .post(control)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPAction", format!("\"{service}#{action}\""))
        .body(soap_envelope(service, action, args))
        .send()
        .await
        .map_err(|e| (None, format!("{action}: {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| (None, format!("{action}: {e}")))?;
    if status.is_success() {
        return Ok(text);
    }
    let code = tag(&text, "errorCode").and_then(|c| c.parse().ok());
    let why = tag(&text, "errorDescription").unwrap_or("refused");
    Err((
        code,
        format!(
            "{action}: {why} ({})",
            code.map_or(status.as_u16().into(), |c| c)
        ),
    ))
}

/// The control point of the router's IGD, found with an SSDP search (to `gw.ssdp`, the multicast group).
async fn upnp_discover(gw: &Gateway, http: &reqwest::Client) -> Result<(String, String), String> {
    let sock = bind_local(gw).await?;
    let req = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {IGD_ST}\r\n\r\n"
    );
    let mut buf = [0u8; 2048];
    let mut location = None;
    'tries: for _ in 0..2 {
        sock.send_to(req.as_bytes(), gw.ssdp)
            .await
            .map_err(|e| format!("cannot search: {e}"))?;
        let deadline = tokio::time::Instant::now() + gw.timeout;
        while let Ok(Ok((n, _))) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await
        {
            let text = String::from_utf8_lossy(&buf[..n]);
            if let Some(l) = text.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("location")
                    .then(|| v.trim().to_string())
            }) {
                location = Some(l);
                break 'tries;
            }
        }
    }
    let location = location.ok_or("no answer")?;
    let desc = http
        .get(&location)
        .send()
        .await
        .map_err(|e| format!("device description: {e}"))?
        .text()
        .await
        .map_err(|e| format!("device description: {e}"))?;
    igd_service(&desc, &location).ok_or_else(|| "no WAN connection service".into())
}

async fn upnp_add(
    http: &reqwest::Client,
    gw: &Gateway,
    control: &str,
    service: &str,
    internal: u16,
    external: u16,
    lease: u32,
) -> Result<(), (Option<u32>, String)> {
    soap(
        http,
        control,
        service,
        "AddPortMapping",
        &[
            ("NewRemoteHost", String::new()),
            ("NewExternalPort", external.to_string()),
            ("NewProtocol", "UDP".into()),
            ("NewInternalPort", internal.to_string()),
            ("NewInternalClient", gw.local_ip.to_string()),
            ("NewEnabled", "1".into()),
            ("NewPortMappingDescription", "termenbox".into()),
            ("NewLeaseDuration", lease.to_string()),
        ],
    )
    .await
    .map(|_| ())
}

async fn upnp_open(
    gw: &Gateway,
    http: &reqwest::Client,
    internal: u16,
    lease: u32,
) -> (Result<Mapping, String>, Option<Ipv4Addr>) {
    let (service, control) = match upnp_discover(gw, http).await {
        Ok(v) => v,
        Err(e) => return (Err(e), None),
    };
    let wan = soap(http, &control, &service, "GetExternalIPAddress", &[])
        .await
        .ok()
        .and_then(|b| tag(&b, "NewExternalIPAddress").and_then(|s| s.parse::<Ipv4Addr>().ok()));
    // Same external port first; another one when the router says it is taken (718); a permanent mapping when it
    // does not do leases (725).
    let mut external = internal;
    let mut lease = lease;
    let mut last = String::new();
    for _ in 0..3 {
        match upnp_add(http, gw, &control, &service, internal, external, lease).await {
            Ok(()) => {
                return (
                    Ok(Mapping {
                        proto: Proto::Upnp,
                        external_ip: wan,
                        external_port: external,
                        internal_port: internal,
                        lease_secs: if lease == 0 { LEASE_SECS } else { lease },
                        gw: gw.clone(),
                        detail: Detail::Upnp { control, service },
                    }),
                    wan,
                )
            }
            Err((Some(718), msg)) => {
                last = msg;
                external = 49152 + (rand::thread_rng().next_u32() % 16000) as u16;
            }
            Err((Some(725), msg)) => {
                last = msg;
                lease = 0;
            }
            Err((_, msg)) => return (Err(msg), wan),
        }
    }
    (Err(last), wan)
}

// --- opening, renewing, removing ---------------------------------------------------------------------------------

/// Tries PCP, then NAT-PMP, then UPnP; the first router that accepts wins.
pub async fn open(gw: &Gateway, http: &reqwest::Client, internal: u16, lease: u32) -> Outcome {
    let mut out = Outcome::default();
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    match pcp_map(gw, internal, internal, lease, nonce).await {
        Ok(m) => {
            out.wan = m.external_ip;
            out.mapping = Some(m);
            return out;
        }
        Err(e) => out.refused.push((Proto::Pcp, e)),
    }
    let (result, wan) = pmp_open(gw, internal, internal, lease).await;
    out.wan = out.wan.or(wan);
    match result {
        Ok(m) => {
            out.mapping = Some(m);
            return out;
        }
        Err(e) => out.refused.push((Proto::NatPmp, e)),
    }
    let (result, wan) = upnp_open(gw, http, internal, lease).await;
    out.wan = out.wan.or(wan);
    match result {
        Ok(m) => out.mapping = Some(m),
        Err(e) => out.refused.push((Proto::Upnp, e)),
    }
    out
}

/// Asks the router to keep the mapping for another lease; the new mapping (its port may have changed).
pub async fn renew(m: &Mapping, http: &reqwest::Client) -> Result<Mapping, String> {
    match &m.detail {
        Detail::Pcp { nonce } => {
            pcp_map(&m.gw, m.internal_port, m.external_port, LEASE_SECS, *nonce).await
        }
        Detail::NatPmp => {
            pmp_open(&m.gw, m.internal_port, m.external_port, LEASE_SECS)
                .await
                .0
        }
        Detail::Upnp { control, service } => {
            upnp_add(
                http,
                &m.gw,
                control,
                service,
                m.internal_port,
                m.external_port,
                LEASE_SECS,
            )
            .await
            .map_err(|e| e.1)?;
            Ok(Mapping {
                lease_secs: LEASE_SECS,
                ..m.clone()
            })
        }
    }
}

/// Takes the mapping away again (on exit). Best effort: the lease ends it anyway.
pub async fn remove(m: &Mapping, http: &reqwest::Client) {
    let gw = &m.gw;
    match &m.detail {
        Detail::Pcp { nonce } => {
            let _ = pcp_map(gw, m.internal_port, m.external_port, 0, *nonce).await;
        }
        Detail::NatPmp => {
            let _ = pmp_open(gw, m.internal_port, 0, 0).await;
        }
        Detail::Upnp { control, service } => {
            let _ = soap(
                http,
                control,
                service,
                "DeletePortMapping",
                &[
                    ("NewRemoteHost", String::new()),
                    ("NewExternalPort", m.external_port.to_string()),
                    ("NewProtocol", "UDP".into()),
                ],
            )
            .await;
        }
    }
}

// --- where this host sits ------------------------------------------------------------------------------------------

/// The default route's gateway, from the text of `/proc/net/route`.
pub fn parse_default_gateway(route: &str) -> Option<Ipv4Addr> {
    route.lines().skip(1).find_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        let flags = u32::from_str_radix(f.get(3)?, 16).ok()?;
        if *f.get(1)? != "00000000" || flags & 0x2 == 0 {
            return None;
        }
        let g = u32::from_str_radix(f.get(2)?, 16).ok()?.to_le_bytes();
        Some(Ipv4Addr::from(g)).filter(|ip| !ip.is_unspecified())
    })
}

/// The address this host uses towards `to`. No packet is sent.
fn local_ip_towards(to: Ipv4Addr) -> Option<Ipv4Addr> {
    let s = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect((to, 9)).ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        _ => None,
    }
}

/// The real router, when there is one to ask.
pub fn detect_gateway() -> Option<Gateway> {
    let route = std::fs::read_to_string("/proc/net/route").ok()?;
    let addr = parse_default_gateway(&route)?;
    Some(Gateway {
        addr,
        local_ip: local_ip_towards(addr)?,
        pmp_port: PMP_PORT,
        ssdp: SSDP_ADDR.parse().ok()?,
        timeout: Duration::from_millis(1200),
    })
}

/// The server topologies of DECISION-0123 that can be told apart without a probe from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topo {
    /// S1 or S2: this host has a public IPv4 address; whether UDP gets through is not verified.
    PublicHost,
    /// S3: behind a router that has a public WAN address.
    Forwardable,
    /// S3 or S4: behind a NAT whose router did not say what its WAN address is.
    BehindNat,
    /// S4: the router's WAN address is not public.
    Cgnat,
    /// S5.
    Ipv6,
    /// S6.
    Overlay,
    /// S7.
    NoInternet,
}

impl Topo {
    pub fn label(self) -> &'static str {
        match self {
            Topo::PublicHost => "S1/S2 public IPv4 on this host",
            Topo::Forwardable => "S3 behind a router with a public address",
            Topo::BehindNat => "S3/S4 behind a NAT; the router did not report its WAN address",
            Topo::Cgnat => "S4 carrier-grade NAT",
            Topo::Ipv6 => "S5 global IPv6",
            Topo::Overlay => "S6 on an overlay network",
            Topo::NoInternet => "S7 no internet seen",
        }
    }
}

/// Everything the classification needs.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// This host's route address to the internet is public.
    pub host_public4: bool,
    pub gateway: bool,
    /// The router's own WAN address (from PCP, NAT-PMP or UPnP).
    pub wan: Option<Ipv4Addr>,
    /// The address an outside service sees this host as.
    pub seen: Option<Ipv4Addr>,
    pub mapped: bool,
    pub v6: Option<Ipv6Addr>,
    pub overlay: bool,
}

/// Where the host sits, from the facts alone. A router WAN address that is private or shared (100.64/10), or that
/// differs from what the outside sees, means carrier-grade or double NAT: no port forward can help there.
pub fn classify(f: &Facts) -> Vec<Topo> {
    let mut out = Vec::new();
    if f.host_public4 {
        out.push(Topo::PublicHost);
    } else {
        match (f.wan, f.seen) {
            (Some(wan), seen) => {
                let public = is_public_unicast(IpAddr::V4(wan));
                let differs = seen.is_some_and(|s| s != wan);
                out.push(if public && !differs {
                    Topo::Forwardable
                } else {
                    Topo::Cgnat
                });
            }
            (None, Some(_)) => out.push(Topo::BehindNat),
            (None, None) if f.v6.is_none() && !f.overlay => out.push(Topo::NoInternet),
            (None, None) => {}
        }
    }
    if f.v6.is_some() {
        out.push(Topo::Ipv6);
    }
    if f.overlay {
        out.push(Topo::Overlay);
    }
    out
}

/// The one thing the owner can do next.
fn next_step(topo: &[Topo], f: &Facts, port: u16, refused: &[(Proto, String)]) -> String {
    let has = |t| topo.contains(&t);
    if has(Topo::PublicHost) {
        return format!(
            "open UDP {port} in this host's firewall (for example: ufw allow {port}/udp)"
        );
    }
    if has(Topo::Cgnat) {
        return "a port forward cannot work behind carrier-grade NAT; use an overlay (Tailscale or WireGuard) or the LAN".into();
    }
    if f.mapped {
        return "none; if a client away from home still cannot connect, check the router's firewall".into();
    }
    if has(Topo::Forwardable) || has(Topo::BehindNat) {
        let why = if refused.is_empty() {
            String::new()
        } else {
            " (the router refused or ignored PCP, NAT-PMP and UPnP)".into()
        };
        return format!("forward UDP {port} to this host in the router{why}, or use an overlay");
    }
    if has(Topo::NoInternet) {
        return "reach this server over the LAN with the pairing link (termenbox-server --print-pairing)".into();
    }
    "none needed".into()
}

/// What the check found, for the startup block and for the candidate bundle.
#[derive(Debug, Clone)]
pub struct Report {
    pub topology: Vec<Topo>,
    pub mapping: Option<Mapping>,
    pub refused: Vec<(Proto, String)>,
    pub facts: Facts,
    pub port: u16,
}

impl Report {
    pub fn new(facts: Facts, outcome: Outcome, port: u16) -> Self {
        Self {
            topology: classify(&facts),
            mapping: outcome.mapping,
            refused: outcome.refused,
            facts,
            port,
        }
    }

    /// The short block printed at start: topology, port state, next step.
    pub fn render(&self) -> String {
        let topo = if self.topology.is_empty() {
            "unknown".to_string()
        } else {
            self.topology
                .iter()
                .map(|t| t.label())
                .collect::<Vec<_>>()
                .join("; ")
        };
        let port = match &self.mapping {
            Some(m) => format!(
                "UDP {} mapped by {} to {}:{} for {} min (mapped, not verified from outside)",
                self.port,
                m.proto.name(),
                m.external_ip.map_or("?".to_string(), |ip| ip.to_string()),
                m.external_port,
                m.lease_secs / 60
            ),
            None if self.topology.contains(&Topo::PublicHost) => {
                format!("UDP {} is not verified from outside", self.port)
            }
            None if self.refused.is_empty() => {
                format!("UDP {} not opened (no router found)", self.port)
            }
            None => format!(
                "UDP {} not opened ({})",
                self.port,
                self.refused
                    .iter()
                    .map(|(p, e)| format!("{}: {e}", p.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        format!(
            "Reachability\n  Topology: {topo}\n  Port: {port}\n  Next: {}",
            next_step(&self.topology, &self.facts, self.port, &self.refused)
        )
    }
}

/// What a start-up check needs from outside this module.
pub struct Config {
    pub port: u16,
    pub gateway: Option<Gateway>,
    pub host_public4: bool,
    pub v6: Option<Ipv6Addr>,
    pub overlay: bool,
    /// The external address an echo service sees; `None` when it could not be asked.
    pub seen: Option<Ipv4Addr>,
}

/// Runs the check once.
pub async fn check(cfg: &Config, http: &reqwest::Client) -> Report {
    let mut outcome = Outcome::default();
    let mut facts = Facts {
        host_public4: cfg.host_public4,
        gateway: cfg.gateway.is_some(),
        seen: cfg.seen,
        v6: cfg.v6,
        overlay: cfg.overlay,
        ..Facts::default()
    };
    // A host with a public address is not behind a NAT: there is no router to ask.
    if !cfg.host_public4 {
        if let Some(gw) = &cfg.gateway {
            outcome = open(gw, http, cfg.port, LEASE_SECS).await;
        }
    }
    facts.wan = outcome.wan;
    facts.mapped = outcome.mapping.is_some();
    Report::new(facts, outcome, cfg.port)
}

/// The running check: publishes its report, keeps the mapping alive and removes it on `release`.
pub struct Handle {
    pub rx: watch::Receiver<Option<Report>>,
    mapping: Arc<Mutex<Option<Mapping>>>,
    http: reqwest::Client,
}

impl Handle {
    /// A handle that never reports anything (LAN-only, or WebRTC off).
    pub fn idle() -> (Self, watch::Sender<Option<Report>>) {
        let (tx, rx) = watch::channel(None);
        (
            Self {
                rx,
                mapping: Arc::default(),
                http: reqwest::Client::new(),
            },
            tx,
        )
    }

    /// Removes the router mapping, if any. Call on the way out.
    pub async fn release(&self) {
        if let Some(m) = self.mapping.lock().await.take() {
            remove(&m, &self.http).await;
            debug!("removed the {} port mapping", m.proto.name());
        }
    }
}

/// Starts the check in the background and prints its block when it is done.
pub fn spawn(mut cfg: Config, http: reqwest::Client, lookup_seen: bool) -> Handle {
    let (tx, rx) = watch::channel(None);
    let mapping: Arc<Mutex<Option<Mapping>>> = Arc::default();
    let handle = Handle {
        rx,
        mapping: mapping.clone(),
        http: http.clone(),
    };
    tokio::spawn(async move {
        if lookup_seen {
            cfg.seen = match crate::netutil::external_ip_seen(&http).await {
                Some(IpAddr::V4(ip)) => Some(ip),
                _ => None,
            };
        }
        let report = check(&cfg, &http).await;
        println!("{}", report.render());
        *mapping.lock().await = report.mapping.clone();
        let _ = tx.send(Some(report.clone()));
        let mut current = report.mapping;
        // Renew at half the lease. A router that stops answering ends the mapping: say so once and try from scratch.
        while let Some(m) = current.clone() {
            tokio::time::sleep(renew_after(m.lease_secs)).await;
            match renew(&m, &http).await {
                Ok(next) => {
                    current = Some(next.clone());
                    *mapping.lock().await = Some(next);
                }
                Err(e) => {
                    warn!(
                        "the router no longer keeps the {} mapping ({e}); asking again",
                        m.proto.name()
                    );
                    let again = check(&cfg, &http).await;
                    current = again.mapping.clone();
                    *mapping.lock().await = again.mapping.clone();
                    let _ = tx.send(Some(again));
                }
            }
        }
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    // --- fake gateways -----------------------------------------------------------------------------------------

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Mode {
        Accept,
        /// The router answers with "not authorized".
        Refuse,
        /// The router does not speak this protocol at all.
        Silent,
    }

    /// A router's UDP 5351: PCP (version 2) and NAT-PMP (version 0), each with its own behaviour.
    struct FakePmpRouter {
        addr: SocketAddr,
        /// Every request seen, as (protocol, requested lifetime).
        seen: Arc<StdMutex<Vec<(Proto, u32)>>>,
    }

    impl FakePmpRouter {
        async fn start(pcp: Mode, pmp: Mode, wan: Ipv4Addr, granted_port: u16) -> Self {
            let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let addr = sock.local_addr().unwrap();
            let seen: Arc<StdMutex<Vec<(Proto, u32)>>> = Arc::default();
            let log = seen.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 1100];
                loop {
                    let Ok((n, from)) = sock.recv_from(&mut buf).await else {
                        return;
                    };
                    let req = &buf[..n];
                    match req.first() {
                        // PCP MAP
                        Some(2) if n >= 60 => {
                            let life = u32::from_be_bytes(req[4..8].try_into().unwrap());
                            log.lock().unwrap().push((Proto::Pcp, life));
                            let mut r = vec![0u8; 60];
                            r.copy_from_slice(req);
                            r[0] = 2;
                            r[1] = 0x81;
                            r[3] = if pcp == Mode::Refuse { 2 } else { 0 };
                            r[4..8].copy_from_slice(&life.to_be_bytes());
                            r[42..44].copy_from_slice(&granted_port.to_be_bytes());
                            r[44..60].copy_from_slice(&v4_mapped(wan));
                            if pcp != Mode::Silent {
                                let _ = sock.send_to(&r, from).await;
                            } else if pmp != Mode::Silent {
                                // A NAT-PMP-only router tells a PCP client it speaks an older version.
                                let _ = sock.send_to(&[0, 0x81, 0, 1, 0, 0, 0, 0], from).await;
                            }
                        }
                        // NAT-PMP external address
                        Some(0) if n == 2 && req[1] == 0 && pmp != Mode::Silent => {
                            log.lock().unwrap().push((Proto::NatPmp, 0));
                            let code: u16 = if pmp == Mode::Refuse { 2 } else { 0 };
                            let mut r = vec![0, 128];
                            r.extend(code.to_be_bytes());
                            r.extend([0, 0, 0, 1]);
                            r.extend(wan.octets());
                            let _ = sock.send_to(&r, from).await;
                        }
                        // NAT-PMP map UDP
                        Some(0) if n == 12 && req[1] == 1 && pmp != Mode::Silent => {
                            let life = u32::from_be_bytes(req[8..12].try_into().unwrap());
                            log.lock().unwrap().push((Proto::NatPmp, life));
                            let code: u16 = if pmp == Mode::Refuse { 2 } else { 0 };
                            let mut r = vec![0, 129];
                            r.extend(code.to_be_bytes());
                            r.extend([0, 0, 0, 1]);
                            r.extend(&req[4..6]);
                            r.extend(granted_port.to_be_bytes());
                            r.extend(life.to_be_bytes());
                            let _ = sock.send_to(&r, from).await;
                        }
                        _ => {}
                    }
                }
            });
            Self { addr, seen }
        }

        fn lifetimes(&self, p: Proto) -> Vec<u32> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|(q, _)| *q == p)
                .map(|(_, l)| *l)
                .collect()
        }
    }

    /// A router's UPnP IGD: SSDP answer, device description and SOAP control, on 127.0.0.1.
    struct FakeIgd {
        ssdp: SocketAddr,
        /// Every SOAP action with its arguments' text.
        actions: Arc<StdMutex<Vec<(String, String)>>>,
    }

    impl FakeIgd {
        /// `add_error`: the UPnP error code AddPortMapping answers with, or none to accept.
        async fn start(wan: Ipv4Addr, add_error: Option<u32>) -> Self {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let http_addr = tcp.local_addr().unwrap();
            let actions: Arc<StdMutex<Vec<(String, String)>>> = Arc::default();
            let log = actions.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut s, _)) = tcp.accept().await else {
                        return;
                    };
                    let log = log.clone();
                    tokio::spawn(async move {
                        let mut req = Vec::new();
                        let mut buf = [0u8; 4096];
                        // Read headers, then the body by Content-Length.
                        loop {
                            let Ok(n) = s.read(&mut buf).await else {
                                return;
                            };
                            if n == 0 {
                                return;
                            }
                            req.extend(&buf[..n]);
                            let text = String::from_utf8_lossy(&req).into_owned();
                            if let Some(h) = text.find("\r\n\r\n") {
                                let len = text[..h]
                                    .lines()
                                    .find_map(|l| {
                                        l.to_ascii_lowercase()
                                            .strip_prefix("content-length:")
                                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                    })
                                    .unwrap_or(0);
                                if req.len() >= h + 4 + len {
                                    break;
                                }
                            }
                        }
                        let text = String::from_utf8_lossy(&req).into_owned();
                        let (status, body) = if text.starts_with("GET /desc.xml") {
                            ("200 OK", "<root><device><deviceList><device><deviceList><device><serviceList>\
                                 <service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
                                 <controlURL>/ctl/ip</controlURL></service></serviceList></device></deviceList></device></deviceList></device></root>"
                                .to_string())
                        } else {
                            let action = text
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .starts_with("soapaction:")
                                        .then(|| l.to_string())
                                })
                                .and_then(|l| {
                                    l.split('#')
                                        .nth(1)
                                        .map(|a| a.trim_end_matches('"').to_string())
                                })
                                .unwrap_or_default();
                            log.lock().unwrap().push((
                                action.clone(),
                                text.split("\r\n\r\n").nth(1).unwrap_or("").to_string(),
                            ));
                            match action.as_str() {
                                "GetExternalIPAddress" => ("200 OK", format!("<NewExternalIPAddress>{wan}</NewExternalIPAddress>")),
                                "AddPortMapping" => match add_error {
                                    None => ("200 OK", "<ok/>".into()),
                                    Some(code) => ("500 Internal Server Error", format!("<errorCode>{code}</errorCode><errorDescription>nope</errorDescription>")),
                                },
                                _ => ("200 OK", "<ok/>".into()),
                            }
                        };
                        let resp = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        let _ = s.write_all(resp.as_bytes()).await;
                    });
                }
            });
            let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let ssdp = udp.local_addr().unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 1500];
                loop {
                    let Ok((_, from)) = udp.recv_from(&mut buf).await else {
                        return;
                    };
                    let r = format!("HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nLocation: http://{http_addr}/desc.xml\r\nST: {IGD_ST}\r\n\r\n");
                    let _ = udp.send_to(r.as_bytes(), from).await;
                }
            });
            Self { ssdp, actions }
        }

        fn actions(&self, name: &str) -> Vec<String> {
            self.actions
                .lock()
                .unwrap()
                .iter()
                .filter(|(a, _)| a == name)
                .map(|(_, b)| b.clone())
                .collect()
        }
    }

    fn gateway(pmp: Option<SocketAddr>, ssdp: Option<SocketAddr>) -> Gateway {
        Gateway {
            addr: Ipv4Addr::LOCALHOST,
            local_ip: Ipv4Addr::LOCALHOST,
            pmp_port: pmp.map_or(1, |a| a.port()),
            // A silent address when there is no IGD: nothing listens, nothing answers.
            ssdp: ssdp.unwrap_or_else(|| "127.0.0.1:1".parse().unwrap()),
            timeout: Duration::from_millis(150),
        }
    }

    fn wan() -> Ipv4Addr {
        "93.184.216.34".parse().unwrap()
    }

    // --- one test per protocol, and the refusals ---------------------------------------------------------------

    #[tokio::test]
    async fn pcp_opens_the_port_renews_it_and_removes_it() {
        let router = FakePmpRouter::start(Mode::Accept, Mode::Silent, wan(), 50123).await;
        let gw = gateway(Some(router.addr), None);
        let http = reqwest::Client::new();
        let out = open(&gw, &http, 4443, 600).await;
        let m = out.mapping.expect("mapped");
        assert_eq!(
            (m.proto, m.external_ip, m.external_port),
            (Proto::Pcp, Some(wan()), 50123)
        );
        assert_eq!(m.lease_secs, 600);
        assert_eq!(out.wan, Some(wan()));
        renew(&m, &http).await.unwrap();
        remove(&m, &http).await;
        assert_eq!(
            router.lifetimes(Proto::Pcp),
            [600, LEASE_SECS, 0],
            "open, renew, remove"
        );
    }

    #[tokio::test]
    async fn nat_pmp_opens_the_port_when_pcp_is_not_spoken() {
        let router = FakePmpRouter::start(Mode::Silent, Mode::Accept, wan(), 4443).await;
        let gw = gateway(Some(router.addr), None);
        let http = reqwest::Client::new();
        let out = open(&gw, &http, 4443, 600).await;
        let m = out.mapping.expect("mapped");
        assert_eq!(
            (m.proto, m.external_ip, m.external_port),
            (Proto::NatPmp, Some(wan()), 4443)
        );
        // The PCP attempt was refused as an unsupported version and is on record.
        assert_eq!(out.refused.len(), 1);
        assert_eq!(out.refused[0].0, Proto::Pcp);
        renew(&m, &http).await.unwrap();
        remove(&m, &http).await;
        assert_eq!(
            router
                .lifetimes(Proto::NatPmp)
                .iter()
                .filter(|l| **l > 0)
                .count(),
            2,
            "open and renew"
        );
        assert_eq!(
            router.lifetimes(Proto::NatPmp).last(),
            Some(&0),
            "removed at the end"
        );
    }

    #[tokio::test]
    async fn upnp_opens_the_port_renews_it_and_removes_it() {
        let igd = FakeIgd::start(wan(), None).await;
        let gw = gateway(None, Some(igd.ssdp));
        let http = reqwest::Client::new();
        let out = open(&gw, &http, 4443, 600).await;
        let m = out.mapping.expect("mapped by UPnP");
        assert_eq!(
            (m.proto, m.external_ip, m.external_port),
            (Proto::Upnp, Some(wan()), 4443)
        );
        // PCP and NAT-PMP got no answer first and are on record.
        assert_eq!(
            out.refused.iter().map(|r| r.0).collect::<Vec<_>>(),
            [Proto::Pcp, Proto::NatPmp]
        );
        renew(&m, &http).await.unwrap();
        remove(&m, &http).await;
        let adds = igd.actions("AddPortMapping");
        assert_eq!(adds.len(), 2, "open and renew");
        assert!(adds[0].contains("<NewExternalPort>4443</NewExternalPort>"));
        assert!(adds[0].contains("<NewProtocol>UDP</NewProtocol>"));
        assert!(adds[0].contains("<NewInternalClient>127.0.0.1</NewInternalClient>"));
        assert_eq!(igd.actions("DeletePortMapping").len(), 1);
    }

    #[tokio::test]
    async fn upnp_picks_another_port_when_the_router_says_it_is_taken() {
        let igd = FakeIgd::start(wan(), Some(718)).await;
        let gw = gateway(None, Some(igd.ssdp));
        let out = open(&gw, &reqwest::Client::new(), 4443, 600).await;
        assert!(out.mapping.is_none(), "this router refuses every port");
        let adds = igd.actions("AddPortMapping");
        assert_eq!(adds.len(), 3, "it tried three ports");
        assert!(
            adds[1].contains("<NewExternalPort>") && !adds[1].contains("<NewExternalPort>4443<")
        );
    }

    #[tokio::test]
    async fn a_router_that_refuses_every_protocol_is_reported_with_each_reason() {
        let router = FakePmpRouter::start(Mode::Refuse, Mode::Refuse, wan(), 4443).await;
        let igd = FakeIgd::start(wan(), Some(501)).await;
        let gw = gateway(Some(router.addr), Some(igd.ssdp));
        let out = open(&gw, &reqwest::Client::new(), 4443, 600).await;
        assert!(out.mapping.is_none());
        let by: Vec<_> = out
            .refused
            .iter()
            .map(|(p, why)| (*p, why.as_str()))
            .collect();
        assert_eq!(by[0], (Proto::Pcp, "not authorized"));
        assert_eq!(by[1], (Proto::NatPmp, "not authorized or refused"));
        assert_eq!(by[2].0, Proto::Upnp);
        assert!(by[2].1.contains("501"), "{}", by[2].1);
        // Refused, but the router still told its WAN address (used to tell CGNAT apart).
        assert_eq!(out.wan, Some(wan()));
    }

    #[tokio::test]
    async fn no_router_at_all_means_three_silences_and_no_mapping() {
        let out = open(&gateway(None, None), &reqwest::Client::new(), 4443, 600).await;
        assert!(out.mapping.is_none() && out.wan.is_none());
        assert_eq!(out.refused.len(), 3);
        assert!(
            out.refused.iter().all(|(_, why)| why == "no answer"),
            "{:?}",
            out.refused
        );
    }

    #[tokio::test]
    async fn a_check_behind_a_router_that_maps_says_mapped_not_verified() {
        let router = FakePmpRouter::start(Mode::Accept, Mode::Silent, wan(), 4443).await;
        let cfg = Config {
            port: 4443,
            gateway: Some(gateway(Some(router.addr), None)),
            host_public4: false,
            v6: None,
            overlay: false,
            seen: Some(wan()),
        };
        let report = check(&cfg, &reqwest::Client::new()).await;
        assert_eq!(report.topology, [Topo::Forwardable]);
        let text = report.render();
        assert!(text.contains("mapped, not verified from outside"), "{text}");
        assert!(
            text.contains("PCP") && text.contains("93.184.216.34:4443"),
            "{text}"
        );
        assert!(text.contains("Next: none"), "{text}");
        assert!(text.lines().count() <= 4, "one short block: {text}");
    }

    #[tokio::test]
    async fn a_host_with_a_public_address_asks_no_router() {
        let router = FakePmpRouter::start(Mode::Accept, Mode::Accept, wan(), 4443).await;
        let cfg = Config {
            port: 4443,
            gateway: Some(gateway(Some(router.addr), None)),
            host_public4: true,
            v6: None,
            overlay: false,
            seen: Some(wan()),
        };
        let report = check(&cfg, &reqwest::Client::new()).await;
        assert!(router.seen.lock().unwrap().is_empty());
        assert_eq!(report.topology, [Topo::PublicHost]);
        assert!(report.render().contains("ufw allow 4443/udp"));
    }

    // --- classification and the rest, without a network ---------------------------------------------------------

    fn facts(wan: Option<&str>, seen: Option<&str>) -> Facts {
        Facts {
            wan: wan.map(|s| s.parse().unwrap()),
            seen: seen.map(|s| s.parse().unwrap()),
            gateway: true,
            ..Facts::default()
        }
    }

    #[test]
    fn a_public_wan_address_that_matches_what_the_outside_sees_is_port_forwardable() {
        assert_eq!(
            classify(&facts(Some("93.184.216.34"), Some("93.184.216.34"))),
            [Topo::Forwardable]
        );
        // The router knows its address but the echo service could not be asked.
        assert_eq!(
            classify(&facts(Some("93.184.216.34"), None)),
            [Topo::Forwardable]
        );
    }

    #[test]
    fn a_private_or_shared_wan_address_is_cgnat() {
        for wan in [
            "10.20.30.40",
            "192.168.8.2",
            "100.64.5.5",
            "100.127.0.1",
            "172.16.0.9",
        ] {
            assert_eq!(
                classify(&facts(Some(wan), Some("198.51.100.9"))),
                [Topo::Cgnat],
                "{wan}"
            );
        }
    }

    #[test]
    fn a_wan_address_that_differs_from_the_seen_one_is_double_nat() {
        assert_eq!(
            classify(&facts(Some("93.184.216.34"), Some("198.51.100.9"))),
            [Topo::Cgnat]
        );
    }

    #[test]
    fn a_router_that_says_nothing_is_a_nat_of_unknown_kind() {
        assert_eq!(
            classify(&facts(None, Some("93.184.216.34"))),
            [Topo::BehindNat]
        );
    }

    #[test]
    fn no_internet_ipv6_overlay_and_public_hosts() {
        assert_eq!(classify(&facts(None, None)), [Topo::NoInternet]);
        let mut f = facts(None, None);
        f.v6 = Some("2001:db8::1".parse().unwrap());
        assert_eq!(
            classify(&f),
            [Topo::Ipv6],
            "IPv6 alone is not 'no internet'"
        );
        f.overlay = true;
        assert_eq!(classify(&f), [Topo::Ipv6, Topo::Overlay]);
        let public = Facts {
            host_public4: true,
            ..Facts::default()
        };
        assert_eq!(classify(&public), [Topo::PublicHost]);
    }

    #[test]
    fn the_cgnat_block_names_the_options_and_offers_no_port_forward() {
        let f = facts(Some("100.72.1.1"), Some("198.51.100.9"));
        let text = Report::new(f, Outcome::default(), 4443).render();
        assert!(text.contains("S4"), "{text}");
        assert!(text.contains("overlay"), "{text}");
        assert!(!text.contains("forward UDP"), "{text}");
    }

    #[test]
    fn a_refused_mapping_asks_for_a_manual_forward() {
        let out = Outcome {
            refused: vec![(Proto::Pcp, "no answer".into())],
            ..Outcome::default()
        };
        let text = Report::new(
            facts(Some("93.184.216.34"), Some("93.184.216.34")),
            out,
            4443,
        )
        .render();
        assert!(text.contains("forward UDP 4443"), "{text}");
        assert!(text.contains("not opened"), "{text}");
    }

    #[test]
    fn the_lease_is_renewed_at_half() {
        assert_eq!(renew_after(3600), Duration::from_secs(1800));
        assert_eq!(
            renew_after(40),
            Duration::from_secs(30),
            "never more often than 30 s"
        );
    }

    #[test]
    fn the_default_gateway_is_read_from_the_route_table() {
        let route =
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
                     eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                     eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(
            parse_default_gateway(route),
            Some("192.168.1.1".parse().unwrap())
        );
        assert_eq!(parse_default_gateway("Iface\n"), None);
    }

    #[test]
    fn the_pcp_request_and_answer_have_the_rfc_layout() {
        let nonce = [9u8; 12];
        let r = pcp_request("192.168.1.5".parse().unwrap(), &nonce, 4443, 4443, 3600);
        assert_eq!((r.len(), r[0], r[1]), (60, 2, 1));
        assert_eq!(&r[4..8], &3600u32.to_be_bytes());
        assert_eq!(&r[20..24], &[192, 168, 1, 5]);
        assert_eq!((r[36], &r[40..42]), (17, &4443u16.to_be_bytes()[..]));
        let mut answer = r.clone();
        answer[1] = 0x81;
        answer[3] = 0;
        answer[42..44].copy_from_slice(&5555u16.to_be_bytes());
        answer[44..60].copy_from_slice(&v4_mapped(wan()));
        assert_eq!(
            parse_pcp_map(&answer, &nonce),
            Ok((5555, Some(wan()), 3600))
        );
        answer[3] = 2;
        assert_eq!(parse_pcp_map(&answer, &nonce), Err("not authorized".into()));
        assert!(parse_pcp_map(&answer, &[1u8; 12]).is_err());
    }

    #[test]
    fn the_wan_connection_service_is_found_in_a_nested_description() {
        let desc = "<root><device><serviceList><service><serviceType>urn:schemas-upnp-org:service:Layer3Forwarding:1</serviceType>\
            <controlURL>/l3</controlURL></service></serviceList><deviceList><device><serviceList><service>\
            <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType><controlURL>/ctl/ip</controlURL>\
            </service></serviceList></device></deviceList></device></root>";
        let (kind, url) = igd_service(desc, "http://192.168.1.1:5000/rootDesc.xml").unwrap();
        assert!(kind.ends_with("WANIPConnection:1"));
        assert_eq!(url, "http://192.168.1.1:5000/ctl/ip");
    }
}
