use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// True for addresses a browser on the internet could actually reach.
pub fn is_public_unicast(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let [o0, o1, ..] = a.octets();
            !(a.is_private()
                || a.is_loopback()
                || a.is_link_local()
                || a.is_multicast()
                || a.is_broadcast()
                || a.is_unspecified()
                || a.is_documentation()
                || (o0 == 100 && (64..128).contains(&o1)) // carrier-grade NAT
                || o0 == 0
                || o0 >= 240)
        }
        IpAddr::V6(a) => {
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || (a.segments()[0] & 0xffc0) == 0xfe80 // link-local
                || (a.segments()[0] & 0xfe00) == 0xfc00 // unique local
                || a == Ipv6Addr::LOCALHOST
                || a.to_ipv4_mapped().is_some_and(|v4| !is_public_unicast(IpAddr::V4(v4))))
        }
    }
}

/// The address of the interface that routes to the internet. No packet is sent.
pub fn local_route_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect(("8.8.8.8", 53)).ok()?;
    Some(s.local_addr().ok()?.ip())
}

/// Public IPv4 of this host: the route address when it is public (most VPSes), otherwise what an
/// external echo service sees (NAT-mapped clouds such as AWS, GCP, Oracle).
pub async fn discover_public_ip(http: &reqwest::Client) -> Option<IpAddr> {
    if let Some(ip) = local_route_ip().filter(|ip| is_public_unicast(*ip)) {
        return Some(ip);
    }
    external_ip_seen(http).await
}

/// What an external echo service sees this host as, whatever the host's own address is.
pub async fn external_ip_seen(http: &reqwest::Client) -> Option<IpAddr> {
    for url in ["https://api.ipify.org", "https://ifconfig.me/ip"] {
        if let Ok(resp) = http.get(url).send().await {
            if let Ok(text) = resp.text().await {
                if let Ok(ip) = text.trim().parse::<IpAddr>() {
                    if is_public_unicast(ip) {
                        return Some(ip);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_routable_addresses_count_as_public() {
        for ok in ["178.104.210.131", "8.8.8.8", "2606:4700::1111"] {
            assert!(is_public_unicast(ok.parse().unwrap()), "{ok}");
        }
        for bad in [
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "192.0.2.1",
            "::1",
            "fe80::1",
            "fd00::1",
            "ff02::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!is_public_unicast(bad.parse().unwrap()), "{bad}");
        }
    }
}
