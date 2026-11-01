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
use termenbox_core::events::{Confidence, EventKind, Source};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::auth::AuthState;
use crate::cert::testing::{now, Device, Signer};
use crate::rtc::{
    self, RtcInfo, CHANNEL_ID, CONTROL_CHANNEL_ID, SERVER_PWD, SERVER_UFRAG, TERM_CHANNEL_ID,
};
use crate::workspaces::WorkspaceManager;
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
    assisted: rtc::Assisted,
    claim: String,
    signer: Signer,
    server_id: String,
    auth: Arc<AuthState>,
    dir: std::path::PathBuf,
    stopped: Arc<std::sync::atomic::AtomicBool>,
    hub: crate::events::EventHub,
    events_socket: std::path::PathBuf,
    /// The key id the candidate bundle is signed by (not the fake id the handshake uses).
    bundle_key_id: String,
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

/// These tests share the machine's default tmux server, which exits as soon as its last workspace is closed; a test
/// starting a workspace at that moment fails. One idle workspace, started once and killed when the test process exits,
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
    fixture_with(0).await
}

/// `history_runs` finished tasks of a long-lived server are already in its history, as on a machine in daily use.
async fn fixture_with(history_runs: usize) -> Fixture {
    fixture_full(history_runs, rtc::Limits::default()).await
}

async fn fixture_full(history_runs: usize, limits: rtc::Limits) -> Fixture {
    keep_tmux_alive();
    let dir = std::env::temp_dir().join(format!("termenbox-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let signer = Signer::new("k1");
    let server_id = "test-server-id".to_string();
    let auth = Arc::new(AuthState::new(dir.clone(), signer.keys(), server_id.clone()).unwrap());
    let claim = auth.claim_token().unwrap().unwrap();
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag = stopped.clone();
    let mut hub = crate::events::EventHub::new([9; 32])
        .with_tab_status(Arc::new(crate::tabstatus::TabStatus::default()));
    if history_runs > 0 {
        let now = crate::events::now_ms();
        let lines: String = (0..history_runs as u64)
            .map(|i| format!("{{\"session\":\"workspace-opencode\",\"agent\":\"opencode\",\"task\":\"Fix the failing build of the installer on a machine that has no compiler or root access\",\"project\":\"termenbox\",\"started_at\":{},\"ended_at\":{},\"outcome\":\"finished\",\"blocked_ms\":0}}\n", now - 4_000_000 + i * 1000, now - 3_990_000 + i * 1000))
            .collect();
        std::fs::write(dir.join("history.jsonl"), lines).unwrap();
        let limits = crate::history::Limits::new(90, 20);
        hub = hub.with_history(Arc::new(
            crate::history::History::open(&dir, limits, now).unwrap(),
        ));
    }
    let events_socket = dir.join("events.sock");
    tokio::spawn(crate::events::serve_socket(
        hub.clone(),
        events_socket.clone(),
        std::sync::Arc::new(crate::owner::Resolver::default()),
    ));
    let env = crate::events::EventEnv {
        hub: hub.clone(),
        socket: events_socket.clone(),
    };
    let mut state = AppState {
        auth: auth.clone(),
        workspaces: Arc::new(
            WorkspaceManager::new(
                Arc::new(crate::backend::TmuxBackend::default().with_events(env)),
                crate::backend::RunMode::Container,
                Arc::new(crate::registry::Registry::memory()),
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
        paths: Arc::default(),
        conductor: None,
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
    let (rtc, assisted) = rtc::start_assisted(
        Arc::new(socket),
        Ipv4Addr::LOCALHOST.into(),
        cert,
        limits,
        state.clone(),
        None,
    )
    .await
    .unwrap();
    state.rtc_info = Some(rtc.clone());
    let bundle_key = Arc::new(crate::directory::ServerKey::from_seed([5u8; 32]));
    let bundle_key_id = bundle_key.server_id();
    let paths = crate::candidates::PathHub::new(
        dir.clone(),
        bundle_key,
        rtc.fingerprint.clone(),
        "test-vps".into(),
        rtc.port,
        Some("192.168.77.7".into()),
        None,
        None,
        false,
        crate::reach::Handle::idle().0.rx,
    );
    paths.refresh().unwrap();
    let _ = state.paths.set(paths);
    Fixture {
        ws_addr: serve(router(state, false)).await,
        rtc,
        assisted,
        claim,
        signer,
        server_id,
        auth,
        dir,
        stopped,
        hub,
        events_socket,
        bundle_key_id,
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
        self.login_speaking(server_id, who, cert, claim, channels, Some(1))
            .await
    }

    /// The handshake announcing protocol version `protocol`; `None` is a client from before versions existed.
    async fn login_speaking(
        &mut self,
        server_id: &str,
        who: &Who,
        cert: &str,
        claim: &str,
        channels: bool,
        protocol: Option<u32>,
    ) {
        match protocol {
            Some(p) => self.send(json!({"type":"hello","protocol":p})).await,
            None => self.send(json!({"type":"hello"})).await,
        }
        let ch = self.recv_type("challenge").await;
        let nonce = ch["nonce"].as_str().unwrap();
        self.send(json!({
            "type":"auth","cert":cert,"server_token":claim,"channels":channels,
            "signature": who.device.sign(server_id, nonce),
        }))
        .await;
    }

    /// A control message: on its own channel over WebRTC, the one socket over WebSocket.
    async fn send_control(&mut self, v: Value) {
        match self {
            Conn::Ws(_) => self.send(v).await,
            Conn::Rtc(c) => c.send(Frame::Control(v.to_string())),
        }
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

/// What travels on the data channels: text on the status channel, bytes on the `term` channel, control messages on
/// the `ctl` channel.
enum Frame {
    Text(String),
    Term(Vec<u8>),
    Control(String),
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
        let server = SocketAddr::new(info.ip, info.port);
        let ufrag: String = uuid::Uuid::new_v4().simple().to_string();
        let local = IceCreds {
            ufrag: ufrag.clone(),
            pass: ufrag,
        };
        let remote = IceCreds {
            ufrag: SERVER_UFRAG.into(),
            pass: SERVER_PWD.into(),
        };
        RtcClient::dial(socket, local, remote, Some(server), pinned_fingerprint).await
    }

    /// The ICE agent of a browser. With no `server` address it knows nothing of the server and can only answer the
    /// checks the server starts (it learns the server's address from them).
    async fn dial(
        socket: UdpSocket,
        local_creds: IceCreds,
        remote_creds: IceCreds,
        server: Option<SocketAddr>,
        pinned_fingerprint: &str,
    ) -> Result<RtcClient, String> {
        let local = socket.local_addr().unwrap();
        let mut rtc = Rtc::builder().build(Instant::now());
        rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
        if let Some(server) = server {
            rtc.add_remote_candidate(Candidate::host(server, "udp").unwrap());
        }
        let cid: ChannelId;
        let term_cid: ChannelId;
        let ctl_cid: ChannelId;
        {
            let mut d = rtc.direct_api();
            d.set_ice_controlling(true);
            d.set_local_ice_credentials(local_creds);
            d.set_remote_ice_credentials(remote_creds);
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
            ctl_cid = d.create_data_channel(ChannelConfig {
                label: "ctl".into(),
                negotiated: Some(CONTROL_CHANNEL_ID),
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
                                Frame::Control(t) => (ctl_cid, false, t.as_bytes()),
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
        .contains("another user"));
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

/// Kills a test's tmux workspace when the test ends, also when it fails halfway (a failed assertion used to leave
/// `detach…` workspaces behind on the machine that ran the tests).
struct KillOnDrop(String);

/// The id the server gave the workspace whose label is `label`, from a `workspaces` message.
fn id_of(msg: &Value, label: &str) -> String {
    msg["workspaces"]
        .as_array()
        .and_then(|l| l.iter().find(|w| w["name"] == label))
        .and_then(|w| w["id"].as_str())
        .unwrap_or_else(|| panic!("a workspace labelled {label} is listed: {msg}"))
        .to_string()
}

async fn wait_workspace_gone(c: &mut Conn, id: &str) -> Value {
    for _ in 0..50 {
        c.send(json!({"type":"list_workspaces"})).await;
        let list = c.recv_type("workspaces").await;
        if !list["workspaces"]
            .as_array()
            .is_some_and(|all| all.iter().any(|w| w["id"] == id))
        {
            return list;
        }
    }
    panic!("the workspace {id} is still listed");
}

impl Drop for KillOnDrop {
    /// Holds the label the test typed: the session is named by the server, so it is found by its label.
    fn drop(&mut self) {
        let listed = std::process::Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}|#{@termenbox_name}"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        for line in listed.lines() {
            if let Some((id, label)) = line.split_once('|') {
                if label == self.0 || id == self.0 {
                    let _ = std::process::Command::new("tmux")
                        .args(["kill-session", "-t", &format!("={id}")])
                        .output();
                }
            }
        }
    }
}

async fn nothing_is_served_before_auth(kind: Kind) {
    let f = fixture().await;
    let mut c = Conn::open(kind, &f).await;
    c.send(json!({"type":"list_workspaces"})).await;
    assert!(c.recv_type("error").await["message"]
        .as_str()
        .unwrap()
        .contains("Authenticate first"));
}

async fn detach_preserves_the_connection_and_workspace(kind: Kind) {
    let f = fixture().await;
    let name = format!("detach{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"new_workspace","name":name})).await;
    let made = c.recv_type("workspaces").await;
    let label = name;
    let name = id_of(&made, &label);
    c.send(json!({"type":"attach","workspace_id":name})).await;
    c.recv_type("workspace_attached").await;
    c.send(json!({"type":"detach"})).await;
    c.send(json!({"type":"input","data":B64.encode("echo DETACHED_INPUT_MUST_NOT_RUN\n")}))
        .await;
    c.send(json!({"type":"ping"})).await;
    c.recv_type("pong").await;
    c.send(json!({"type":"peek","workspace":name})).await;
    let snapshot = c.recv_type("snapshot").await;
    assert!(!snapshot["text"]
        .as_str()
        .unwrap()
        .contains("DETACHED_INPUT_MUST_NOT_RUN"));
    c.send(json!({"type":"attach","workspace_id":name})).await;
    c.recv_type("workspace_attached").await;
    c.send(json!({"type":"input","data":B64.encode("echo reattached$((40+2))\n")}))
        .await;
    assert!(c.output_contains("reattached42").await);
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

async fn reconnect_resumes_the_tmux_workspace_with_its_screen(kind: Kind) {
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
    c.send(json!({"type":"new_workspace","name":name})).await;
    let workspaces = c.recv_type("workspaces").await;
    let label = name;
    let name = id_of(&workspaces, &label);
    assert!(workspaces["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == name.as_str()));

    c.send(json!({"type":"attach","workspace_id":name})).await;
    c.recv_type("workspace_attached").await;
    c.send(json!({"type":"input","data":B64.encode("echo resume$((40+2))\n")}))
        .await;
    assert!(c.output_contains("resume42").await);

    // The network drops.
    drop(c);

    // The client reconnects without a claim token, lists, and re-attaches: the screen comes back.
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, "").await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"attach","workspace_id":name})).await;
    c.recv_type("workspace_attached").await;
    assert!(
        c.output_contains("resume42").await,
        "earlier output is repainted"
    );

    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

/// A workspace gets its event variables; what a tool writes to the socket reaches a connected client
/// sanitised and in order, and a client that connects later gets the snapshot and the missed events.
async fn agent_events_flow_from_a_workspace_tool_to_the_client(kind: Kind) {
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
    c.send(json!({"type":"new_workspace","name":name})).await;
    let made = c.recv_type("workspaces").await;
    let label = name;
    let name = id_of(&made, &label);
    c.send(json!({"type":"attach","workspace_id":name})).await;
    c.recv_type("workspace_attached").await;
    c.send(json!({"type":"input","data":B64.encode("echo who:$TERMENBOX_SESSION; test -S \"$TERMENBOX_EVENT_SOCKET\" && echo sock:ok\n")})).await;
    assert!(
        c.output_contains(&format!("who:{name}")).await,
        "the workspace knows its name"
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
    assert_eq!(snap["states"][0]["workspace"], name.as_str());
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

/// `peek` returns what is on a workspace's screen without attaching, and says unchanged for a hash it already gave.
async fn peek_shows_a_workspace_screen_without_attaching(kind: Kind) {
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
    c.send(json!({"type":"new_workspace","name":name})).await;
    let made = c.recv_type("workspaces").await;
    let label = name;
    let name = id_of(&made, &label);
    c.send(json!({"type":"reply","workspace":name,"text":"echo peeked$((40+2))"}))
        .await;
    let mut snap = json!(null);
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        c.send(json!({"type":"peek","workspace":name})).await;
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
    c.send(json!({"type":"peek","workspace":name,"hash":hash}))
        .await;
    let again = c.recv_type("snapshot").await;
    assert_eq!(again["unchanged"], true);
    assert_eq!(again["text"], "");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();
}

async fn windows_close_window_and_close_workspace_go_through_the_handlers(kind: Kind) {
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
    // workspace at the wrong moment; creating the workspace is retried when that race loses.
    let mut made = Value::Null;
    for attempt in 0..3 {
        c.send(json!({"type":"new_workspace","name":name})).await;
        let reply = c.recv_any(&["workspaces", "error"]).await;
        if reply["type"] == "workspaces" {
            made = reply;
            break;
        }
        assert!(attempt < 2, "tmux would not start a workspace: {reply}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let label = name;
    let name = id_of(&made, &label);
    c.send(json!({"type":"windows","workspace":name})).await;
    let one = c.recv_type("window_list").await;
    assert_eq!(one["windows"].as_array().unwrap().len(), 1);
    assert!(std::process::Command::new("tmux")
        .args(["new-window", "-t", &format!("={name}:"), "-n", "second"])
        .status()
        .unwrap()
        .success());
    c.send(json!({"type":"windows","workspace":name})).await;
    let two = c.recv_type("window_list").await;
    assert_eq!(two["windows"].as_array().unwrap().len(), 2);
    assert_eq!(two["windows"][1]["name"], "second");
    // A new window opens in the directory of the current one and becomes the current one.
    // Window 1 was just made and may not have settled in its directory yet: ask from window 0.
    c.send(json!({"type":"select_window","workspace":name,"index":0}))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    c.send(json!({"type":"new_window","workspace":name})).await;
    let mut three = serde_json::Value::Null;
    for _ in 0..30 {
        c.send(json!({"type":"windows","workspace":name})).await;
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
    c.send(json!({"type":"close_window","workspace":name,"index":2}))
        .await;
    c.recv_type("workspaces").await;
    c.send(json!({"type":"close_window","workspace":name,"index":1}))
        .await;
    c.recv_type("workspaces").await;
    c.send(json!({"type":"windows","workspace":name})).await;
    assert_eq!(
        c.recv_type("window_list").await["windows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    c.send(json!({"type":"close_workspace","workspace":name}))
        .await;
    let list = c.recv_type("workspaces").await;
    assert!(
        !list["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == name),
        "{list}"
    );
    // A name that is not a workspace is an error, not a crash.
    c.send(json!({"type":"close_workspace","workspace":"no-such-workspace"}))
        .await;
    c.recv_type("error").await;
}

/// A protocol-2 client creates a workspace with its first tab, then adds, renames, focuses, attaches to and removes
/// tabs; removing the last one leaves a shell behind.
async fn tabs_follow_the_registry(kind: Kind) {
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let label = format!("tb{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(label.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login_speaking(
        &f.server_id,
        &alice,
        &alice.cert,
        &f.claim,
        false,
        Some(termenbox_core::ext::PROTOCOL_VERSION),
    )
    .await;
    let ok = c.recv_type("auth_ok").await;
    assert_eq!(ok["tabs"], true, "{ok}");

    let mut made = Value::Null;
    for attempt in 0..3 {
        c.send(json!({"type":"new_workspace","name":label})).await;
        let reply = c.recv_any(&["workspaces", "error"]).await;
        if reply["type"] == "workspaces" {
            made = reply;
            break;
        }
        assert!(attempt < 2, "tmux would not start a workspace: {reply}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let id = id_of(&made, &label);
    let workspace = |list: &Value| -> Value {
        list["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("the workspace is listed: {list}"))
    };
    let one = workspace(&made);
    let tabs = one["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 1, "{one}");
    let first = tabs[0]["id"].as_str().unwrap().to_string();
    assert!(tabs[0]["window"].is_string(), "{one}");

    c.send(
        json!({"type":"new_tab","workspace":id.clone(),"template":"shell","label":"Logs","dir":""}),
    )
    .await;
    let list = c.recv_type("workspaces").await;
    let two = workspace(&list);
    let tabs = two["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 2, "{two}");
    let second = tabs[1]["id"].as_str().unwrap().to_string();
    assert_eq!(tabs[1]["label"], "Logs", "{two}");

    c.send(
        json!({"type":"rename_tab","workspace":id.clone(),"tab":second.clone(),"label":"Build"}),
    )
    .await;
    let list = c.recv_type("workspaces").await;
    let renamed = workspace(&list);
    assert_eq!(renamed["tabs"][1]["label"], "Build", "{renamed}");

    c.send(json!({"type":"focus_tab","workspace":id.clone(),"tab":second.clone()}))
        .await;
    let list = c.recv_type("workspaces").await;
    assert_eq!(workspace(&list)["focus"], second, "{list}");

    c.send(json!({"type":"attach","workspace_id":id.clone(),"tab":second.clone()}))
        .await;
    let attached = c.recv_type("workspace_attached").await;
    assert_eq!(attached["tab"], second, "{attached}");

    // A prompt on a tab that is about to close must not survive the close: no attention/push target and no
    // stale tab_states entry.
    let status = f.hub.tab_status().expect("the test hub tracks tabs");
    status.set_pane("%e2e-close", &id, &first);
    f.hub
        .ingest_as_tab(
            &id,
            &f.hub.token(&id),
            &id,
            Source::Hook,
            Confidence::High,
            vec![
                EventKind::State {
                    state: termenbox_core::events::AgentState::NeedsInput,
                },
                EventKind::Prompt {
                    id: "e2e-close-prompt".into(),
                    prompt_kind: "yes_no".into(),
                    choices: vec!["Yes".into(), "No".into()],
                    decision: None,
                },
            ],
            Some(&first),
        )
        .unwrap();
    assert!(
        f.hub
            .attention_items()
            .iter()
            .any(|a| a.tab.as_deref() == Some(first.as_str())),
        "the pending attention names the tab being closed"
    );

    c.send(json!({"type":"close_tab","workspace":id.clone(),"tab":first.clone()}))
        .await;
    let list = c.recv_type("workspaces").await;
    assert!(
        f.hub
            .attention_items()
            .iter()
            .all(|a| a.tab.as_deref() != Some(first.as_str())),
        "closing a tab leaves no attention for it"
    );
    assert!(
        status
            .snapshot()
            .iter()
            .all(|s| !(s.workspace == id && s.tab == first)),
        "closing a tab leaves no prompt or state for it"
    );
    let left = workspace(&list);
    assert_eq!(left["tabs"].as_array().unwrap().len(), 1, "{left}");

    // The last tab is replaced by a fresh shell tab, so the workspace never becomes empty.
    c.send(json!({"type":"close_tab","workspace":id.clone(),"tab":second.clone()}))
        .await;
    let list = c.recv_type("workspaces").await;
    let replaced = workspace(&list);
    let tabs = replaced["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 1, "{replaced}");
    assert_ne!(tabs[0]["id"], second, "{replaced}");
    assert!(tabs[0]["window"].is_string(), "{replaced}");

    c.send_control(json!({"type":"close_workspace","workspace":id.clone(),"req":99}))
        .await;
    loop {
        let v = c.recv().await.expect("closed");
        if v["type"] == "control_ack" && v["req"] == 99 {
            assert_eq!(v["ok"], true, "{v}");
            break;
        }
    }
    wait_workspace_gone(&mut c, &id).await;
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
    c.send(json!({"type":"new_workspace","name":name})).await;
    let made = c.recv_type("workspaces").await;
    let label = name;
    let name = id_of(&made, &label);
    c.send(json!({"type":"attach","workspace_id":name})).await;
    c.recv_type("workspace_attached").await;
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
    // `yes x | head` is far cheaper than `head /dev/zero | tr` (no per-byte tr process), so the flood
    // starts flowing even on a loaded CI runner where the shell competes for CPU with parallel tests.
    c.send_term(b"yes x | head -c 4000000; echo\n").await;
    let line = json!({"session": name, "token": f.hub.token(&name), "adapter": "claude",
        "data": {"hook_event_name": "Notification", "message": "needs a decision"}})
    .to_string()
        + "\n";
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
    // Connect the events socket here, not earlier: under load the flood-wait above can take long
    // enough for the server's 2 s idle-connection read timeout (events.rs::serve_socket) to close
    // the stream before the write. The real hook (events.rs::run_hook) connects and writes back to
    // back; the test now matches that pattern.
    let mut s = tokio::net::UnixStream::connect(&f.events_socket)
        .await
        .unwrap();
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

/// A hook that opens a question reaches the client as one `status_frame` holding both the workspace's status and
/// the Attention item (decision 0191), and the old `activity` and `attention` messages still come beside it.
async fn a_hook_reaches_the_client_as_one_status_frame(kind: Kind) {
    use tokio::io::AsyncWriteExt;
    let f = fixture().await;
    let name = "framed".to_string();
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"agent_sync","after":{}})).await;
    c.recv_type("status_frame").await;

    let line = json!({"session": name, "token": f.hub.token(&name), "adapter": "claude",
        "data": {"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion",
                 "tool_input": {"questions": [{"question": "Which one?", "options": [{"label": "a"}, {"label": "b"}]}]}}})
    .to_string()
        + "\n";
    // Let what the attach set in motion settle, so the next frame is the hook's.
    while tokio::time::timeout(Duration::from_millis(300), c.recv())
        .await
        .is_ok()
    {}
    let mut s = tokio::net::UnixStream::connect(&f.events_socket)
        .await
        .unwrap();
    s.write_all(line.as_bytes()).await.unwrap();

    let (mut frames, mut old_attention) = (Vec::new(), 0);
    let end = Instant::now() + Duration::from_millis(1500);
    while let Ok(Some(v)) =
        tokio::time::timeout(end.saturating_duration_since(Instant::now()), c.recv()).await
    {
        match v["type"].as_str() {
            Some("status_frame") => frames.push(v),
            Some("attention") if !v["items"].as_array().unwrap().is_empty() => old_attention += 1,
            _ => {}
        }
    }
    assert_eq!(frames.len(), 1, "{frames:?}");
    let frame = &frames[0];
    assert_eq!(frame["v"], 1);
    assert_eq!(frame["workspaces"][0]["activity"], "blocked");
    assert_eq!(frame["attention"][0]["kind"], "needs_input");
    assert_eq!(frame["attention"][0]["workspace"], json!(name));
    assert!(old_attention >= 1, "the old attention message is kept");
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
    c.send(json!({"type":"new_workspace","name":name})).await;
    let made = c.recv_type("workspaces").await;
    let label = name;
    let name = id_of(&made, &label);
    c.send(json!({"type":"attach","workspace_id":name})).await;
    c.recv_type("workspace_attached").await;
    c.send(json!({"type":"input","data":B64.encode("timeout 5 yes x; echo end-$((40+2))\n")}))
        .await;
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

/// The Activity view asks for the history; a server that has run for weeks answers with more than one message of the
/// direct transport holds, and the connection must survive it: the newest runs come, marked as cut, and the next request
/// is answered.
async fn a_long_history_is_cut_to_fit_and_the_connection_survives(kind: Kind) {
    let f = fixture_with(400).await;
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    let now = crate::events::now_ms();
    c.send(json!({"type":"history","from":now - 86_400_000,"to":now + 1000}))
        .await;
    let h = c.recv_type("history").await;
    let runs = h["runs"].as_array().unwrap();
    assert!(!runs.is_empty() && runs.len() < 300, "{} runs", runs.len());
    assert_eq!(h["truncated"], true);
    assert!(
        h.to_string().len() < 64 * 1024,
        "{} bytes",
        h.to_string().len()
    );
    c.send(json!({"type":"list_workspaces"})).await;
    c.recv_type("workspaces").await;
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
    c.send(json!({"type":"new_workspace","name":name})).await;
    let made = c.recv_type("workspaces").await;
    let label = name;
    let name = id_of(&made, &label);
    c.send(json!({"type":"reply","workspace":name,"text":"echo Deploy$((40+2)) now? '(y/n)'"}))
        .await;
    let mut view = json!(null);
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        c.send(json!({"type":"workspace_view","workspace":name}))
            .await;
        view = c.recv_type("workspace_view").await;
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
    c.send(json!({"type":"workspace_view","workspace":name,"hash":hash}))
        .await;
    let again = c.recv_type("workspace_view").await;
    assert_eq!(again["unchanged"], true);
    assert_eq!(again["tail"], json!([]));
    // an old server would skip this; this one must too, and keep serving
    c.send(json!({"type":"a_message_from_the_future","x":1}))
        .await;
    c.send(json!({"type":"ping"})).await;
    c.recv_type("pong").await;
    c.send(
        json!({"type":"workspace_answer","workspace":name,"prompt_id":"v-000000000000","option":0}),
    )
    .await;
    let err = c.recv_type("error").await;
    assert!(
        err["message"].as_str().unwrap().contains("changed"),
        "{err}"
    );
    let id = view["prompt"]["id"].as_str().unwrap();
    c.send(json!({"type":"workspace_answer","workspace":name,"prompt_id":id,"option":9}))
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

/// DECISION-0123: after auth the server pushes its signed candidate bundle; it verifies against the server key and
/// names this server's fingerprint and the LAN address the owner configured.
async fn an_authenticated_client_gets_a_signed_candidate_bundle(kind: Kind) {
    use termenbox_core::bundle::{Bundle, CandidateKind};
    let f = fixture().await;
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    let msg = c.recv_type("candidates").await;
    let b = Bundle::verify(
        msg["bundle"].as_str().unwrap(),
        now(),
        Some(&f.bundle_key_id),
        None,
    )
    .unwrap();
    assert_eq!(b.fingerprint, f.rtc.fingerprint);
    assert!(b
        .candidates
        .iter()
        .any(|c| c.kind == CandidateKind::Lan && c.addr == "192.168.77.7" && c.port == f.rtc.port));
}

/// Version negotiation (step 3.2): a client that announces the current protocol gets it named in `auth_ok`, and one
/// that announces a newer one is answered with ours. Its messages use the workspace vocabulary: closing a workspace
/// that does not exist is answered with an error (an unknown message is ignored without a reply).
async fn a_client_that_announces_the_protocol_speaks_the_workspace_vocabulary(kind: Kind) {
    let f = fixture().await;
    let alice = f.who(1, "alice");
    for (announced, claim) in [(1u32, f.claim.as_str()), (99, "")] {
        let mut c = Conn::open(kind, &f).await;
        c.login_speaking(
            &f.server_id,
            &alice,
            &alice.cert,
            claim,
            false,
            Some(announced),
        )
        .await;
        let ok = c.recv_type("auth_ok").await;
        assert_eq!(
            ok["protocol"],
            announced.min(termenbox_core::ext::PROTOCOL_VERSION)
        );
        c.send(json!({"type":"close_workspace","workspace":"no-such-workspace"}))
            .await;
        c.recv_type("error").await;
    }
}

/// A client from before versions existed (no `protocol` in its hello) is spoken to in the old wire vocabulary, which
/// only `legacy.rs` knows. Its old message names are understood: closing a workspace that does not exist is answered
/// with an error. LEGACY SHIM test: deleted with the shim in step 8.1.
async fn legacy_an_old_clients_messages_are_understood(kind: Kind) {
    let f = fixture().await;
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login_speaking(&f.server_id, &alice, &alice.cert, &f.claim, false, None)
        .await;
    let ok = c.recv_type("auth_ok").await;
    assert_eq!(ok.get("protocol").and_then(Value::as_u64).unwrap_or(0), 0);
    c.send(json!({"type":"close_session","session":"no-such-workspace"}))
        .await;
    c.recv_type("error").await;
}

/// And what it is sent is in the old vocabulary (needs tmux for a workspace to exist). LEGACY SHIM test.
async fn legacy_an_old_client_is_answered_in_the_old_vocabulary(kind: Kind) {
    let f = fixture().await;
    let name = format!("legacy{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login_speaking(&f.server_id, &alice, &alice.cert, &f.claim, false, None)
        .await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"list_sessions"})).await;
    let list = c.recv_type("sessions").await;
    assert!(list["sessions"].is_array(), "{list}");
    assert!(list.get("workspaces").is_none());
    c.send(json!({"type":"new_session","name":name})).await;
    let list = c.recv_type("sessions").await;
    // The old vocabulary lists the same workspaces under `sessions`; the server chose the id.
    let label = name;
    let name = id_of(&json!({"workspaces": list["sessions"]}), &label);
    c.send(json!({"type":"attach","session_id":name})).await;
    let attached = c.recv_type("session_attached").await;
    assert_eq!(attached["session_id"], name.as_str());
    c.send(json!({"type":"windows","session":name})).await;
    let windows = c.recv_type("window_list").await;
    assert_eq!(windows["session"], name.as_str());
    c.send(json!({"type":"close_session","session":name})).await;
    c.recv_type("sessions").await;
}

/// Control messages that carry a `req` are answered with a `control_ack` that says whether they were carried out
/// (step 3.3): on the control channel over WebRTC, ahead of everything queued over WebSocket. No tmux needed.
async fn a_control_message_is_acknowledged_with_its_result(kind: Kind) {
    let f = fixture().await;
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    // Carried out: success.
    c.send_control(json!({"type":"detach","req":7})).await;
    let ack = c.recv_type("control_ack").await;
    assert_eq!(
        (ack["req"].as_u64(), ack["ok"].as_bool()),
        (Some(7), Some(true)),
        "{ack}"
    );
    assert!(ack.get("error").is_none());
    // Refused: the error comes back in the ack, not as a separate error message.
    c.send_control(json!({"type":"close_workspace","workspace":"no-such-workspace","req":8}))
        .await;
    let ack = c.recv_type("control_ack").await;
    assert_eq!(
        (ack["req"].as_u64(), ack["ok"].as_bool()),
        (Some(8), Some(false)),
        "{ack}"
    );
    assert!(
        !ack["error"].as_str().unwrap_or_default().is_empty(),
        "{ack}"
    );
    // A message that is not understood is still answered when it asks for an ack.
    c.send_control(json!({"type":"from_the_future","req":9}))
        .await;
    let ack = c.recv_type("control_ack").await;
    assert_eq!(
        (ack["req"].as_u64(), ack["ok"].as_bool()),
        (Some(9), Some(false)),
        "{ack}"
    );
    // Without `req` nothing changes: a refusal is an error message, and ping is still answered.
    c.send(json!({"type":"close_workspace","workspace":"no-such-workspace"}))
        .await;
    c.recv_type("error").await;
    c.send(json!({"type":"ping"})).await;
    c.recv_type("pong").await;
}

/// The longest a control message may wait to be acknowledged while an attached terminal floods the connection: ten
/// times the baseline measured on CI on 2026-10-25 (54 ms over WebRTC, 53 ms over WebSocket; the flood is `seq` in a
/// loop and the client reads continuously). A regression that queues the ack behind bulk traffic costs seconds.
const CONTROL_ACK_BOUND: Duration = Duration::from_millis(500);

/// A cancel (`detach` stops the stream) sent while the terminal floods the connection is acknowledged within the
/// bound, over both transports (step 3.3, guardrail `control-never-waits-behind-bulk`).
async fn a_cancel_during_an_output_flood_is_acknowledged_within_the_bound(kind: Kind) {
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let f = fixture().await;
    let name = format!("cf{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let _cleanup = KillOnDrop(name.clone());
    let alice = f.who(1, "alice");
    let mut c = Conn::open(kind, &f).await;
    c.login(&f, &alice, &f.claim).await;
    c.recv_type("auth_ok").await;
    c.send(json!({"type":"new_workspace","name":name})).await;
    let made = c.recv_type("workspaces").await;
    let label = name;
    let name = id_of(&made, &label);
    c.send(json!({"type":"attach","workspace_id":name})).await;
    c.recv_type("workspace_attached").await;
    c.send(json!({"type":"input","data":B64.encode("timeout 20 yes x\n")}))
        .await;
    // Wait until the flood is running: enough output to prove it is flowing. The threshold is modest:
    // under load the WebSocket/WebRTC transport backpressures and `yes` cannot sustain MBs in 15 s
    // alongside parallel tests. The flood's volume is not what this test measures; the control_ack
    // timing below is.
    let mut seen = 0usize;
    let start = Instant::now();
    while seen < 50_000 && start.elapsed() < Duration::from_secs(15) {
        let Some(v) = c.recv().await else { break };
        if v["type"] == "output" || v["type"] == "term" {
            seen += v["data"].as_str().map_or(0, str::len);
        }
    }
    assert!(seen >= 50_000, "the flood never started: {seen} bytes");
    let sent = Instant::now();
    c.send_control(json!({"type":"detach","req":42})).await;
    let mut waited = None;
    while sent.elapsed() < Duration::from_secs(15) {
        let Some(v) = c.recv().await else { break };
        if v["type"] == "control_ack" && v["req"] == 42 {
            assert_eq!(v["ok"], true);
            waited = Some(sent.elapsed());
            break;
        }
    }
    let waited = waited.expect("the cancel was never acknowledged");
    eprintln!("control_ack after {waited:?} during a flood ({kind:?})");
    assert!(
        waited < CONTROL_ACK_BOUND,
        "the ack took {waited:?}, bound {CONTROL_ACK_BOUND:?}"
    );
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
    a_control_message_is_acknowledged_with_its_result,
    a_cancel_during_an_output_flood_is_acknowledged_within_the_bound,
    a_client_that_announces_the_protocol_speaks_the_workspace_vocabulary,
    legacy_an_old_clients_messages_are_understood,
    legacy_an_old_client_is_answered_in_the_old_vocabulary,
    a_long_history_is_cut_to_fit_and_the_connection_survives,
    only_the_claiming_github_user_gets_in,
    an_authenticated_client_gets_a_signed_candidate_bundle,
    a_connection_for_another_server_expired_or_revoked_certificates_fail,
    uninstall_needs_the_name_and_a_recent_login_then_wipes_the_config,
    nothing_is_served_before_auth,
    reconnect_resumes_the_tmux_workspace_with_its_screen,
    detach_preserves_the_connection_and_workspace,
    agent_events_flow_from_a_workspace_tool_to_the_client,
    a_burst_of_input_messages_does_not_drop_the_connection,
    peek_shows_a_workspace_screen_without_attaching,
    tabs_follow_the_registry,
    session_view_reads_the_screen_and_unknown_messages_are_ignored,
    windows_close_window_and_close_workspace_go_through_the_handlers,
    terminal_bytes_use_their_own_lane_and_status_is_not_held_up_by_output,
    a_hook_reaches_the_client_as_one_status_frame,
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
    c.send(json!({"type":"list_workspaces"})).await;
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
    c.send(json!({"type":"list_workspaces"})).await;
    c.recv_type("error").await;
}

/// A browser-side offer for an assisted session whose only candidate is `socket`'s address.
fn assisted_offer(socket: &UdpSocket, ufrag: &str) -> rtc::AssistedOffer {
    rtc::AssistedOffer {
        ufrag: ufrag.into(),
        pwd: format!("{ufrag}0123456789abcdef"),
        candidates: vec![socket.local_addr().unwrap()],
        local: vec![],
        relay: None,
    }
}

async fn wait_active(f: &Fixture, want: usize) -> bool {
    for _ in 0..50 {
        if f.assisted.active().await == want {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn assisted_ice_connects_with_checks_started_by_the_server() {
    let f = fixture().await;
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ufrag = uuid::Uuid::new_v4().simple().to_string();
    let offer = assisted_offer(&socket, &ufrag);
    let pwd = offer.pwd.clone();
    let answer = f.assisted.start(offer).await.unwrap();
    assert_eq!(f.assisted.active().await, 1);

    // The client is told nothing of the server's address: the only way through is a check from the server.
    let client = RtcClient::dial(
        socket,
        IceCreds { ufrag, pass: pwd },
        IceCreds {
            ufrag: answer.ufrag.clone(),
            pass: answer.pwd,
        },
        None,
        &f.rtc.fingerprint,
    )
    .await;
    let mut c = Conn::Rtc(client.expect("the assisted session connects"));
    c.send(json!({"type": "list_workspaces"})).await;
    c.recv_type("error").await;

    f.assisted.cancel(&answer.ufrag);
    assert!(wait_active(&f, 0).await, "teardown leaves a session behind");
}

#[tokio::test]
async fn assisted_ice_gives_up_on_a_candidate_that_never_answers() {
    let f = fixture_full(
        0,
        rtc::Limits {
            assisted_ttl: Duration::from_millis(600),
            ..Default::default()
        },
    )
    .await;
    // Bound but never read from, so every check goes unanswered.
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ufrag = uuid::Uuid::new_v4().simple().to_string();
    f.assisted
        .start(assisted_offer(&silent, &ufrag))
        .await
        .unwrap();
    assert_eq!(f.assisted.active().await, 1);
    assert!(
        wait_active(&f, 0).await,
        "an unanswered session is never dropped"
    );
}

#[tokio::test]
async fn assisted_ice_refuses_bad_credentials_and_too_many_sessions() {
    let f = fixture_full(
        0,
        rtc::Limits {
            max_assisted: 1,
            ..Default::default()
        },
    )
    .await;
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut bad = assisted_offer(&silent, "short");
    assert!(f.assisted.start(bad).await.is_err());
    bad = assisted_offer(&silent, &uuid::Uuid::new_v4().simple().to_string());
    f.assisted.start(bad).await.unwrap();
    let second = assisted_offer(&silent, &uuid::Uuid::new_v4().simple().to_string());
    assert!(f.assisted.start(second).await.is_err());
    assert_eq!(f.assisted.active().await, 1);
}

#[tokio::test]
async fn assisted_ice_connects_through_the_relay_with_the_direct_path_blocked() {
    let f = fixture().await;
    let mut fake = crate::turn::testing::fake().await;
    let relay = crate::turn::allocate(
        &crate::turn::testing::credentials(&fake, crate::turn::testing::PASS),
        f.assisted.relay_inbox(),
    )
    .await
    .unwrap();
    let relay_addr = relay.addr();

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ufrag = uuid::Uuid::new_v4().simple().to_string();
    let mut offer = assisted_offer(&socket, &ufrag);
    // The server is told only a dead port on the client's IP: its direct checks go nowhere, and the relay still learns
    // the client's IP for the permission. The client is told only the relayed address, never the server's own.
    let dead = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    offer.candidates = vec![dead.local_addr().unwrap()];
    offer.relay = Some(relay);
    let pwd = offer.pwd.clone();
    let answer = f.assisted.start(offer).await.unwrap();

    let client = RtcClient::dial(
        socket,
        IceCreds { ufrag, pass: pwd },
        IceCreds {
            ufrag: answer.ufrag.clone(),
            pass: answer.pwd,
        },
        Some(relay_addr),
        &f.rtc.fingerprint,
    )
    .await;
    let mut c = Conn::Rtc(client.expect("the session connects through the relay"));
    c.send(json!({"type": "list_workspaces"})).await;
    c.recv_type("error").await;

    // Ending the session releases the allocation.
    f.assisted.cancel(&answer.ufrag);
    tokio::time::timeout(Duration::from_secs(3), fake.closed.recv())
        .await
        .expect("the allocation is released with the session")
        .unwrap();
}

#[tokio::test]
async fn assisted_ice_without_a_relay_still_connects_when_the_allocation_is_refused() {
    let f = fixture().await;
    let fake = crate::turn::testing::fake().await;
    let relay = crate::turn::allocate_or_none(
        &crate::turn::testing::credentials(&fake, "not-the-password"),
        f.assisted.relay_inbox(),
    )
    .await;
    assert!(relay.is_none());

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ufrag = uuid::Uuid::new_v4().simple().to_string();
    let mut offer = assisted_offer(&socket, &ufrag);
    offer.relay = relay;
    let pwd = offer.pwd.clone();
    let answer = f.assisted.start(offer).await.unwrap();
    let client = RtcClient::dial(
        socket,
        IceCreds { ufrag, pass: pwd },
        IceCreds {
            ufrag: answer.ufrag,
            pass: answer.pwd,
        },
        None,
        &f.rtc.fingerprint,
    )
    .await;
    assert!(client.is_ok());
}

/// The server's signaling handler over the fixture's real transport, a fake Cloudflare and a fake TURN server.
async fn signal_rig(
    f: &Fixture,
    turn: Option<&crate::turn::testing::Fake>,
) -> (
    crate::signal::Signal,
    Arc<crate::signal::testing::FakeCloudflare>,
) {
    use crate::signal::testing::*;
    let servers = match turn {
        Some(t) => json!({
            "urls": [format!("turn:{}?transport=udp", t.addr)],
            "username": crate::turn::testing::USER,
            "credential": crate::turn::testing::PASS,
        }),
        None => secret_servers(),
    };
    let cf = Arc::new(cloudflare(servers).await);
    let signal = crate::signal::Signal::new(
        f.auth.clone(),
        f.assisted.clone(),
        crate::signal::Endpoint {
            host: SocketAddr::new(f.rtc.ip, f.rtc.port),
            mapped: None,
        },
        Some(crate::signal::Minter::new(&cf.base, KEY_ID, TOKEN)),
        Arc::new(crate::signal::Usage::open(&f.dir, 500)),
    );
    (signal, cf)
}

fn reply_json(r: Option<Vec<u8>>) -> Value {
    serde_json::from_slice(&r.expect("a reply")).unwrap()
}

#[tokio::test]
async fn signal_offer_starts_a_session_the_client_connects_to() {
    use crate::signal::testing::signed;
    let f = fixture().await;
    let (signal, _cf) = signal_rig(&f, None).await;
    let who = f.who(1, "alice");
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ufrag = uuid::Uuid::new_v4().simple().to_string();
    let pwd = format!("{ufrag}0123456789abcdef");
    let offer = signed(
        &who.device,
        &who.cert,
        &f.server_id,
        json!({"t": "offer", "ufrag": ufrag, "pwd": pwd,
               "cands": [socket.local_addr().unwrap().to_string()], "claim": f.claim}),
    );
    let answer = reply_json(signal.handle(3, &offer).await);
    assert_eq!(answer["t"], "answer", "{answer}");
    assert_eq!(f.assisted.active().await, 1);

    // A signed candidate for that session is taken; the same for a session this client did not start is not.
    let session = answer["ufrag"].as_str().unwrap().to_string();
    let extra = signed(
        &who.device,
        &who.cert,
        &f.server_id,
        json!({"t": "cand", "session": session, "addr": "127.0.0.1:9"}),
    );
    assert_eq!(reply_json(signal.handle(3, &extra).await)["t"], "ok");
    let theirs = signed(
        &who.device,
        &who.cert,
        &f.server_id,
        json!({"t": "cand", "session": session, "addr": "127.0.0.1:9"}),
    );
    assert_eq!(reply_json(signal.handle(4, &theirs).await)["t"], "error");

    let client = RtcClient::dial(
        socket,
        IceCreds { ufrag, pass: pwd },
        IceCreds {
            ufrag: session,
            pass: answer["pwd"].as_str().unwrap().into(),
        },
        None,
        &f.rtc.fingerprint,
    )
    .await;
    let mut c = Conn::Rtc(client.expect("connects"));
    c.send(json!({"type": "list_workspaces"})).await;
    c.recv_type("error").await;
}

#[tokio::test]
async fn signal_offer_that_wants_a_relay_gets_one_and_a_stranger_does_not_start_a_session() {
    use crate::signal::testing::signed;
    let f = fixture().await;
    let mut turn = crate::turn::testing::fake().await;
    let (signal, cf) = signal_rig(&f, Some(&turn)).await;
    let who = f.who(1, "alice");
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ufrag = uuid::Uuid::new_v4().simple().to_string();
    let pwd = format!("{ufrag}0123456789abcdef");
    let fields = |claim: &str| {
        json!({"t": "offer", "ufrag": ufrag, "pwd": pwd, "relay": true,
               "cands": [socket.local_addr().unwrap().to_string()], "claim": claim})
    };

    // An unauthenticated offer starts nothing and costs nothing.
    let stranger = f.who(2, "mallory");
    let bad = signed(
        &stranger.device,
        &stranger.cert,
        &f.server_id,
        fields("wrong"),
    );
    assert_eq!(reply_json(signal.handle(1, &bad).await)["t"], "error");
    assert_eq!(f.assisted.active().await, 0);
    assert_eq!(cf.calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let good = signed(&who.device, &who.cert, &f.server_id, fields(&f.claim));
    let answer = reply_json(signal.handle(1, &good).await);
    assert_eq!(answer["t"], "answer", "{answer}");
    let cands: Vec<String> = answer["cands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().into())
        .collect();
    assert_eq!(
        cands.len(),
        2,
        "the host address and the relayed one: {cands:?}"
    );
    assert_eq!(cf.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    // The relay is the server's own allocation and goes with the session.
    f.assisted.cancel(answer["ufrag"].as_str().unwrap());
    tokio::time::timeout(Duration::from_secs(3), turn.closed.recv())
        .await
        .expect("the allocation is released")
        .unwrap();
}
