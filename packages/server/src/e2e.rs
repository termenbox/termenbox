//! Whole-server tests: a real client over each transport (WebSocket and WebRTC data channel),
//! a mock GitHub, and real tmux. Every scenario runs over both transports.
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use str0m::channel::{ChannelConfig, ChannelId};
use str0m::config::Fingerprint;
use str0m::ice::IceCreds;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::auth::AuthState;
use crate::cert::testing::{now, Device, Signer};
use crate::rtc::{self, RtcInfo, CHANNEL_ID, SERVER_PWD, SERVER_UFRAG, TERM_CHANNEL_ID};
use crate::sessions::SessionManager;
use crate::{router, AppState};

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("127.0.0.1:{}", addr.port())
}

struct Fixture {
    ws_addr: String,
    rtc: RtcInfo,
    claim: String,
    signer: Signer,
    server_id: String,
    auth: Arc<AuthState>,
    dir: std::path::PathBuf,
    stopped: Arc<std::sync::atomic::AtomicBool>,
    hub: crate::events::EventHub,
    events_socket: std::path::PathBuf,
}

/// A browser: its device key and the certificate the hosted function issued for it.
struct Who {
    device: Device,
    cert: String,
}

impl Fixture {
    fn who(&self, id: u64, login: &str) -> Who {
        let device = Device::new();
        let cert = self.signer.issue(&device, id, login, now() + 3600);
        Who { device, cert }
    }
}

static KEEPER: std::sync::OnceLock<String> = std::sync::OnceLock::new();

extern "C" fn kill_keeper() {
    if let Some(name) = KEEPER.get() {
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &format!("={name}")])
            .output();
    }
}

/// These tests share the machine's default tmux server, which exits as soon as its last session is closed; a test
/// starting a session at that moment fails. One idle session, started once and killed when the test process exits,
/// keeps the server up for all of them.
fn keep_tmux_alive() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let name = format!("e2ekeep{}", std::process::id());
        let started = std::process::Command::new("tmux")
            .args(["new-session", "-d", "-s", &name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if started && KEEPER.set(name).is_ok() {
            // SAFETY: registers a plain function that only runs a child process.
            unsafe { libc::atexit(kill_keeper) };
        }
    });
}

async fn fixture() -> Fixture {
    keep_tmux_alive();
    let dir = std::env::temp_dir().join(format!("termenbox-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let signer = Signer::new("k1");
    let server_id = "test-server-id".to_string();
    let auth = Arc::new(AuthState::new(dir.clone(), signer.keys(), server_id.clone()).unwrap());
    let claim = auth.claim_token().unwrap().unwrap();
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag = stopped.clone();
    let hub = crate::events::EventHub::new([9; 32]);
    let events_socket = dir.join("events.sock");
    tokio::spawn(crate::events::serve_socket(
        hub.clone(),
        events_socket.clone(),
    ));
    let env = crate::events::EventEnv {
        hub: hub.clone(),
        socket: events_socket.clone(),
    };
    let mut state = AppState {
        auth: auth.clone(),
        sessions: Arc::new(
            SessionManager::new(
                Arc::new(crate::backend::TmuxBackend::default().with_events(env)),
                crate::backend::RunMode::Container,
            )
            .with_events(hub.clone())
            .with_uninstall(
                "test-vps".into(),
                Arc::new(move || stop_flag.store(true, std::sync::atomic::Ordering::SeqCst)),
            ),
        ),
        server_name: "test-vps".into(),
        mode: "container".into(),
        rtc_info: None,
    };
    let cert = rtc::load_or_create_cert(&dir).unwrap();
    let (socket, _) = rtc::bind_transport(
        Ipv4Addr::LOCALHOST.into(),
        Some(0), // an ephemeral port, so tests can run side by side
        rtc::UDP_PORT_RANGE,
        None,
    )
    .await
    .unwrap();
    let rtc = rtc::start_with_socket(
        socket,
        Ipv4Addr::LOCALHOST.into(),
        cert,
        rtc::Limits::default(),
        state.clone(),
    )
    .await
    .unwrap();
    state.rtc_info = Some(rtc.clone());
    Fixture {
        ws_addr: serve(router(state, false)).await,
        rtc,
        claim,
        signer,
        server_id,
        auth,
        dir,
        stopped,
        hub,
        events_socket,
    }
}

// --- test clients -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Kind {
    Ws,
    Rtc,
}

enum Conn {
    Ws(
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ),
    Rtc(RtcClient),
}

impl Conn {
    async fn open(kind: Kind, f: &Fixture) -> Conn {
        match kind {
            Kind::Ws => Conn::Ws(
                connect_async(format!("ws://{}/ws", f.ws_addr))
                    .await
                    .unwrap()
                    .0,
            ),
            Kind::Rtc => Conn::Rtc(
                RtcClient::connect(&f.rtc, &f.rtc.fingerprint)
                    .await
                    .unwrap(),
            ),
        }
    }

    /// Runs the handshake as `who`, signing for `server_id`; the reply (`auth_ok` or `error`) is next.
    async fn login(&mut self, f: &Fixture, who: &Who, claim: &str) {
        self.login_for(&f.server_id, who, &who.cert, claim).await
    }

    async fn login_for(&mut self, server_id: &str, who: &Who, cert: &str, claim: &str) {
        self.login_lanes(server_id, who, cert, claim, false).await
    }

    /// Like `login_for`, asking (or not) for terminal bytes on their own lane.
    async fn login_lanes(
        &mut self,
        server_id: &str,
        who: &Who,
        cert: &str,
        claim: &str,
        channels: bool,
    ) {
        self.send(json!({"type":"hello"})).await;
        let ch = self.recv_type("challenge").await;
        let nonce = ch["nonce"].as_str().unwrap();
        self.send(json!({
            "type":"auth","cert":cert,"server_token":claim,"channels":channels,
            "signature": who.device.sign(server_id, nonce),
        }))
        .await;
    }

    async fn send(&mut self, v: Value) {
        match self {
            Conn::Ws(ws) => ws.send(Message::Text(v.to_string())).await.unwrap(),
            Conn::Rtc(c) => c.send(Frame::Text(v.to_string())),
        }
    }

    /// Terminal input on the `term` lane.
    async fn send_term(&mut self, bytes: &[u8]) {
        match self {
            Conn::Ws(ws) => {
                let mut frame = vec![crate::link::LANE_TERM];
                frame.extend_from_slice(bytes);
                ws.send(Message::Binary(frame)).await.unwrap()
            }
            Conn::Rtc(c) => c.send(Frame::Term(bytes.to_vec())),
        }
    }

    async fn recv(&mut self) -> Option<Value> {
        match self {
            Conn::Ws(ws) => loop {
                match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
                    Ok(Some(Ok(Message::Text(t)))) => {
                        return Some(serde_json::from_str(&t).unwrap())
                    }
                    Ok(Some(Ok(Message::Binary(b))))
                        if b.first() == Some(&crate::link::LANE_TERM) =>
                    {
                        return Some(term_message(&b[1..]))
                    }
                    Ok(Some(Ok(_))) => continue,
                    _ => return None,
                }
            },
            Conn::Rtc(c) => match tokio::time::timeout(Duration::from_secs(5), c.rx.recv()).await {
                Ok(Some(Frame::Text(t))) => Some(serde_json::from_str(&t).unwrap()),
                Ok(Some(Frame::Term(b))) => Some(term_message(&b)),
                _ => None,
            },
        }
    }

    /// The next message of any of these types, skipping others.
    async fn recv_any(&mut self, types: &[&str]) -> Value {
        for _ in 0..50 {
            let v = self.recv().await.expect("connection closed early");
            if types.iter().any(|t| v["type"] == *t) {
                return v;
            }
        }
        panic!("none of {types:?} arrived");
    }

    async fn recv_type(&mut self, ty: &str) -> Value {
        for _ in 0..50 {
            let v = self.recv().await.expect("connection closed early");
            if v["type"] == ty {
                return v;
            }
        }
        panic!("no {ty} message");
    }

    async fn output_contains(&mut self, needle: &str) -> bool {
        let mut seen = String::new();
        for _ in 0..100 {
            let Some(v) = self.recv().await else {
                return false;
            };
            if v["type"] == "output" || v["type"] == "term" {
                seen.push_str(&String::from_utf8_lossy(
                    &B64.decode(v["data"].as_str().unwrap()).unwrap(),
                ));
                if seen.contains(needle) {
                    return true;
                }
            }
        }
        false
    }
}

/// A WebRTC data-channel client built the way the browser builds it: from the server's IP, port and
/// pinned fingerprint only, with its own ICE ufrag/pwd set to one random string.
struct RtcClient {
    to_net: mpsc::UnboundedSender<Frame>,
    rx: mpsc::UnboundedReceiver<Frame>,
}

/// What travels on the data channels: text on the status channel, bytes on the `term` channel.
enum Frame {
    Text(String),
    Term(Vec<u8>),
}

/// A `term` frame as the tests read it: `{"type":"term","data":<base64>}`.
fn term_message(bytes: &[u8]) -> Value {
    json!({"type": "term", "data": B64.encode(bytes)})
}

fn parse_fingerprint(s: &str) -> Fingerprint {
    let (func, hex) = s.split_once(' ').unwrap();
    Fingerprint {
        hash_func: func.into(),
        bytes: hex
            .split(':')
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect(),
    }
}

impl RtcClient {
    /// Resolves once the data channel is open, or errors after 5 s.
    async fn connect(info: &RtcInfo, pinned_fingerprint: &str) -> Result<RtcClient, String> {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local = socket.local_addr().unwrap();
        let server = SocketAddr::new(info.ip, info.port);
        let ufrag: String = uuid::Uuid::new_v4().simple().to_string();

        let mut rtc = Rtc::builder().build(Instant::now());
        rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
        rtc.add_remote_candidate(Candidate::host(server, "udp").unwrap());
        let cid: ChannelId;
        let term_cid: ChannelId;
        {
            let mut d = rtc.direct_api();
            d.set_ice_controlling(true);
            d.set_local_ice_credentials(IceCreds {
                ufrag: ufrag.clone(),
                pass: ufrag.clone(),
            });
            d.set_remote_ice_credentials(IceCreds {
                ufrag: SERVER_UFRAG.into(),
                pass: SERVER_PWD.into(),
            });
            d.set_remote_fingerprint(parse_fingerprint(pinned_fingerprint));
            d.start_dtls(true).unwrap();
            d.start_sctp(true);
            cid = d.create_data_channel(ChannelConfig {
                label: "t".into(),
                negotiated: Some(CHANNEL_ID),
                ..Default::default()
            });
            term_cid = d.create_data_channel(ChannelConfig {
                label: "term".into(),
                negotiated: Some(TERM_CHANNEL_ID),
                ..Default::default()
            });
        }

        let (to_net, mut from_app) = mpsc::unbounded_channel::<Frame>();
        let (to_app, rx) = mpsc::unbounded_channel::<Frame>();
        let (open_tx, open_rx) = tokio::sync::oneshot::channel::<()>();
        let mut open_tx = Some(open_tx);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2000];
            let mut pending: Vec<Frame> = vec![];
            let mut open = false;
            loop {
                let mut next = Instant::now() + Duration::from_millis(100);
                loop {
                    if open {
                        while let Some(m) = pending.first() {
                            let (id, binary, bytes) = match m {
                                Frame::Text(t) => (cid, false, t.as_bytes()),
                                Frame::Term(b) => (term_cid, true, b.as_slice()),
                            };
                            let Some(mut ch) = rtc.channel(id) else { break };
                            if ch.write(binary, bytes).unwrap_or(false) {
                                pending.remove(0);
                            } else {
                                break;
                            }
                        }
                    }
                    match rtc.poll_output() {
                        Ok(Output::Transmit(t)) => {
                            let _ = socket.send_to(&t.contents, t.destination).await;
                        }
                        Ok(Output::Timeout(t)) => {
                            next = next.min(t);
                            break;
                        }
                        Ok(Output::Event(Event::ChannelOpen(..))) => {
                            open = true;
                            if let Some(tx) = open_tx.take() {
                                let _ = tx.send(());
                            }
                        }
                        Ok(Output::Event(Event::ChannelData(d))) => {
                            let _ = to_app.send(if d.id == term_cid {
                                Frame::Term(d.data.to_vec())
                            } else {
                                Frame::Text(String::from_utf8_lossy(&d.data).into_owned())
                            });
                        }
                        Ok(Output::Event(_)) => {}
                        Err(_) => return,
                    }
                }
                tokio::select! {
                    r = socket.recv_from(&mut buf) => {
                        if let Ok((n, source)) = r {
                            if let Ok(contents) = (&buf[..n]).try_into() {
                                let _ = rtc.handle_input(Input::Receive(Instant::now(), Receive {
                                    proto: Protocol::Udp, source, destination: local, contents,
                                }));
                            }
                        }
                    }
                    m = from_app.recv() => match m { Some(m) => pending.push(m), None => return },
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(next)) => {}
                }
                let _ = rtc.handle_input(Input::Timeout(Instant::now()));
                if !rtc.is_alive() {
                    return;
                }
            }
        });

        match tokio::time::timeout(Duration::from_secs(5), open_rx).await {
            Ok(Ok(())) => Ok(RtcClient { to_net, rx }),
            _ => Err("data channel did not open".into()),
        }
    }

    fn send(&self, frame: Frame) {
        let _ = self.to_net.send(frame);
    }
}

// --- scenarios, each run over both transports ---------------------------------------------------

async fn only_the_claiming_github_user_gets_in(kind: Kind) {
    let f = fixture().await;
    let (alice, bob) = (f.who(1, "alice"), f.who(2, "bob"));

    // Unclaimed: no claim token, no entry.
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, "").await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("not registered"));

    // A certificate from an unknown signer is rejected.
    let mallory = Who {
        cert: Signer::new("k1").issue(&alice.device, 1, "alice", now() + 60),
        device: Device::new(),
    };
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &mallory, &f.claim).await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("invalid"));

    // Alice claims it.
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    let ok = c.recv_type("auth_ok").await;
    assert_eq!(
        (
            ok["user"].as_str().unwrap(),
            ok["server_name"].as_str().unwrap()
        ),
        ("alice", "test-vps")
    );

    // Bob cannot, even with the (now spent) claim token; Alice returns without it.
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &bob, &f.claim).await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("another GitHub user"));
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, "").await;
    c.recv_type("auth_ok").await;
}

async fn a_connection_for_another_server_expired_or_revoked_certificates_fail(kind: Kind) {
    let f = fixture().await;
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;

    // A signature a rogue server collected for itself does not work here.
    let mut c = Conn::open(kind, &f).await;
    c.login_for("some-other-server", &alice, &alice.cert, "")
        .await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("sign in again"));

    // An expired certificate.
    let old = f.signer.issue(&alice.device, 1, "alice", now() - 5);
    let mut c = Conn::open(kind, &f).await;
    c.login_for(&f.server_id, &alice, &old, "").await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("expired"));

    // A certificate from a signer under an unknown kid.
    let unknown = Signer::new("unknown").issue(&alice.device, 1, "alice", now() + 60);
    let mut c = Conn::open(kind, &f).await;
    c.login_for(&f.server_id, &alice, &unknown, "").await;
    c.recv_type("error").await;

    // The old wire format, which carried a GitHub token, is not accepted.
    let mut c = Conn::open(kind, &f).await;
    c.send(json!({"type":"auth","token":"gho_secret","server_token":""}))
        .await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("Authenticate first"));

    // Revoking a device (as the owner, from another device) locks it out.
    let phone = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &phone, "").await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"revoke_device","device":phone.device.id()}))
        .await;
    c.recv_type("device_revoked").await;
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &phone, "").await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("revoked"));
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, "").await;
    c.recv_type("auth_ok").await;
    assert_eq!(f.auth.revoked_devices(), vec![phone.device.id()]);
}

async fn uninstall_needs_the_name_and_a_recent_login_then_wipes_the_config(kind: Kind) {
    let f = fixture().await;
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;

    // Wrong name.
    c.send(json!({"type":"uninstall","confirm_name":"other"}))
        .await;
    let msg = c.recv_type("error").await;
    assert!(msg["message"].as_str().unwrap().contains("does not match"));

    // A certificate older than an hour, and one from before certificates carried an issue time.
    for iat in [Some(now() - 7200), None] {
        let cert = f
            .signer
            .issue_at(&alice.device, 1, "alice", now() + 3600, iat);
        let mut stale = Conn::open(kind, &f).await;
        stale.login_for(&f.server_id, &alice, &cert, "").await;
        stale.recv_type("auth_ok").await;
        stale
            .send(json!({"type":"uninstall","confirm_name":"test-vps"}))
            .await;
        let msg = stale.recv_type("error").await;
        assert!(
            msg["message"].as_str().unwrap().contains("Sign in again"),
            "{msg}"
        );
    }
    assert!(!f.stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert!(f.dir.join("owner.json").exists());

    // Right name and a fresh certificate.
    c.send(json!({"type":"uninstall","confirm_name":"test-vps"}))
        .await;
    let done = c.recv_type("uninstall_done").await;
    assert_eq!(done["mode"], "container");
    let left: Vec<_> = std::fs::read_dir(&f.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["uninstalled".to_string()]);
    assert!(f.stopped.load(std::sync::atomic::Ordering::SeqCst));
}

/// Kills a test's tmux session when the test ends, also when it fails halfway (a failed assertion used to leave
/// `detach…` sessions behind on the machine that ran the tests).
struct KillOnDrop(String);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &format!("={}", self.0)])
            .output();
    }
}

async fn nothing_is_served_before_auth(kind: Kind) {
    let f = fixture().await;
    let mut c = Conn::open(kind, &f).await;
    c.send(json!({"type":"list_sessions"})).await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("Authenticate first"));
}

async fn detach_preserves_the_connection_and_session(kind: Kind) {
    let f = fixture().await;
    let name = format!("detach{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"new_session","name":name})).await;
    c.recv_type("sessions").await;
    c.send(json!({"type":"attach","session_id":name})).await;
    c.recv_type("session_attached").await;
    c.send(json!({"type":"detach"})).await;
    c.send(json!({"type":"input","data":B64.encode("echo DETACHED_INPUT_MUST_NOT_RUN\n")}))
        .await;
    c.send(json!({"type":"ping"})).await;
    c.recv_type("pong").await;
    c.send(json!({"type":"peek","session":name})).await;
    let snapshot = c.recv_type("snapshot").await;
    assert!(!snapshot["text"]
        .as_str()
        .unwrap()
        .contains("DETACHED_INPUT_MUST_NOT_RUN"));
    c.send(json!({"type":"attach","session_id":name})).await;
    c.recv_type("session_attached").await;
    c.send(json!({"type":"input","data":B64.encode("echo reattached$((40+2))\n")}))
        .await;
    assert!(c.output_contains("reattached42").await);
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

async fn reconnect_resumes_the_tmux_session_with_its_screen(kind: Kind) {
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let name = format!("e2e{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());

    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"new_session","name":name})).await;
    let sessions = c.recv_type("sessions").await;
    assert!(sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == name.as_str()));

    c.send(json!({"type":"attach","session_id":name})).await;
    c.recv_type("session_attached").await;
    c.send(json!({"type":"input","data":B64.encode("echo resume$((40+2))\n")}))
        .await;
    assert!(c.output_contains("resume42").await);

    // The network drops.
    drop(c);

    // The client reconnects without a claim token, lists, and re-attaches: the screen comes back.
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, "").await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"attach","session_id":name})).await;
    c.recv_type("session_attached").await;
    assert!(
        c.output_contains("resume42").await,
        "earlier output is repainted"
    );

    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

/// A session gets its event variables; what a tool writes to the socket reaches a connected client
/// sanitised and in order, and a client that connects later gets the snapshot and the missed events.
async fn agent_events_flow_from_a_session_tool_to_the_client(kind: Kind) {
    use tokio::io::AsyncWriteExt;
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let name = format!("ev{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"new_session","name":name})).await;
    c.recv_type("sessions").await;
    c.send(json!({"type":"attach","session_id":name})).await;
    c.recv_type("session_attached").await;
    c.send(json!({"type":"input","data":B64.encode("echo who:$TERMENBOX_SESSION; test -S \"$TERMENBOX_EVENT_SOCKET\" && echo sock:ok\n")})).await;
    assert!(
        c.output_contains(&format!("who:{name}")).await,
        "the session knows its name"
    );

    let send = |data: Value, token: String| {
        let line = json!({"session": name, "token": token, "adapter": "claude", "data": data})
            .to_string()
            + "\n";
        let sock = f.events_socket.clone();
        async move {
            let mut s = tokio::net::UnixStream::connect(sock).await.unwrap();
            s.write_all(line.as_bytes()).await.unwrap();
        }
    };
    send(
        json!({"hook_event_name": "UserPromptSubmit"}),
        "forged".into(),
    )
    .await;
    send(
        json!({"hook_event_name": "UserPromptSubmit"}),
        f.hub.token(&name),
    )
    .await;
    send(
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
               "tool_input": {"command": "curl -H \"Authorization: Bearer hunter2hunter2\" 10.1.2.3:8080\nrm -rf /"}}),
        f.hub.token(&name),
    )
    .await;

    let mut seen = Vec::new();
    for _ in 0..50 {
        let v = c.recv().await.expect("closed");
        if v["type"] == "agent_events" {
            seen.extend(v["events"].as_array().unwrap().clone());
        }
        if seen.len() >= 2 {
            break;
        }
    }
    let wire = serde_json::to_string(&seen).unwrap();
    assert_eq!(seen[0]["kind"], "state");
    assert_eq!(seen[0]["state"], "working");
    assert_eq!(
        seen[0]["seq"], 1,
        "the forged event was refused and consumed no seq"
    );
    assert_eq!(seen[1]["kind"], "tool");
    for leaked in ["hunter2", "10.1.2.3", "\\n", "\\u001b"] {
        assert!(!wire.contains(leaked), "{leaked} leaked: {wire}");
    }

    // A client that connects later: snapshot first, then what it missed.
    let mut d = Conn::open(kind, &f).await;
    d.login(&f, &alice, "").await;
    d.recv_type("auth_ok").await;
    d.send(json!({"type":"agent_sync","after":{name.clone(): 1}}))
        .await;
    let snap = d.recv_type("agent_snapshot").await;
    assert_eq!(snap["states"][0]["session"], name.as_str());
    assert_eq!(snap["states"][0]["state"], "working");
    assert_eq!(snap["states"][0]["seq"], 2);
    assert_eq!(snap["states"][0]["stale"], false);
    let missed = d.recv_type("agent_events").await;
    assert_eq!(
        missed["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["seq"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [2]
    );

    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

/// xterm answers tmux's colour-palette query with one `input` message per colour, about 260 in a burst right
/// after attach. The connection must survive that (it used to be dropped when 32 messages queued up).
async fn a_burst_of_input_messages_does_not_drop_the_connection(kind: Kind) {
    let f = fixture().await;
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    for i in 0..300 {
        c.send(json!({"type":"input","data":B64.encode(format!("\x1b]4;{i};rgb:eeee/eeee/eeee\x1b\\"))})).await;
    }
    c.send(json!({"type":"ping"})).await;
    c.recv_type("pong").await;
}

/// `peek` returns what is on a session's screen without attaching, and says unchanged for a hash it already gave.
async fn peek_shows_a_session_screen_without_attaching(kind: Kind) {
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let name = format!("pk{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"new_session","name":name})).await;
    c.recv_type("sessions").await;
    c.send(json!({"type":"reply","session":name,"text":"echo peeked$((40+2))"}))
        .await;
    let mut snap = json!(null);
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        c.send(json!({"type":"peek","session":name})).await;
        snap = c.recv_type("snapshot").await;
        if snap["text"].as_str().unwrap_or("").contains("peeked42") {
            break;
        }
    }
    assert!(
        snap["text"].as_str().unwrap().contains("peeked42"),
        "{snap}"
    );
    let hash = snap["hash"].as_str().unwrap().to_string();
    c.send(json!({"type":"peek","session":name,"hash":hash}))
        .await;
    let again = c.recv_type("snapshot").await;
    assert_eq!(again["unchanged"], true);
    assert_eq!(again["text"], "");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

async fn windows_close_window_and_close_session_go_through_the_handlers(kind: Kind) {
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let name = format!("cw{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    // These tests share the machine's default tmux server, which exits when another test closes its last
    // session at the wrong moment; creating the session is retried when that race loses.
    for attempt in 0..3 {
        c.send(json!({"type":"new_session","name":name})).await;
        let reply = c.recv_any(&["sessions", "error"]).await;
        if reply["type"] == "sessions" {
            break;
        }
        assert!(attempt < 2, "tmux would not start a session: {reply}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    c.send(json!({"type":"windows","session":name})).await;
    let one = c.recv_type("window_list").await;
    assert_eq!(one["windows"].as_array().unwrap().len(), 1);
    assert!(std::process::Command::new("tmux")
        .args(["new-window", "-t", &format!("={name}:"), "-n", "second"])
        .status()
        .unwrap()
        .success());
    c.send(json!({"type":"windows","session":name})).await;
    let two = c.recv_type("window_list").await;
    assert_eq!(two["windows"].as_array().unwrap().len(), 2);
    assert_eq!(two["windows"][1]["name"], "second");
    // A new window opens in the directory of the current one and becomes the current one.
    // Window 1 was just made and may not have settled in its directory yet: ask from window 0.
    c.send(json!({"type":"select_window","session":name,"index":0}))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    c.send(json!({"type":"new_window","session":name})).await;
    let mut three = serde_json::Value::Null;
    for _ in 0..30 {
        c.send(json!({"type":"windows","session":name})).await;
        three = c.recv_type("window_list").await;
        // A fresh pane reports tmux's own directory until its shell has started: wait for it to settle.
        let w = three["windows"].as_array().unwrap();
        if w.len() == 3 && w[2]["path"] == w[0]["path"] {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let w = three["windows"].as_array().unwrap();
    assert_eq!(w.len(), 3, "{three}");
    assert_eq!(w[2]["path"], w[0]["path"], "{three}");
    assert_eq!(w[2]["active"], true, "{three}");
    c.send(json!({"type":"close_window","session":name,"index":2}))
        .await;
    c.recv_type("sessions").await;
    c.send(json!({"type":"close_window","session":name,"index":1}))
        .await;
    c.recv_type("sessions").await;
    c.send(json!({"type":"windows","session":name})).await;
    assert_eq!(
        c.recv_type("window_list").await["windows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    c.send(json!({"type":"close_session","session":name})).await;
    let list = c.recv_type("sessions").await;
    assert!(
        !list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == name),
        "{list}"
    );
    // A name that is not a session is an error, not a crash.
    c.send(json!({"type":"close_session","session":"no-such-session"}))
        .await;
    c.recv_type("error").await;
}

/// A client that asks for lanes gets `channels` in `auth_ok`, sends keystrokes and reads output as raw bytes on
/// the `term` lane, and an Attention raised during a flood of output still arrives within a second. A client that
/// does not ask is told nothing about lanes and keeps getting base64 JSON output.
async fn terminal_bytes_use_their_own_lane_and_status_is_not_held_up_by_output(kind: Kind) {
    use tokio::io::AsyncWriteExt;
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let name = format!("ln{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");

    let mut old = Conn::open(kind, &f).await;
    old.login(&f, &alice, &f.claim).await;
    assert!(old.recv_type("auth_ok").await.get("channels").is_none());

    let mut c = Conn::open(kind, &f).await;
    c.login_lanes(&f.server_id, &alice, &alice.cert, "", true)
        .await;
    let ok = c.recv_type("auth_ok").await;
    assert_eq!(ok["channels"], json!(["term"]));
    c.send(json!({"type":"new_session","name":name})).await;
    c.recv_type("sessions").await;
    c.send(json!({"type":"attach","session_id":name})).await;
    c.recv_type("session_attached").await;
    c.send_term(b"echo lane-$((20+22))\n").await;
    let mut raw = String::new();
    for _ in 0..100 {
        let v = c.recv().await.expect("closed");
        assert_ne!(
            v["type"], "output",
            "output must not be JSON on a lane connection"
        );
        if v["type"] == "term" {
            raw.push_str(&String::from_utf8_lossy(
                &B64.decode(v["data"].as_str().unwrap()).unwrap(),
            ));
            if raw.contains("lane-42") {
                break;
            }
        }
    }
    assert!(raw.contains("lane-42"), "{raw:?}");

    // A flood of output, and in the middle of it a hook says the agent needs the owner.
    c.send_term(b"head -c 4000000 /dev/zero | tr '\\0' 'x'; echo\n")
        .await;
    let line = json!({"session": name, "token": f.hub.token(&name), "adapter": "claude",
        "data": {"hook_event_name": "Notification", "message": "needs a decision"}})
    .to_string()
        + "\n";
    let mut s = tokio::net::UnixStream::connect(&f.events_socket)
        .await
        .unwrap();
    let (mut bytes, mut chunks) = (0usize, 0usize);
    // Wait until the flood is really flowing, then send the event.
    while bytes < 200_000 {
        let v = c.recv().await.expect("closed");
        if v["type"] == "term" {
            bytes += B64.decode(v["data"].as_str().unwrap()).unwrap().len();
            chunks += 1;
        }
    }
    let sent = Instant::now();
    s.write_all(line.as_bytes()).await.unwrap();
    let mut during = 0usize;
    let waited = loop {
        let v = c.recv().await.expect("closed");
        if v["type"] == "attention" && v["items"][0]["kind"] == "needs_input" {
            break sent.elapsed();
        }
        if v["type"] == "term" {
            during += 1;
        }
        assert!(
            sent.elapsed() < Duration::from_secs(5),
            "no attention message"
        );
    };
    eprintln!("lanes {kind:?}: attention after {waited:?}, {during} term messages in between, {chunks} before");
    assert!(waited < Duration::from_secs(1), "{waited:?}");
    assert_eq!(
        v_first_prompt(&mut c).await,
        Some("needs a decision".to_string())
    );

    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

/// The prompt text of the open `needs_input` item, from the next `attention` message.
async fn v_first_prompt(c: &mut Conn) -> Option<String> {
    for _ in 0..2000 {
        let v = c.recv().await?;
        if v["type"] == "attention" {
            if let Some(p) = v["items"][0]["prompt"].as_str() {
                return Some(p.to_string());
            }
        }
    }
    None
}

/// The single JSON pipe still carries a long flood of output, then the command after it.
async fn a_flood_of_output_on_the_single_pipe_still_reaches_the_client(kind: Kind) {
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let name = format!("fl{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"new_session","name":name})).await;
    c.recv_type("sessions").await;
    c.send(json!({"type":"attach","session_id":name})).await;
    c.recv_type("session_attached").await;
    c.send(json!({"type":"input","data":B64.encode("timeout 5 sh -c 'while :; do seq 1 5000; done'; echo end-$((40+2))\n")})).await;
    let start = Instant::now();
    let mut seen = String::new();
    while start.elapsed() < Duration::from_secs(20) {
        let Some(v) = c.recv().await else { break };
        if v["type"] == "output" {
            seen.push_str(&String::from_utf8_lossy(
                &B64.decode(v["data"].as_str().unwrap()).unwrap(),
            ));
            if seen.contains("end-42") {
                return;
            }
        }
    }
    panic!("end-42 never arrived: {} bytes", seen.len());
}

/// `session_view` answers with the cleaned tail, tier and a parsed prompt without attaching; a message the server does
/// not know is ignored and the connection stays up; an answer for a prompt that is not on the pane is refused.
async fn session_view_reads_the_screen_and_unknown_messages_are_ignored(kind: Kind) {
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let name = format!("sv{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"new_session","name":name})).await;
    c.recv_type("sessions").await;
    c.send(json!({"type":"reply","session":name,"text":"echo Deploy$((40+2)) now? '(y/n)'"}))
        .await;
    let mut view = json!(null);
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        c.send(json!({"type":"session_view","session":name})).await;
        view = c.recv_type("session_view").await;
        // The typed command line also shows "(y/n)"; wait for its output, the line with the sum worked out.
        if view["prompt"]["question"]
            .as_str()
            .is_some_and(|q| q.contains("Deploy42"))
        {
            break;
        }
    }
    assert_eq!(view["tier"], "t3", "{view}");
    assert_eq!(view["prompt"]["kind"], "yes_no", "{view}");
    assert!(
        view["prompt"]["question"]
            .as_str()
            .unwrap()
            .contains("Deploy42"),
        "{view}"
    );
    assert!(
        !view.to_string().contains("\"keys\""),
        "options carry no keys: {view}"
    );
    let hash = view["hash"].as_str().unwrap().to_string();
    c.send(json!({"type":"session_view","session":name,"hash":hash}))
        .await;
    let again = c.recv_type("session_view").await;
    assert_eq!(again["unchanged"], true);
    assert_eq!(again["tail"], json!([]));
    // an old server would skip this; this one must too, and keep serving
    c.send(json!({"type":"a_message_from_the_future","x":1}))
        .await;
    c.send(json!({"type":"ping"})).await;
    c.recv_type("pong").await;
    c.send(json!({"type":"session_answer","session":name,"prompt_id":"v-000000000000","option":0}))
        .await;
    let err = c.recv_type("error").await;
    assert!(
        err["message"].as_str().unwrap().contains("changed"),
        "{err}"
    );
    let id = view["prompt"]["id"].as_str().unwrap();
    c.send(json!({"type":"session_answer","session":name,"prompt_id":id,"option":9}))
        .await;
    let err = c.recv_type("error").await;
    assert!(
        err["message"].as_str().unwrap().contains("No such choice"),
        "{err}"
    );
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

macro_rules! over_both_transports {
    ($($name:ident),*) => {$(
        mod $name {
            #[tokio::test] async fn ws() { super::$name(super::Kind::Ws).await }
            #[tokio::test] async fn rtc() { super::$name(super::Kind::Rtc).await }
        }
    )*};
}

over_both_transports!(
    only_the_claiming_github_user_gets_in,
    a_connection_for_another_server_expired_or_revoked_certificates_fail,
    uninstall_needs_the_name_and_a_recent_login_then_wipes_the_config,
    nothing_is_served_before_auth,
    reconnect_resumes_the_tmux_session_with_its_screen,
    detach_preserves_the_connection_and_session,
    agent_events_flow_from_a_session_tool_to_the_client,
    a_burst_of_input_messages_does_not_drop_the_connection,
    peek_shows_a_session_screen_without_attaching,
    session_view_reads_the_screen_and_unknown_messages_are_ignored,
    windows_close_window_and_close_session_go_through_the_handlers,
    terminal_bytes_use_their_own_lane_and_status_is_not_held_up_by_output,
    a_flood_of_output_on_the_single_pipe_still_reaches_the_client
);

// --- WebRTC-specific ----------------------------------------------------------------------------

#[tokio::test]
async fn a_client_that_pins_the_wrong_fingerprint_never_connects() {
    let f = fixture().await;
    let wrong = format!("sha-256 {}", vec!["AB"; 32].join(":"));
    assert!(RtcClient::connect(&f.rtc, &wrong).await.is_err());
}

#[tokio::test]
async fn garbage_and_foreign_stun_create_no_peers_and_the_server_stays_up() {
    let f = fixture().await;
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server = SocketAddr::new(f.rtc.ip, f.rtc.port);
    for payload in [&b"hello"[..], &[0u8; 64][..], &[0xffu8; 1500][..]] {
        sock.send_to(payload, server).await.unwrap();
    }
    // A real client still connects afterwards.
    let mut c = Conn::open(Kind::Rtc, &f).await;
    c.send(json!({"type":"list_sessions"})).await;
    c.recv_type("error").await;
}

#[tokio::test]
async fn half_open_flood_is_capped_and_real_clients_still_get_in() {
    let f = fixture().await;
    let server = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), f.rtc.port);
    // 200 distinct sources (ports) from one IP: at most `max_per_ip` can become peers.
    let mut socks = vec![];
    for i in 0..200u32 {
        let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let u = format!("{}:{:016x}{:016x}", SERVER_UFRAG, i, i);
        let mut attr = vec![0, 6, 0, u.len() as u8];
        attr.extend_from_slice(u.as_bytes());
        attr.extend(std::iter::repeat(0).take((4 - u.len() % 4) % 4));
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
        m.extend([9u8; 12]);
        m.extend(attr);
        s.send_to(&m, server).await.unwrap();
        socks.push(s);
    }
    // The flood came from 127.0.0.1, which is also where the test client lives, so the per-IP cap
    // applies to it as well; after the half-open peers expire it must get in again.
    tokio::time::sleep(Duration::from_secs(11)).await;
    let mut c = Conn::open(Kind::Rtc, &f).await;
    c.send(json!({"type":"list_sessions"})).await;
    c.recv_type("error").await;
}
