//! The candidate bundle this server signs and pushes (DECISION-0123, R62): the addresses a client can race, all
//! around one identity (the server key and the pinned DTLS fingerprint). LAN and overlay addresses are found
//! here; the public ones come from the reachability check (`reach.rs`). Private addresses go to authenticated
//! clients and the pairing link only, never to the directory (GUARDRAIL-lan-addresses-never-reach-the-directory).
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use termenbox_core::bundle::{Bundle, Candidate, CandidateKind, MAX_CANDIDATES};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::directory::ServerKey;
use crate::netutil::is_public_unicast;
use crate::reach::Report;

/// A bundle is valid for a week and re-signed daily at the latest, so a server that is off for a while still hands
/// its next client a fresh one and a leaked old one dies on its own.
const LIFETIME_SECS: u64 = 7 * 24 * 3600;
const RESIGN_BEFORE_SECS: u64 = 24 * 3600;
/// Interfaces are looked at this often; a bundle is only re-signed when its candidates changed.
const RESCAN: Duration = Duration::from_secs(60);
/// A client races every candidate, so few are better; more would mostly be bridges and containers.
pub const MAX_LAN: usize = 4;

/// What an interface is, by its name.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Iface {
    /// A physical or ordinary network: its RFC1918 addresses are LAN candidates.
    Lan,
    /// Tailscale or WireGuard: an overlay address (decision 0123, rung 4).
    Overlay,
    /// Loopback, containers and VPN plumbing: never advertised.
    Skip,
}

pub fn classify_iface(name: &str) -> Iface {
    let n = name.to_ascii_lowercase();
    if n.starts_with("tailscale") || n.starts_with("wg") {
        return Iface::Overlay;
    }
    const SKIP: [&str; 9] = [
        "lo", "docker", "br-", "veth", "cni", "tap", "tun", "virbr", "flannel",
    ];
    if SKIP.iter().any(|p| n.starts_with(p)) {
        return Iface::Skip;
    }
    Iface::Lan
}

pub fn is_rfc1918(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

/// The addresses of this host's interfaces, as `(interface name, address)`.
pub fn interfaces() -> Vec<(String, IpAddr)> {
    use std::ffi::CStr;
    let mut out = Vec::new();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills a linked list that is read only until freeifaddrs; every pointer is checked.
    unsafe {
        if libc::getifaddrs(&mut head) != 0 {
            return out;
        }
        let mut cur = head;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || ifa.ifa_flags & libc::IFF_UP as u32 == 0 {
                continue;
            }
            let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
            match (*ifa.ifa_addr).sa_family as i32 {
                libc::AF_INET => {
                    let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                    out.push((name, IpAddr::V4(ip)));
                }
                libc::AF_INET6 => {
                    let sin6 = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    out.push((name, IpAddr::V6(Ipv6Addr::from(sin6.sin6_addr.s6_addr))));
                }
                _ => {}
            }
        }
        libc::freeifaddrs(head);
    }
    out
}

/// At most `MAX_LAN` RFC1918 IPv4 addresses on ordinary interfaces, or the owner's own list
/// (`TERMENBOX_LAN_ADDRS`, comma-separated) when set.
pub fn lan_addrs(ifaces: &[(String, IpAddr)], overridden: Option<&str>) -> Vec<Ipv4Addr> {
    if let Some(list) = overridden.filter(|s| !s.trim().is_empty()) {
        return list
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .take(MAX_LAN)
            .collect();
    }
    let mut out: Vec<Ipv4Addr> = Vec::new();
    for (name, ip) in ifaces {
        if let IpAddr::V4(v4) = ip {
            if classify_iface(name) == Iface::Lan && is_rfc1918(*v4) && !out.contains(v4) {
                out.push(*v4);
            }
        }
    }
    out.truncate(MAX_LAN);
    out
}

/// Tailscale and WireGuard addresses, or the owner's list (`TERMENBOX_OVERLAY_ADDRS`).
pub fn overlay_addrs(ifaces: &[(String, IpAddr)], overridden: Option<&str>) -> Vec<IpAddr> {
    if let Some(list) = overridden.filter(|s| !s.trim().is_empty()) {
        return list
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .take(MAX_LAN)
            .collect();
    }
    let mut out: Vec<IpAddr> = Vec::new();
    for (name, ip) in ifaces {
        let link_local = matches!(ip, IpAddr::V6(a) if (a.segments()[0] & 0xffc0) == 0xfe80);
        if classify_iface(name) == Iface::Overlay && !link_local && !out.contains(ip) {
            out.push(*ip);
        }
    }
    out.truncate(MAX_LAN);
    out
}

/// The first global IPv6 address on an ordinary interface (S5).
pub fn global_ipv6(ifaces: &[(String, IpAddr)]) -> Option<Ipv6Addr> {
    ifaces.iter().find_map(|(name, ip)| match ip {
        IpAddr::V6(a)
            if classify_iface(name) == Iface::Lan && is_public_unicast(IpAddr::V6(*a)) =>
        {
            Some(*a)
        }
        _ => None,
    })
}

/// What a bundle is built from.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub port: u16,
    pub lan: Vec<Ipv4Addr>,
    pub overlay: Vec<IpAddr>,
    /// The host's own public IPv4 (a VPS), when it has one.
    pub public4: Option<Ipv4Addr>,
    /// The router's mapping of the UDP port, from `reach` (external address and port).
    pub mapped4: Option<(Ipv4Addr, u16)>,
    /// A global IPv6 address this server also listens on.
    pub v6: Option<Ipv6Addr>,
    /// The address free STUN servers saw the transport socket at (`stun.rs`), when it does not depend on the server.
    pub stun4: Option<(Ipv4Addr, u16)>,
}

/// The candidate list for `inputs`: cheapest first (LAN, IPv6, IPv4, overlay). A WAN address that is not public
/// (CGNAT, a private router address) is not a candidate: it would only cost every client a failed attempt.
pub fn candidates(inputs: &Inputs) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut push = |kind, addr: IpAddr, port| {
        let c = Candidate {
            kind,
            addr: addr.to_string(),
            port,
        };
        if !out.contains(&c) {
            out.push(c);
        }
    };
    for ip in &inputs.lan {
        push(CandidateKind::Lan, IpAddr::V4(*ip), inputs.port);
    }
    if let Some(v6) = inputs.v6 {
        push(CandidateKind::Wan6, IpAddr::V6(v6), inputs.port);
    }
    if let Some((ip, port)) = inputs
        .mapped4
        .filter(|(ip, _)| is_public_unicast(IpAddr::V4(*ip)))
    {
        push(CandidateKind::Wan4, IpAddr::V4(ip), port);
    }
    if let Some((ip, port)) = inputs
        .stun4
        .filter(|(ip, _)| is_public_unicast(IpAddr::V4(*ip)))
    {
        push(CandidateKind::Wan4, IpAddr::V4(ip), port);
    }
    if let Some(ip) = inputs
        .public4
        .filter(|ip| is_public_unicast(IpAddr::V4(*ip)))
    {
        push(CandidateKind::Wan4, IpAddr::V4(ip), inputs.port);
    }
    for ip in &inputs.overlay {
        push(CandidateKind::Overlay, *ip, inputs.port);
    }
    out.truncate(MAX_CANDIDATES);
    out
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1)
}

fn read_seq(dir: &Path) -> u64 {
    std::fs::read_to_string(dir.join("bundle_seq"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// The next `seq`, remembered in `dir` so it keeps rising across restarts and between the running server and the
/// one-shot `--print-pairing`. The read-modify-write holds a file lock, because those are separate processes.
pub fn next_seq(dir: &Path) -> Result<u64> {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("bundle_seq.lock"))?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // Blocks until the other holder is done; released when `lock` is dropped.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    let next = read_seq(dir) + 1;
    let path = dir.join("bundle_seq");
    let tmp = dir.join(format!("bundle_seq.{}.tmp", std::process::id()));
    crate::auth::write_private(&tmp, next.to_string().as_bytes())?;
    std::fs::rename(&tmp, &path)?;
    drop(lock);
    Ok(next)
}

fn candidates_path(dir: &Path) -> PathBuf {
    dir.join("bundle_candidates.json")
}

fn published_path(dir: &Path) -> PathBuf {
    dir.join("bundle_published")
}

/// The name cut to the bundle's limit, which counts bytes: on a character boundary, so a name in Cyrillic, CJK or
/// with emoji still signs.
fn truncate_name(name: &str) -> String {
    let mut end = name.len().min(termenbox_core::bundle::MAX_NAME);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].to_string()
}

/// Signs a bundle for `list`; the wire string is what clients get.
pub fn sign(
    key: &ServerKey,
    seq: u64,
    fingerprint: &str,
    name: &str,
    published: bool,
    list: Vec<Candidate>,
) -> Result<(Bundle, String)> {
    sign_assisted(key, seq, fingerprint, name, published, list, None)
}

/// The owner's signaling Worker URL for a bundle (DECISION-0141): present only when `--setup-cloudflare` has run and
/// the server is not LAN-only. A relay is always on offer with it, since the setup makes the TURN key too.
pub fn assisted_signal(dir: &Path, enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }
    let url = crate::signal::Config::load(dir).ok().flatten()?.signal;
    (url.len() <= 200).then_some(url)
}

/// `sign`, with the signaling Worker's URL when the assisted path is set up.
pub fn sign_assisted(
    key: &ServerKey,
    seq: u64,
    fingerprint: &str,
    name: &str,
    published: bool,
    list: Vec<Candidate>,
    signal: Option<String>,
) -> Result<(Bundle, String)> {
    let bundle = Bundle {
        server_id: key.server_id(),
        seq,
        expires: now_secs() + LIFETIME_SECS,
        published,
        fingerprint: fingerprint.to_string(),
        name: truncate_name(name),
        candidates: list,
        relay: signal.is_some(),
        signal,
    };
    let signature = key.sign(&bundle.signed_bytes()?);
    let wire = bundle.encode(&signature)?;
    Ok((bundle, wire))
}

/// Re-reads the local interfaces for a bundle input, with the owner's overrides.
pub fn local_inputs(port: u16, lan_override: Option<&str>) -> Inputs {
    let ifaces = interfaces();
    Inputs {
        port,
        lan: lan_addrs(&ifaces, lan_override),
        overlay: overlay_addrs(
            &ifaces,
            std::env::var("TERMENBOX_OVERLAY_ADDRS").ok().as_deref(),
        ),
        ..Inputs::default()
    }
}

struct Current {
    bundle: Bundle,
    wire: String,
}

/// Keeps the current signed bundle: rebuilt when the addresses change, and before it expires.
pub struct PathHub {
    dir: PathBuf,
    key: Arc<ServerKey>,
    fingerprint: String,
    name: String,
    port: u16,
    lan_override: Option<String>,
    public4: Option<Ipv4Addr>,
    v6: Option<Ipv6Addr>,
    /// This server publishes to the directory (see `directory::publishes`); signed into every bundle.
    published: bool,
    reach: watch::Receiver<Option<Report>>,
    stun: watch::Receiver<Option<crate::stun::Mapped>>,
    /// The assisted path may be advertised (not LAN-only); `assisted_signal` then reads the owner's config.
    assisted: std::sync::atomic::AtomicBool,
    current: Mutex<Option<Current>>,
}

impl PathHub {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dir: PathBuf,
        key: Arc<ServerKey>,
        fingerprint: String,
        name: String,
        port: u16,
        lan_override: Option<String>,
        public4: Option<Ipv4Addr>,
        v6: Option<Ipv6Addr>,
        published: bool,
        reach: watch::Receiver<Option<Report>>,
    ) -> Arc<Self> {
        Self::new_with_stun(
            dir,
            key,
            fingerprint,
            name,
            port,
            lan_override,
            public4,
            v6,
            published,
            reach,
            watch::channel(None).1,
        )
    }

    /// `new`, with the STUN discovery's result as one more input to the candidates.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_stun(
        dir: PathBuf,
        key: Arc<ServerKey>,
        fingerprint: String,
        name: String,
        port: u16,
        lan_override: Option<String>,
        public4: Option<Ipv4Addr>,
        v6: Option<Ipv6Addr>,
        published: bool,
        reach: watch::Receiver<Option<Report>>,
        stun: watch::Receiver<Option<crate::stun::Mapped>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            dir,
            key,
            fingerprint,
            name,
            port,
            lan_override,
            public4,
            v6,
            published,
            reach,
            stun,
            assisted: std::sync::atomic::AtomicBool::new(false),
            current: Mutex::new(None),
        })
    }

    /// Sign the owner's signaling Worker into the bundle once `--setup-cloudflare` has made one.
    pub fn enable_assisted(&self) {
        self.assisted
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The wire string of the current bundle.
    pub fn current(&self) -> Option<String> {
        self.current.lock().ok()?.as_ref().map(|c| c.wire.clone())
    }

    /// The wire string to push to a client now: re-signed first when another process has taken a higher seq.
    pub fn current_fresh(&self) -> Option<String> {
        if let Err(e) = self.refresh() {
            warn!("cannot sign a candidate bundle: {e:#}");
        }
        self.current()
    }

    pub fn current_bundle(&self) -> Option<Bundle> {
        self.current.lock().ok()?.as_ref().map(|c| c.bundle.clone())
    }

    fn inputs(&self) -> Inputs {
        let mut inputs = local_inputs(self.port, self.lan_override.as_deref());
        inputs.public4 = self.public4;
        inputs.v6 = self.v6;
        inputs.mapped4 = self
            .reach
            .borrow()
            .as_ref()
            .and_then(|r| r.mapping.as_ref())
            .and_then(|m| m.external_ip.map(|ip| (ip, m.external_port)));
        inputs.stun4 = self.stun.borrow().as_ref().and_then(|m| m.candidate());
        inputs
    }

    /// Signs a new bundle when the candidates changed or the current one is close to its expiry.
    pub fn refresh(&self) -> Result<bool> {
        let list = candidates(&self.inputs());
        let signal = assisted_signal(
            &self.dir,
            self.assisted.load(std::sync::atomic::Ordering::Relaxed),
        );
        let mut cur = self
            .current
            .lock()
            .map_err(|_| anyhow::anyhow!("poisoned"))?;
        if let Some(c) = cur.as_ref() {
            // `--print-pairing` runs in another process and takes its own seq: a phone that paired with it keeps that
            // seq, and would reject this bundle as stale, so sign again above it.
            if c.bundle.candidates == list
                && c.bundle.signal == signal
                && c.bundle.expires > now_secs() + RESIGN_BEFORE_SECS
                && read_seq(&self.dir) <= c.bundle.seq
            {
                return Ok(false);
            }
        }
        let seq = next_seq(&self.dir)?;
        let (bundle, wire) = sign_assisted(
            &self.key,
            seq,
            &self.fingerprint,
            &self.name,
            self.published,
            list.clone(),
            signal,
        )?;
        info!(
            "Candidate bundle seq {seq}: {}",
            list.iter()
                .map(|c| format!("{:?}", c.kind).to_lowercase())
                .collect::<Vec<_>>()
                .join(", ")
        );
        // The one-shot `--print-pairing` keeps the public candidates the running server found.
        if let Ok(text) = serde_json::to_vec(&list) {
            let _ = crate::auth::write_private(&candidates_path(&self.dir), &text);
        }
        // ...and whether this server publishes, for the same reason.
        let _ = crate::auth::write_private(
            &published_path(&self.dir),
            if self.published { b"1" } else { b"0" },
        );
        *cur = Some(Current { bundle, wire });
        Ok(true)
    }

    pub async fn run(self: Arc<Self>) {
        let mut reach = self.reach.clone();
        let mut stun = self.stun.clone();
        let mut stun_open = true;
        loop {
            if let Err(e) = self.refresh() {
                warn!("cannot sign a candidate bundle: {e:#}");
            }
            tokio::select! {
                _ = tokio::time::sleep(RESCAN) => {}
                changed = reach.changed() => {
                    // No check is running (LAN-only): just wait for the next scan.
                    if changed.is_err() {
                        tokio::time::sleep(RESCAN).await;
                    }
                }
                // With no STUN discovery the channel is closed: that branch must then never fire, not spin.
                open = async {
                    if stun_open { stun.changed().await.is_ok() } else { std::future::pending().await }
                } => stun_open = open,
            }
        }
    }
}

/// For `--print-pairing`: a freshly signed bundle with this host's current LAN and overlay addresses and the public
/// candidates the running server last found.
pub fn pairing_bundle(
    dir: &Path,
    key: &ServerKey,
    fingerprint: &str,
    name: &str,
    port: u16,
    lan_override: Option<&str>,
    lan_only: bool,
) -> Result<String> {
    let remembered: Vec<Candidate> = std::fs::read(candidates_path(dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut list = candidates(&local_inputs(port, lan_override));
    for c in remembered {
        if !c.kind.is_private() && !list.contains(&c) {
            list.push(c);
        }
    }
    list.truncate(MAX_CANDIDATES);
    // Whether the running server publishes: what it last signed. Not known (never started) or LAN-only here means no.
    let published =
        !lan_only && std::fs::read_to_string(published_path(dir)).is_ok_and(|s| s.trim() == "1");
    let seq = next_seq(dir).context("cannot remember the bundle sequence number")?;
    let signal = assisted_signal(dir, !lan_only);
    Ok(sign_assisted(key, seq, fingerprint, name, published, list, signal)?.1)
}

/// The link a phone opens or scans: the signed bundle in the fragment, so no host ever sees it. An unclaimed server
/// adds its one-time claim token outside the signed part (it is a bearer secret, trusted like a claim link).
pub fn pairing_link(client_url: &str, wire: &str, claim: Option<&str>) -> String {
    let base = format!("{}/#/pair/{wire}", client_url.trim_end_matches('/'));
    match claim {
        Some(c) => format!("{base}?claim={c}"),
        None => base,
    }
}

/// The one block every pairing command prints (decision 0126): the QR, the link it encodes and, once the server has
/// one, the short code. An unclaimed server's link also claims it; an owned server's link only re-adds it.
pub struct PairingBlock {
    pub link: String,
    pub claims: bool,
    pub code: Option<String>,
}

impl PairingBlock {
    pub fn new(client_url: &str, wire: &str, claim: Option<&str>, code: Option<&str>) -> Self {
        PairingBlock {
            link: pairing_link(client_url, wire, claim),
            claims: claim.is_some(),
            code: code.map(str::to_string),
        }
    }

    /// The terminal text. `with_qr` is false when output is piped and no `--qr` was asked for.
    pub fn render(&self, with_qr: bool) -> String {
        let mut out = String::new();
        if with_qr {
            if let Some(qr) = crate::auth::claim_qr(&self.link) {
                out.push_str(&qr);
                out.push('\n');
            }
        }
        out.push_str(&format!(
            "Scan this, open the link, or type the code in Add Server:\n\n  {}\n\n",
            self.link
        ));
        if let Some(code) = &self.code {
            out.push_str(&format!("  Code: {code}\n\n"));
        }
        out.push_str("The link carries this server's name, key fingerprint and addresses, signed by the server; the app shows them before it connects.\n");
        if self.claims {
            out.push_str(
                "The first user to confirm becomes the owner. It works for 15 minutes, once.\n",
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ifs(list: &[(&str, &str)]) -> Vec<(String, IpAddr)> {
        list.iter()
            .map(|(n, a)| (n.to_string(), a.parse().unwrap()))
            .collect()
    }

    #[test]
    fn only_ordinary_interfaces_give_lan_candidates() {
        let all = ifs(&[
            ("lo", "127.0.0.1"),
            ("eth0", "192.168.1.20"),
            ("wlan0", "10.0.0.7"),
            ("docker0", "172.17.0.1"),
            ("br-1a2b", "172.18.0.1"),
            ("veth9", "172.19.0.1"),
            ("cni0", "10.244.0.1"),
            ("tailscale0", "100.64.1.2"),
            ("wg0", "10.8.0.2"),
            ("tun0", "10.9.0.2"),
            ("tap0", "10.10.0.2"),
            ("eth1", "169.254.3.3"),
            ("eth2", "203.0.113.9"),
            ("eth3", "172.20.0.5"),
            ("eth4", "192.168.1.20"),
        ]);
        let got = lan_addrs(&all, None);
        assert_eq!(
            got,
            ["192.168.1.20", "10.0.0.7", "172.20.0.5"].map(|s| s.parse::<Ipv4Addr>().unwrap())
        );
    }

    #[test]
    fn at_most_four_lan_candidates_and_the_override_wins() {
        let many: Vec<(String, IpAddr)> = (1..=8)
            .map(|i| (format!("eth{i}"), format!("192.168.{i}.2").parse().unwrap()))
            .collect();
        assert_eq!(lan_addrs(&many, None).len(), MAX_LAN);
        let over = lan_addrs(&many, Some("10.1.1.1, 10.1.1.2,nonsense"));
        assert_eq!(
            over,
            ["10.1.1.1", "10.1.1.2"].map(|s| s.parse::<Ipv4Addr>().unwrap())
        );
    }

    #[test]
    fn overlay_comes_from_tailscale_and_wireguard_only() {
        let all = ifs(&[
            ("eth0", "192.168.1.20"),
            ("tailscale0", "100.64.1.2"),
            ("tailscale0", "fe80::1"),
            ("wg0", "10.8.0.2"),
            ("tun0", "10.9.0.2"),
        ]);
        let got = overlay_addrs(&all, None);
        assert_eq!(
            got,
            ["100.64.1.2", "10.8.0.2"].map(|s| s.parse::<IpAddr>().unwrap())
        );
    }

    #[test]
    fn a_global_ipv6_is_found_but_not_a_unique_local_one() {
        let all = ifs(&[
            ("eth0", "fe80::1"),
            ("eth0", "fd00::5"),
            ("eth0", "2001:4860::9"),
        ]);
        assert_eq!(global_ipv6(&all), Some("2001:4860::9".parse().unwrap()));
        assert_eq!(global_ipv6(&ifs(&[("eth0", "fd00::5")])), None);
    }

    #[test]
    fn a_cgnat_or_private_wan_address_is_not_a_candidate() {
        let inputs = Inputs {
            port: 4443,
            lan: vec!["192.168.1.20".parse().unwrap()],
            mapped4: Some(("100.72.1.1".parse().unwrap(), 4443)),
            public4: Some("10.0.0.3".parse().unwrap()),
            ..Inputs::default()
        };
        let c = candidates(&inputs);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].kind, CandidateKind::Lan);
    }

    #[test]
    fn mapped_and_public_addresses_become_wan_candidates_in_order() {
        let inputs = Inputs {
            port: 4443,
            lan: vec!["192.168.1.20".parse().unwrap()],
            mapped4: Some(("93.184.216.34".parse().unwrap(), 50123)),
            v6: Some("2001:4860::9".parse().unwrap()),
            overlay: vec!["100.64.1.2".parse().unwrap()],
            ..Inputs::default()
        };
        let kinds: Vec<_> = candidates(&inputs)
            .iter()
            .map(|c| (c.kind, c.port))
            .collect();
        assert_eq!(
            kinds,
            [
                (CandidateKind::Lan, 4443),
                (CandidateKind::Wan6, 4443),
                (CandidateKind::Wan4, 50123),
                (CandidateKind::Overlay, 4443),
            ]
        );
    }

    #[test]
    fn seq_rises_across_calls_and_the_pairing_link_keeps_the_bundle_in_the_fragment() {
        let dir = std::env::temp_dir().join(format!("termenbox-seq-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(next_seq(&dir).unwrap(), 1);
        assert_eq!(next_seq(&dir).unwrap(), 2);
        let link = pairing_link("https://app.example/", "PAYLOAD", Some("tok"));
        assert_eq!(link, "https://app.example/#/pair/PAYLOAD?claim=tok");
        assert_eq!(
            pairing_link("https://app.example", "P", None),
            "https://app.example/#/pair/P"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_signed_bundle_verifies_and_never_carries_more_than_it_was_given() {
        let key = ServerKey::from_seed([3u8; 32]);
        let fp = format!("sha-256 {}", vec!["0A"; 32].join(":"));
        let list = candidates(&Inputs {
            port: 4443,
            lan: vec!["192.168.1.20".parse().unwrap()],
            ..Inputs::default()
        });
        let (_, wire) = sign(&key, 5, &fp, "box", false, list.clone()).unwrap();
        let b = Bundle::verify(&wire, now_secs(), Some(&key.server_id()), Some(4)).unwrap();
        assert_eq!((b.seq, b.candidates, b.name), (5, list, "box".to_string()));
        assert_eq!(b.fingerprint, fp);
    }

    fn lan_list() -> Vec<Candidate> {
        candidates(&Inputs {
            port: 4443,
            lan: vec!["192.168.1.20".parse().unwrap()],
            ..Inputs::default()
        })
    }

    #[test]
    fn a_name_longer_than_the_limit_in_bytes_is_cut_on_a_character_boundary_and_still_signs() {
        let key = ServerKey::from_seed([3u8; 32]);
        let fp = format!("sha-256 {}", vec!["0A"; 32].join(":"));
        // 40 Cyrillic letters are 80 bytes but only 40 characters; 20 emoji are 80 bytes too.
        for name in [
            "д".repeat(40),
            "🦀".repeat(20),
            "a".repeat(200),
            "ñ".repeat(33),
        ] {
            let (b, wire) = sign(&key, 1, &fp, &name, false, lan_list())
                .unwrap_or_else(|e| panic!("{} bytes: {e}", name.len()));
            assert!(b.name.len() <= termenbox_core::bundle::MAX_NAME);
            assert!(name.starts_with(&b.name), "a prefix of the name");
            assert!(
                b.name.len() > termenbox_core::bundle::MAX_NAME - 4,
                "no more is cut than one character needs"
            );
            Bundle::verify(&wire, now_secs(), Some(&key.server_id()), None).unwrap();
        }
    }

    #[test]
    fn concurrent_callers_never_get_the_same_seq() {
        let dir = std::env::temp_dir().join(format!("termenbox-seq-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    (0..40).map(|_| next_seq(&dir).unwrap()).collect::<Vec<_>>()
                })
            })
            .collect();
        let mut all: Vec<u64> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();
        all.sort_unstable();
        let expected: Vec<u64> = (1..=640).collect();
        assert_eq!(all, expected, "every seq handed out once, none skipped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pairing_bundle_made_by_another_process_does_not_make_the_running_servers_bundle_stale() {
        let dir = std::env::temp_dir().join(format!("termenbox-seq-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = Arc::new(ServerKey::from_seed([3u8; 32]));
        let fp = format!("sha-256 {}", vec!["0A"; 32].join(":"));
        let hub = PathHub::new(
            dir.clone(),
            key.clone(),
            fp.clone(),
            "box".into(),
            4443,
            Some("192.168.1.20".into()),
            None,
            None,
            false,
            crate::reach::Handle::idle().0.rx,
        );
        hub.refresh().unwrap();
        let before = hub.current_bundle().unwrap().seq;
        // `termenbox-server --print-pairing`: a separate process bumping the same counter.
        let pairing =
            pairing_bundle(&dir, &key, &fp, "box", 4443, Some("192.168.1.20"), false).unwrap();
        let (paired, _) = Bundle::decode(&pairing).unwrap();
        assert!(paired.seq > before);
        // The phone has now stored the pairing seq. What the server pushes next has to beat it.
        let pushed = hub.current_fresh().expect("a bundle");
        let (pushed, _) = Bundle::decode(&pushed).unwrap();
        assert!(
            pushed.seq > paired.seq,
            "pushed seq {} must be above the pairing seq {}",
            pushed.seq,
            paired.seq
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bundle_carries_the_owners_worker_only_once_it_is_set_up_and_never_when_lan_only() {
        let dir = std::env::temp_dir().join(format!("termenbox-sig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = Arc::new(ServerKey::from_seed([3u8; 32]));
        let fp = format!("sha-256 {}", vec!["0A"; 32].join(":"));
        let hub = PathHub::new(
            dir.clone(), key.clone(), fp.clone(), "box".into(), 4443, Some("192.168.1.20".into()),
            None, None, false, crate::reach::Handle::idle().0.rx,
        );
        hub.enable_assisted();
        hub.refresh().unwrap();
        assert_eq!(hub.current_bundle().unwrap().signal, None);

        let url = "wss://termenbox-relay-abc.owner.workers.dev/s/6kpsYabcdefghijkl";
        crate::auth::write_private(
            &dir.join(crate::signal::CONFIG_FILE),
            format!(r#"{{"signal":"{url}","turn_key_id":"k","turn_key_token":"t"}}"#).as_bytes(),
        )
        .unwrap();
        assert!(hub.refresh().unwrap(), "the new Worker is a change: sign again");
        let b = hub.current_bundle().unwrap();
        assert_eq!((b.signal.as_deref(), b.relay), (Some(url), true));

        let decode = |lan_only| {
            let wire = pairing_bundle(&dir, &key, &fp, "box", 4443, Some("192.168.1.20"), lan_only).unwrap();
            Bundle::decode(&wire).unwrap().0
        };
        assert_eq!(decode(false).signal.as_deref(), Some(url));
        assert_eq!(decode(true).signal, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stun_mapped_address_becomes_a_wan4_candidate_and_changes_the_bundle_seq() {
        use crate::stun::{Mapped, Mapping};
        let dir = std::env::temp_dir().join(format!("termenbox-seq-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = Arc::new(ServerKey::from_seed([3u8; 32]));
        let fp = format!("sha-256 {}", vec!["0A"; 32].join(":"));
        let (tx, rx) = watch::channel(None);
        let hub = PathHub::new_with_stun(
            dir.clone(), key, fp, "box".into(), 4443, Some("192.168.1.20".into()),
            None, None, false, crate::reach::Handle::idle().0.rx, rx,
        );
        hub.refresh().unwrap();
        let first = hub.current_bundle().unwrap();
        assert!(first.candidates.iter().all(|c| c.kind != CandidateKind::Wan4));
        assert!(!hub.refresh().unwrap(), "nothing changed: same seq");
        let mapped = |mapping, addr: &str| Mapped { addr: addr.parse().unwrap(), mapping, answers: 2 };
        // A symmetric mapping depends on the destination: not advertised.
        tx.send(Some(mapped(Mapping::Symmetric, "93.184.216.34:50000"))).unwrap();
        assert!(!hub.refresh().unwrap());
        tx.send(Some(mapped(Mapping::EndpointIndependent, "93.184.216.34:50000"))).unwrap();
        assert!(hub.refresh().unwrap());
        let next = hub.current_bundle().unwrap();
        assert!(next.seq > first.seq);
        assert!(next.candidates.iter().any(|c| (c.kind, c.addr.as_str(), c.port) == (CandidateKind::Wan4, "93.184.216.34", 50000)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn hub_in(dir: &Path, key: &Arc<ServerKey>, fp: &str, published: bool) -> Arc<PathHub> {
        PathHub::new(
            dir.to_path_buf(),
            key.clone(),
            fp.to_string(),
            "box".into(),
            4443,
            Some("192.168.1.20".into()),
            None,
            None,
            published,
            crate::reach::Handle::idle().0.rx,
        )
    }

    #[test]
    fn a_bundle_says_whether_this_server_publishes_to_the_directory_and_the_pairing_link_agrees() {
        let key = Arc::new(ServerKey::from_seed([3u8; 32]));
        let fp = format!("sha-256 {}", vec!["0A"; 32].join(":"));
        let pairing = |dir: &Path, lan_only: bool| {
            let wire = pairing_bundle(dir, &key, &fp, "box", 4443, Some("192.168.1.20"), lan_only)
                .unwrap();
            Bundle::decode(&wire).unwrap().0.published
        };
        let fresh = || {
            let dir = std::env::temp_dir().join(format!("termenbox-pub-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };

        // A server that publishes, and the `--print-pairing` run next to it (another process: it reads what the
        // server last signed).
        let dir = fresh();
        let hub = hub_in(&dir, &key, &fp, true);
        hub.refresh().unwrap();
        assert!(hub.current_bundle().unwrap().published);
        assert!(pairing(&dir, false));
        // The owner's own LAN-only setting wins over whatever the last run said.
        assert!(!pairing(&dir, true));

        // A LAN-only server never says it publishes.
        let dir = fresh();
        let hub = hub_in(&dir, &key, &fp, false);
        hub.refresh().unwrap();
        assert!(!hub.current_bundle().unwrap().published);
        assert!(!pairing(&dir, false));

        // Never started: no claim that anything is published.
        assert!(!pairing(&fresh(), false));
    }

    #[test]
    fn pairing_block_has_link_and_claim_only_when_unowned() {
        let unowned = PairingBlock::new("https://app.example", "PAYLOAD", Some("tok"), None);
        let text = unowned.render(false);
        assert!(text.contains("https://app.example/#/pair/PAYLOAD?claim=tok"));
        assert!(text.contains("becomes the owner"));
        let owned = PairingBlock::new("https://app.example", "PAYLOAD", None, None).render(false);
        assert!(owned.contains("https://app.example/#/pair/PAYLOAD"));
        assert!(!owned.contains("claim=") && !owned.contains("becomes the owner"));
    }

    #[test]
    fn pairing_block_qr_fits_an_80_column_terminal_for_a_real_bundle() {
        let key = ServerKey::from_seed([3u8; 32]);
        let fp = format!("sha-256 {}", vec!["0A"; 32].join(":"));
        let list = candidates(&Inputs {
            port: 4443,
            lan: vec!["192.168.1.20".parse().unwrap()],
            ..Inputs::default()
        });
        let (_, wire) = sign(&key, 1, &fp, "box", false, list).unwrap();
        let claim = "A".repeat(43);
        let text =
            PairingBlock::new("https://app.termenbox.dev", &wire, Some(&claim), None).render(true);
        let widest = text
            .lines()
            .take_while(|l| !l.starts_with("Scan this"))
            .map(|l| l.chars().count())
            .max()
            .unwrap();
        // Plan R65 asked for 60, but a claim plus a signed bundle needs a version-13 code (69 columns at the lowest correction).
        assert!(widest <= 80, "QR is {widest} columns wide");
    }
}
