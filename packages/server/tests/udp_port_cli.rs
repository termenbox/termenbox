//! The reserved UDP range (DECISION-0118), as the real binary and installer see it: with no
//! TERMENBOX_UDP_PORT the server binds the first free port from 4443 to 4542 and remembers it, so two
//! servers with separate config dirs take different ports and a restart keeps its port.
//!
//! Each server gets its own config dir and an ephemeral HTTP port, and is asked over the dev-only
//! `/rtc-info` route which UDP port it bound, so the owner's own running service is never touched.
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const RANGE: std::ops::RangeInclusive<u16> = 4443..=4542;

fn config_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tm-udp-{}-{tag}", std::process::id()));
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

/// A free port from the reserved range, so an explicit choice is inside the range and can be remembered.
/// Ports the owner's own running service holds are skipped (their bind fails).
fn free_reserved_udp_port() -> u16 {
    for port in RANGE {
        if UdpSocket::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    panic!("no free port in the reserved range");
}

/// A server the test started; stopped when it goes out of scope, even on a failed assertion.
struct Server {
    child: Child,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(dir: &Path, http_port: u16, explicit: Option<u16>) -> Server {
    // Logs go to files: no pipe to fill, and a failed start can show them.
    let out = File::create(dir.join("server.log")).unwrap();
    let err = File::create(dir.join("server.err")).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_termenbox-server"));
    cmd.env("TERMENBOX_CONFIG_DIR", dir)
        .env("TERMENBOX_HOST", "127.0.0.1")
        .env("TERMENBOX_PORT", http_port.to_string())
        .env("TERMENBOX_PUBLIC_IP", "127.0.0.1")
        .env("TERMENBOX_DIRECTORY_URL", "")
        .env("TERMENBOX_DEV_INFO", "on")
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    match explicit {
        Some(port) => cmd.env("TERMENBOX_UDP_PORT", port.to_string()),
        None => cmd.env_remove("TERMENBOX_UDP_PORT"),
    };
    Server {
        child: cmd.spawn().unwrap(),
    }
}

/// Polls `/rtc-info` until the server reports the UDP port it bound.
fn wait_udp_port(dir: &Path, http_port: u16) -> u16 {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(port) = reported_udp_port(http_port) {
            return port;
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(dir.join("server.log")).unwrap_or_default();
            let err = std::fs::read_to_string(dir.join("server.err")).unwrap_or_default();
            panic!("the server never reported its UDP port on {http_port}\n--- stdout ---\n{log}\n--- stderr ---\n{err}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn reported_udp_port(http_port: u16) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", http_port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    stream
        .write_all(b"GET /rtc-info HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let body = response.split_once("\r\n\r\n")?.1;
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value["port"].as_u64().map(|p| p as u16)
}

/// These tests bind real ports; run them one at a time so a free-port probe is not reused by another.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn two_servers_pick_different_ports_from_the_reserved_range() {
    let _serial = serial();
    let (d1, d2) = (config_dir("a"), config_dir("b"));
    let (h1, h2) = (free_tcp_port(), free_tcp_port());
    let a = start(&d1, h1, None);
    let b = start(&d2, h2, None);
    let (pa, pb) = (wait_udp_port(&d1, h1), wait_udp_port(&d2, h2));

    assert!(RANGE.contains(&pa), "{pa} is outside the reserved range");
    assert!(RANGE.contains(&pb), "{pb} is outside the reserved range");
    assert_ne!(pa, pb, "two servers must not share one port");

    drop(a);
    drop(b);
    let _ = std::fs::remove_dir_all(&d1);
    let _ = std::fs::remove_dir_all(&d2);
}

#[test]
fn a_restart_keeps_the_port_it_saved() {
    let _serial = serial();
    let dir = config_dir("restart");
    let first = {
        let h = free_tcp_port();
        let child = start(&dir, h, None);
        let port = wait_udp_port(&dir, h);
        drop(child); // stop it before the restart
        port
    };

    let h = free_tcp_port();
    let child = start(&dir, h, None);
    let again = wait_udp_port(&dir, h);
    assert_eq!(again, first, "a restart must reuse the saved port");

    drop(child);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_explicit_port_is_used_as_given_and_remembered() {
    let _serial = serial();
    let dir = config_dir("explicit");
    let port = free_reserved_udp_port();
    let h = free_tcp_port();
    let first = start(&dir, h, Some(port));
    assert_eq!(wait_udp_port(&dir, h), port);
    drop(first); // stop it before the restart

    // An explicit port updates the saved file, so a later start without the flag keeps it.
    let h = free_tcp_port();
    let child = start(&dir, h, None);
    assert_eq!(
        wait_udp_port(&dir, h),
        port,
        "the explicit port must be remembered"
    );

    drop(child);
    let _ = std::fs::remove_dir_all(&dir);
}
