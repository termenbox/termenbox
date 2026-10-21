//! `--print-pairing` and LAN-only mode (DECISION-0123), as the real binary sees them: a server started with
//! TERMENBOX_LAN_ONLY=1 and no public address (no internet to look it up) still serves, and the link the CLI
//! prints carries a bundle signed by the server key, with the LAN address, the server's DTLS fingerprint and the
//! claim token of an unclaimed server in it.
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use termenbox_core::bundle::{Bundle, CandidateKind};

fn config_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tm-pair-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn free_tcp_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_reserved_udp_port() -> u16 {
    (4443..=4542u16)
        .find(|p| UdpSocket::bind(("127.0.0.1", *p)).is_ok())
        .expect("no free port in the reserved range")
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn base(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_termenbox-server"));
    cmd.env("TERMENBOX_CONFIG_DIR", dir)
        .env("TERMENBOX_HOST", "127.0.0.1")
        .env("TERMENBOX_LAN_ONLY", "1")
        .env("TERMENBOX_LAN_ADDRS", "192.168.55.5")
        .env("TERMENBOX_CLIENT_URL", "https://app.test")
        .env("TERMENBOX_SERVER_NAME", "pair-box")
        .env_remove("TERMENBOX_PUBLIC_IP")
        .env_remove("TERMENBOX_DIRECTORY_URL");
    cmd
}

fn get(http_port: u16, path: &str) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", http_port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut text = String::new();
    s.read_to_string(&mut text).ok()?;
    text.split("\r\n\r\n").nth(1).map(str::to_string)
}

fn pairing(dir: &Path) -> String {
    let out = base(dir).arg("--print-pairing").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn link_of(text: &str) -> &str {
    text.split_whitespace()
        .find(|w| w.starts_with("https://app.test/#/pair/"))
        .expect("a pairing link")
}

#[test]
fn a_lan_only_server_with_no_public_address_prints_a_signed_pairing_link() {
    let dir = config_dir("lan");
    let http_port = free_tcp_port();
    let log = File::create(dir.join("server.log")).unwrap();
    let err = File::create(dir.join("server.err")).unwrap();

    // Before it has ever started there is no port to pair with, and it says so.
    let early = base(&dir).arg("--print-pairing").output().unwrap();
    assert!(!early.status.success());
    assert!(String::from_utf8_lossy(&early.stderr).contains("start it once"));

    let _server = Server(
        base(&dir)
            .env("TERMENBOX_PORT", http_port.to_string())
            .env("TERMENBOX_UDP_PORT", free_reserved_udp_port().to_string())
            .env("TERMENBOX_DEV_INFO", "on")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(err))
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let info = loop {
        if let Some(body) = get(http_port, "/rtc-info").filter(|b| b.contains("fingerprint")) {
            break serde_json::from_str::<serde_json::Value>(&body).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "the LAN-only server never came up:\n{}",
            std::fs::read_to_string(dir.join("server.err")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(100));
    };

    let text = pairing(&dir);
    let link = link_of(&text);
    let (payload, claim) = link
        .trim_start_matches("https://app.test/#/pair/")
        .split_once("?claim=")
        .expect("an unclaimed server adds its claim token");
    assert_eq!(claim.len(), 43);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let bundle = Bundle::verify(payload, now, None, None).expect("the bundle verifies");
    assert_eq!(bundle.name, "pair-box");
    assert_eq!(bundle.fingerprint, info["fingerprint"].as_str().unwrap());
    let lan: Vec<_> = bundle
        .candidates
        .iter()
        .filter(|c| c.kind == CandidateKind::Lan)
        .collect();
    assert_eq!(lan.len(), 1);
    assert_eq!(
        (lan[0].addr.as_str(), lan[0].port as u64),
        ("192.168.55.5", info["port"].as_u64().unwrap())
    );
    assert!(
        bundle.candidates.iter().all(|c| c.kind.is_private()),
        "LAN-only advertises no public address: {:?}",
        bundle.candidates
    );

    // Each print is a new bundle, and a client that saw the first rejects an older one as stale.
    let second = pairing(&dir);
    let again = link_of(&second)
        .trim_start_matches("https://app.test/#/pair/")
        .split_once("?claim=")
        .unwrap()
        .0;
    let newer = Bundle::verify(again, now, None, Some(bundle.seq)).expect("newer seq");
    assert!(newer.seq > bundle.seq);
    assert!(Bundle::verify(payload, now, None, Some(newer.seq)).is_err());
}
