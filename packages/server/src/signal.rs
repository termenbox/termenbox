//! The server's side of the assisted connection's signaling (DECISION-0140, step 2.5). The server dials out to the
//! owner's Worker (`infra/owner-relay`) and answers what clients send through it. The Worker is only a switchboard:
//! it holds no secret and checks nothing, so every message is verified here (GUARDRAIL
//! relay-credentials-are-owner-scoped-minted-after-auth-and-capped).
//!
//! A frame from the Worker is `[client id][JSON]`; the answer goes back the same way. A client message is
//! `{"t": "offer" | "cand" | "relay", "n": nonce, "exp": unix seconds, "cert", "sig", ...}` and `sig` is the device
//! key's signature (the same challenge the WebSocket and data-channel handshakes use, `cert::challenge_message`) over
//! `"<t>.<n>.<exp>.<digest>"`, where `digest` is the unpadded base64url SHA-256 of the message's fields (`body`). So a
//! signature covers the kind, the freshness and the content; one nonce is accepted once.
//!
//! * `offer`: the client's ICE credentials and candidates. The answer carries the server's ICE credentials and
//!   candidates, and the assisted session (`rtc::Assisted`) starts checking toward the client.
//! * `cand`: one more client candidate for the session that client started.
//! * `relay`: the client asks for TURN credentials. They are minted on the owner's Cloudflare key with a short TTL,
//!   only after the checks above, and never while the month's relayed bytes are at the cap.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use str0m::Candidate;
use tokio::sync::{mpsc, watch, Semaphore};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use crate::auth::AuthState;
use crate::rtc::{Assisted, AssistedOffer};
use crate::stun::Mapped;
use crate::turn;

/// Cloudflare's credential endpoint (FACT-0058); tests point `Minter` somewhere else.
pub const CLOUDFLARE_API: &str = "https://rtc.live.cloudflare.com";
/// A minted credential lives this long.
const CREDENTIAL_TTL: u64 = 15 * 60;
/// A message may claim to expire at most this far ahead; a longer reach is refused as a replay window.
const MAX_AHEAD: u64 = 60;
/// What each minted credential is counted as against the monthly cap: an estimate of what one 15-minute terminal
/// session relays, since the credential is the last point the server controls.
pub const RESERVE_PER_CREDENTIAL: u64 = 128 * 1024 * 1024;
pub const DEFAULT_CAP_GB: u64 = 500;
const MAX_SEEN: usize = 4096;
const MAX_CANDIDATES: usize = 16;
const MAX_IN_FLIGHT: usize = 8;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// --- configuration -------------------------------------------------------------------------------------------------

/// What `--setup-cloudflare` (step 2.7) leaves in the config directory: the Worker's `wss://` URL (with the server's
/// route) and the TURN key id with the token that mints credentials from it. Nothing else of the owner's account.
#[derive(Clone, Deserialize)]
pub struct Config {
    pub signal: String,
    pub turn_key_id: String,
    pub turn_key_token: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("signal", &self.signal)
            .finish_non_exhaustive()
    }
}

pub const CONFIG_FILE: &str = "relay.json";

/// `http(s)` or `ws(s)` URL whose host is this machine: the only plaintext a test override may name.
fn is_loopback_url(url: &str, scheme: &str) -> bool {
    let Some(rest) = url.strip_prefix(scheme) else {
        return false;
    };
    ["127.0.0.1", "localhost", "[::1]"].iter().any(|h| {
        rest.strip_prefix(h)
            .is_some_and(|r| r.is_empty() || r.starts_with([':', '/']))
    })
}

/// Where the server dials the Worker: `TERMENBOX_SIGNAL_DIAL` when it is a loopback `ws://` (the end-to-end test's
/// fake Worker, which has no certificate the server trusts), otherwise the configured `wss://` URL. The bundle
/// always advertises the configured URL, so the override changes nothing a client or the directory sees.
pub fn dial_url(configured: &str, over: Option<&str>) -> String {
    match over {
        Some(u) if is_loopback_url(u, "ws://") => u.to_string(),
        Some(_) => {
            warn!("TERMENBOX_SIGNAL_DIAL ignored: only a loopback ws:// URL is accepted");
            configured.to_string()
        }
        None => configured.to_string(),
    }
}

/// The credential endpoint: `TERMENBOX_RELAY_API` when it is `https://` or a loopback `http://` (a fake in the
/// end-to-end test), otherwise Cloudflare's.
pub fn credential_api(over: Option<&str>) -> String {
    match over {
        Some(u) if u.starts_with("https://") || is_loopback_url(u, "http://") => u.to_string(),
        Some(_) => {
            warn!(
                "TERMENBOX_RELAY_API ignored: only https:// or a loopback http:// URL is accepted"
            );
            CLOUDFLARE_API.to_string()
        }
        None => CLOUDFLARE_API.to_string(),
    }
}

impl Config {
    /// `None` when the owner has not set the assisted path up.
    pub fn load(dir: &Path) -> Result<Option<Config>> {
        let path = dir.join(CONFIG_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)?.permissions().mode();
            if mode & 0o077 != 0 {
                return Err(anyhow!(
                    "{} holds a token and must be readable by its owner only (chmod 600)",
                    path.display()
                ));
            }
        }
        let c: Config = serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid", path.display()))?;
        if !c.signal.starts_with("wss://") {
            return Err(anyhow!("the signaling URL must be wss://"));
        }
        Ok(Some(c))
    }
}

// --- the monthly relayed-bytes counter ----------------------------------------------------------------------------

/// "YYYY-MM" of a unix time, UTC.
pub fn month_of(secs: u64) -> String {
    // Howard Hinnant's civil-from-days.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

#[derive(Serialize, Deserialize, Default)]
struct UsageFile {
    month: String,
    bytes: u64,
}

/// Bytes charged to the owner's Cloudflare allowance this month, kept in the config directory so a restart does not
/// reset the cap. The cap is the guardrail's: at it, nothing more is minted.
pub struct Usage {
    path: PathBuf,
    cap: u64,
    state: Mutex<UsageFile>,
}

impl Usage {
    pub fn open(dir: &Path, cap_gb: u64) -> Usage {
        let path = dir.join("relay_usage.json");
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Usage {
            path,
            cap: cap_gb.saturating_mul(1_000_000_000),
            state: Mutex::new(state),
        }
    }

    /// `TERMENBOX_RELAY_CAP_GB`, or the default.
    pub fn cap_from_env(value: Option<&str>) -> u64 {
        value
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(DEFAULT_CAP_GB)
    }

    fn roll(&self, st: &mut UsageFile, now: u64) {
        let month = month_of(now);
        if st.month != month {
            *st = UsageFile { month, bytes: 0 };
        }
    }

    pub fn at_cap_at(&self, now: u64) -> bool {
        let mut st = self.state.lock().unwrap();
        self.roll(&mut st, now);
        st.bytes >= self.cap
    }

    pub fn at_cap(&self) -> bool {
        self.at_cap_at(now_secs())
    }

    pub fn charge_at(&self, bytes: u64, now: u64) {
        let mut st = self.state.lock().unwrap();
        self.roll(&mut st, now);
        st.bytes = st.bytes.saturating_add(bytes);
        let text = serde_json::to_string(&*st).unwrap_or_default();
        if let Err(e) = crate::auth::write_private(&self.path, text.as_bytes()) {
            warn!("cannot save the relay counter: {e}");
        }
    }

    pub fn charge(&self, bytes: u64) {
        self.charge_at(bytes, now_secs());
    }

    /// One line for the status output.
    pub fn status_line(&self) -> String {
        let mut st = self.state.lock().unwrap();
        self.roll(&mut st, now_secs());
        let gb = |b: u64| b as f64 / 1e9;
        if st.bytes >= self.cap {
            format!(
                "Relay: unavailable, the monthly cap of {:.0} GB is reached ({})",
                gb(self.cap),
                st.month
            )
        } else {
            format!(
                "Relay: {:.1} of {:.0} GB used this month ({})",
                gb(st.bytes),
                gb(self.cap),
                st.month
            )
        }
    }
}

// --- minting credentials ------------------------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

impl std::fmt::Debug for IceServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IceServer")
            .field("urls", &self.urls)
            .finish_non_exhaustive()
    }
}

impl IceServer {
    /// `host:port` of the first UDP TURN url, for the server's own allocation.
    pub fn turn_udp(&self) -> Option<String> {
        self.urls.iter().find_map(|u| {
            let rest = u.strip_prefix("turn:")?;
            let (hostport, query) = rest.split_once('?').unwrap_or((rest, ""));
            (!query.contains("transport=tcp")).then(|| hostport.to_string())
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MintError {
    /// The monthly cap is reached: no call was made.
    Unavailable,
    Failed,
}

pub struct Minter {
    http: reqwest::Client,
    api: String,
    key_id: String,
    token: String,
}

impl Minter {
    pub fn new(api: &str, key_id: &str, token: &str) -> Minter {
        Minter {
            // Cloudflare answers 403 (error 1010) to a client without a User-Agent (FACT-0059).
            http: reqwest::Client::builder()
                .user_agent(concat!("termenbox-server/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            api: api.trim_end_matches('/').to_string(),
            key_id: key_id.into(),
            token: token.into(),
        }
    }

    async fn call(&self) -> Result<IceServer> {
        let url = format!(
            "{}/v1/turn/keys/{}/credentials/generate",
            self.api, self.key_id
        );
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "ttl": CREDENTIAL_TTL }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "the credential endpoint answered {}",
                resp.status()
            ));
        }
        #[derive(Deserialize)]
        struct Body {
            #[serde(rename = "iceServers")]
            ice_servers: serde_json::Value,
        }
        let body: Body = resp.json().await?;
        let one = match body.ice_servers {
            serde_json::Value::Array(mut a) if !a.is_empty() => a.remove(0),
            other => other,
        };
        Ok(serde_json::from_value(one)?)
    }
}

// --- the message handler ------------------------------------------------------------------------------------------

/// What the server tells a client about itself.
pub struct Endpoint {
    /// The transport socket's address.
    pub host: SocketAddr,
    /// The STUN-mapped address (step 2.2), when there is one.
    pub mapped: Option<watch::Receiver<Option<Mapped>>>,
}

impl Endpoint {
    fn mapped(&self) -> Option<SocketAddr> {
        self.mapped.as_ref()?.borrow().as_ref().map(|m| m.addr)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Msg {
    t: String,
    n: String,
    exp: u64,
    cert: String,
    sig: String,
    claim: Option<String>,
    ufrag: String,
    pwd: String,
    cands: Vec<String>,
    relay: bool,
    session: String,
    addr: String,
}

impl Msg {
    /// The fields the signature covers, in a fixed order.
    fn body(&self) -> String {
        match self.t.as_str() {
            "offer" => format!(
                "{}\n{}\n{}\n{}",
                self.ufrag,
                self.pwd,
                self.relay,
                self.cands.join(",")
            ),
            "cand" => format!("{}\n{}", self.session, self.addr),
            _ => String::new(),
        }
    }

    /// The string the device signs.
    fn signed(&self) -> String {
        let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(self.body().as_bytes()));
        format!("{}.{}.{}.{}", self.t, self.n, self.exp, digest)
    }
}

fn valid_nonce(n: &str) -> bool {
    (16..=64).contains(&n.len())
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub struct Signal {
    auth: Arc<AuthState>,
    assisted: Assisted,
    endpoint: Endpoint,
    minter: Option<Minter>,
    usage: Arc<Usage>,
    /// Nonces accepted so far, with the time they stop mattering.
    seen: Mutex<HashMap<String, u64>>,
    /// The session each Worker client id started, so a `cand` can only extend its own.
    sessions: Mutex<HashMap<u8, String>>,
}

impl Signal {
    pub fn new(
        auth: Arc<AuthState>,
        assisted: Assisted,
        endpoint: Endpoint,
        minter: Option<Minter>,
        usage: Arc<Usage>,
    ) -> Signal {
        Signal {
            auth,
            assisted,
            endpoint,
            minter,
            usage,
            seen: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Answers one client message. `None` means it was ignored: malformed, expired or replayed messages get no reply,
    /// so a flood costs the sender nothing it can read.
    pub async fn handle(&self, client: u8, payload: &[u8]) -> Option<Vec<u8>> {
        let msg: Msg = serde_json::from_slice(payload).ok()?;
        let now = now_secs();
        if !valid_nonce(&msg.n) || msg.exp < now || msg.exp > now + MAX_AHEAD {
            return None;
        }
        if self.seen.lock().unwrap().contains_key(&msg.n) {
            return None;
        }
        let who = self.auth.check(
            &msg.cert,
            &msg.sig,
            &msg.signed(),
            msg.claim.as_deref(),
            None,
        );
        if let Err(e) = who {
            return Some(reply(
                &serde_json::json!({"t": "error", "reason": e.message()}),
            ));
        }
        {
            let mut seen = self.seen.lock().unwrap();
            seen.retain(|_, exp| *exp >= now);
            if seen.len() >= MAX_SEEN {
                return None;
            }
            seen.insert(msg.n.clone(), msg.exp);
        }
        let out = match msg.t.as_str() {
            "offer" => self.offer(client, &msg).await,
            "cand" => self.candidate(client, &msg),
            "relay" => self.relay(&msg).await,
            _ => return None,
        };
        Some(reply(&out))
    }

    async fn offer(&self, client: u8, m: &Msg) -> serde_json::Value {
        let error = |why: &str| serde_json::json!({"t": "error", "reason": why});
        if m.cands.len() > MAX_CANDIDATES {
            return error("too many candidates");
        }
        let candidates: Vec<SocketAddr> = m.cands.iter().filter_map(|c| c.parse().ok()).collect();
        let relay = if m.relay {
            self.own_relay().await
        } else {
            None
        };
        let mut local = Vec::new();
        let mut cands = vec![self.endpoint.host];
        if let Some(mapped) = self.endpoint.mapped() {
            if let Ok(c) = Candidate::server_reflexive(mapped, self.endpoint.host, "udp") {
                local.push(c);
                cands.push(mapped);
            }
        }
        if let Some(r) = &relay {
            cands.push(r.addr());
        }
        let started = self
            .assisted
            .start(AssistedOffer {
                ufrag: m.ufrag.clone(),
                pwd: m.pwd.clone(),
                candidates,
                local,
                relay,
            })
            .await;
        match started {
            Ok(a) => {
                self.sessions
                    .lock()
                    .unwrap()
                    .insert(client, a.ufrag.clone());
                serde_json::json!({
                    "t": "answer",
                    "ufrag": a.ufrag,
                    "pwd": a.pwd,
                    "cands": cands.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                })
            }
            Err(e) => {
                debug!("assisted offer refused: {e}");
                error("offer refused")
            }
        }
    }

    fn candidate(&self, client: u8, m: &Msg) -> serde_json::Value {
        let own = self.sessions.lock().unwrap().get(&client).cloned();
        match (own, m.addr.parse::<SocketAddr>()) {
            (Some(s), Ok(addr)) if s == m.session => {
                self.assisted.candidate(&s, addr);
                serde_json::json!({"t": "ok"})
            }
            _ => serde_json::json!({"t": "error", "reason": "no such session"}),
        }
    }

    async fn relay(&self, _m: &Msg) -> serde_json::Value {
        match self.mint().await {
            Ok(server) => serde_json::json!({"t": "ice", "servers": [server]}),
            Err(MintError::Unavailable) => {
                serde_json::json!({"t": "error", "reason": "relay unavailable"})
            }
            Err(MintError::Failed) => {
                serde_json::json!({"t": "error", "reason": "relay failed"})
            }
        }
    }

    /// Mints one credential, unless the month's cap is reached (then no API call is made).
    async fn mint(&self) -> Result<IceServer, MintError> {
        let minter = self.minter.as_ref().ok_or(MintError::Unavailable)?;
        if self.usage.at_cap() {
            return Err(MintError::Unavailable);
        }
        match minter.call().await {
            Ok(server) => {
                self.usage.charge(RESERVE_PER_CREDENTIAL);
                Ok(server)
            }
            Err(e) => {
                // The error text names the endpoint's status, never a credential.
                warn!("minting a relay credential failed: {e:#}");
                Err(MintError::Failed)
            }
        }
    }

    /// The server's own allocation, for sessions whose client asked for a relay.
    async fn own_relay(&self) -> Option<turn::Relay> {
        let server = self.mint().await.ok()?;
        let creds = turn::Credentials {
            server: server.turn_udp()?,
            username: server.username,
            password: server.credential,
        };
        turn::allocate_or_none(&creds, self.assisted.relay_inbox()).await
    }
}

fn reply(v: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_default()
}

// --- the connection to the Worker --------------------------------------------------------------------------------

/// Doubling waits between attempts to reach the Worker, up to a ceiling; a connection that held resets it.
pub struct Backoff {
    base: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    pub fn new(base: Duration, max: Duration) -> Backoff {
        Backoff {
            base,
            max,
            next: base,
        }
    }

    pub fn next(&mut self) -> Duration {
        let now = self.next;
        self.next = (self.next * 2).min(self.max);
        now
    }

    pub fn reset(&mut self) {
        self.next = self.base;
    }
}

/// A connection that stayed up this long counts as healthy.
const HEALTHY: Duration = Duration::from_secs(30);

/// Keeps one outbound WebSocket to the Worker for as long as the process runs.
pub async fn run(url: String, signal: Arc<Signal>, mut backoff: Backoff) {
    let url = format!("{url}?role=server");
    loop {
        let started = Instant::now();
        match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok((ws, _)) => {
                info!("signaling connected");
                serve(ws, &signal).await;
                if started.elapsed() >= HEALTHY {
                    backoff.reset();
                }
            }
            Err(e) => debug!("signaling connect failed: {e}"),
        }
        tokio::time::sleep(backoff.next()).await;
    }
}

async fn serve<S>(ws: tokio_tungstenite::WebSocketStream<S>, signal: &Arc<Signal>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut tx, mut rx) = ws.split();
    let (out, mut replies) = mpsc::unbounded_channel::<Vec<u8>>();
    let limit = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    loop {
        tokio::select! {
            m = rx.next() => {
                let data = match m {
                    Some(Ok(Message::Binary(b))) => b,
                    Some(Ok(Message::Text(t))) => t.into_bytes(),
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                    Some(Ok(_)) => continue,
                };
                let Some((&id, payload)) = data.split_first() else { continue };
                let Ok(permit) = limit.clone().try_acquire_owned() else { continue };
                let (signal, out, payload) = (signal.clone(), out.clone(), payload.to_vec());
                tokio::spawn(async move {
                    if let Some(r) = signal.handle(id, &payload).await {
                        let mut frame = vec![id];
                        frame.extend(r);
                        let _ = out.send(frame);
                    }
                    drop(permit);
                });
            }
            Some(frame) = replies.recv() => {
                if tx.send(Message::Binary(frame)).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
pub mod testing {
    //! A fake of Cloudflare's credential endpoint and a signer for client messages.
    use super::*;
    use crate::cert::testing::Device;
    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub const KEY_ID: &str = "key-1";
    pub const TOKEN: &str = "TURNKEYTOKEN-not-for-logs";

    pub struct FakeCloudflare {
        pub base: String,
        pub calls: Arc<AtomicUsize>,
    }

    #[derive(Clone)]
    struct Fake {
        calls: Arc<AtomicUsize>,
        servers: serde_json::Value,
    }

    /// `servers` is the `iceServers` value it hands out.
    pub async fn cloudflare(servers: serde_json::Value) -> FakeCloudflare {
        let calls = Arc::new(AtomicUsize::new(0));
        let fake = Fake {
            calls: calls.clone(),
            servers,
        };
        async fn generate(
            State(f): State<Fake>,
            headers: HeaderMap,
        ) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
            f.calls.fetch_add(1, Ordering::SeqCst);
            let ok = headers
                .get("authorization")
                .is_some_and(|v| v == format!("Bearer {TOKEN}").as_str())
                && headers.contains_key("user-agent");
            if !ok {
                return Err(axum::http::StatusCode::FORBIDDEN);
            }
            Ok(Json(serde_json::json!({ "iceServers": f.servers })))
        }
        let app = Router::new()
            .route("/v1/turn/keys/:id/credentials/generate", post(generate))
            .with_state(fake);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeCloudflare { base, calls }
    }

    pub fn secret_servers() -> serde_json::Value {
        serde_json::json!({
            "urls": ["stun:stun.example:3478", "turn:turn.example:3478?transport=udp"],
            "username": "SECRETUSER-123",
            "credential": "SECRETCRED-456"
        })
    }

    /// A client message signed by `device`, as the browser will sign it.
    pub fn signed(
        device: &Device,
        cert: &str,
        server_id: &str,
        mut fields: serde_json::Value,
    ) -> Vec<u8> {
        let o = fields.as_object_mut().unwrap();
        o.entry("n").or_insert_with(|| {
            URL_SAFE_NO_PAD
                .encode(uuid::Uuid::new_v4().as_bytes())
                .into()
        });
        o.entry("exp").or_insert((now_secs() + 30).into());
        o.insert("cert".into(), cert.into());
        let msg: Msg = serde_json::from_value(serde_json::Value::Object(o.clone())).unwrap();
        o.insert("sig".into(), device.sign(server_id, &msg.signed()).into());
        serde_json::to_vec(&fields).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::cert::testing::{now, Device, Signer};
    use std::sync::atomic::Ordering;

    const SERVER_ID: &str = "signal-test-server";

    struct Rig {
        signal: Signal,
        device: Device,
        cert: String,
        claim: String,
        calls: Arc<std::sync::atomic::AtomicUsize>,
        usage: Arc<Usage>,
        _dir: PathBuf,
    }

    async fn rig(cap_gb: u64) -> Rig {
        let dir = std::env::temp_dir().join(format!("termenbox-signal-{}", uuid::Uuid::new_v4()));
        let signer = Signer::new("k1");
        let auth = Arc::new(AuthState::new(dir.clone(), signer.keys(), SERVER_ID.into()).unwrap());
        let claim = auth.claim_token().unwrap().unwrap();
        let device = Device::new();
        let cert = signer.issue(&device, 1, "alice", now() + 3600);
        let cf = cloudflare(secret_servers()).await;
        let usage = Arc::new(Usage::open(&dir, cap_gb));
        let signal = Signal::new(
            auth,
            Assisted::detached(),
            Endpoint {
                host: "127.0.0.1:4443".parse().unwrap(),
                mapped: None,
            },
            Some(Minter::new(&cf.base, KEY_ID, TOKEN)),
            usage.clone(),
        );
        Rig {
            signal,
            device,
            cert,
            claim,
            calls: cf.calls,
            usage,
            _dir: dir,
        }
    }

    impl Rig {
        fn relay_request(&self, extra: serde_json::Value) -> Vec<u8> {
            let mut f = serde_json::json!({"t": "relay", "claim": self.claim});
            f.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            signed(&self.device, &self.cert, SERVER_ID, f)
        }
    }

    fn text(r: Option<Vec<u8>>) -> String {
        String::from_utf8(r.expect("a reply")).unwrap()
    }

    #[tokio::test]
    async fn signal_months_are_utc_calendar_months() {
        assert_eq!(month_of(0), "1970-01");
        assert_eq!(month_of(951_782_400), "2000-02");
        assert_eq!(month_of(1_700_000_000), "2023-11");
        assert_eq!(month_of(1_704_067_199), "2023-12");
        assert_eq!(month_of(1_704_067_200), "2024-01");
    }

    #[tokio::test]
    async fn no_credential_without_device_cert() {
        let r = rig(500).await;
        // No certificate at all, a certificate from an unknown signer, and a good certificate with a wrong signature.
        let stranger = Signer::new("k1");
        let other = Device::new();
        let forged = stranger.issue(&other, 1, "mallory", now() + 3600);
        let blank = signed(&r.device, "", SERVER_ID, serde_json::json!({"t": "relay"}));
        let unknown = signed(
            &other,
            &forged,
            SERVER_ID,
            serde_json::json!({"t": "relay"}),
        );
        let wrong_key = signed(
            &other,
            &r.cert,
            SERVER_ID,
            serde_json::json!({"t": "relay"}),
        );
        for m in [blank, unknown, wrong_key] {
            let reply = text(r.signal.handle(1, &m).await);
            assert!(
                reply.contains("error") && !reply.contains("SECRET"),
                "{reply}"
            );
        }
        assert_eq!(r.calls.load(Ordering::SeqCst), 0, "no API call was made");
        // The real device, for contrast.
        let ok = text(
            r.signal
                .handle(1, &r.relay_request(serde_json::json!({})))
                .await,
        );
        assert!(ok.contains("ice"), "{ok}");
        assert_eq!(r.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cap_stops_minting() {
        let r = rig(1).await;
        r.usage.charge(1_000_000_000);
        let reply = text(
            r.signal
                .handle(1, &r.relay_request(serde_json::json!({})))
                .await,
        );
        assert!(reply.contains("relay unavailable"), "{reply}");
        assert_eq!(r.calls.load(Ordering::SeqCst), 0, "no API call at the cap");
        assert!(r.usage.status_line().contains("unavailable"));
        // The count belongs to a month: the next one starts empty.
        assert!(!r.usage.at_cap_at(now() + 40 * 86_400));
    }

    #[tokio::test]
    async fn signal_each_minted_credential_counts_against_the_cap() {
        let r = rig(500).await;
        r.signal
            .handle(1, &r.relay_request(serde_json::json!({})))
            .await;
        assert!(r.usage.status_line().contains("0.1 of 500 GB"));
        // A restart keeps the count.
        let again = Usage::open(r.signal.auth.dir(), 500);
        assert!(again.status_line().contains("0.1 of 500 GB"));
    }

    #[tokio::test]
    async fn credential_never_in_bundle_or_logs() {
        use std::io::Write;
        #[derive(Clone)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let logs = Buf(Arc::default());
        let sink = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || sink.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let r = rig(500).await;
        // A success, a failure (a wrong token) and a refused offer all pass through the logging.
        let ok = text(
            r.signal
                .handle(1, &r.relay_request(serde_json::json!({})))
                .await,
        );
        assert!(ok.contains("SECRETCRED-456"), "the client does get it");
        let broken = Signal::new(
            r.signal.auth.clone(),
            Assisted::detached(),
            Endpoint {
                host: "127.0.0.1:4443".parse().unwrap(),
                mapped: None,
            },
            Some(Minter::new("http://127.0.0.1:1", KEY_ID, TOKEN)),
            r.usage.clone(),
        );
        broken
            .handle(1, &r.relay_request(serde_json::json!({})))
            .await;
        let offer = signed(
            &r.device,
            &r.cert,
            SERVER_ID,
            serde_json::json!({"t": "offer", "ufrag": "short", "pwd": "x", "cands": [], "claim": r.claim}),
        );
        r.signal.handle(1, &offer).await;

        let logged = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(
            logged.contains("minting a relay credential failed"),
            "the capture saw the log lines"
        );
        let server = IceServer {
            urls: vec!["turn:t:3478".into()],
            username: "SECRETUSER-123".into(),
            credential: "SECRETCRED-456".into(),
        };
        let config = Config {
            signal: "wss://x/s/abc".into(),
            turn_key_id: KEY_ID.into(),
            turn_key_token: TOKEN.into(),
        };
        let bundle = termenbox_core::bundle::Bundle {
            signal: Some("wss://x.workers.dev/s/6kpsYabcdefghijkl".into()),
            relay: true,
            ..bundle_for_test()
        };
        let encoded = bundle.signed_bytes().unwrap();
        let everything = [
            logged,
            format!("{server:?}{config:?}{}", r.usage.status_line()),
            String::from_utf8_lossy(&encoded).into_owned(),
        ]
        .join("\n");
        for secret in ["SECRETCRED-456", "SECRETUSER-123", TOKEN] {
            assert!(!everything.contains(secret), "{secret} leaked");
        }
    }

    fn bundle_for_test() -> termenbox_core::bundle::Bundle {
        termenbox_core::bundle::Bundle {
            server_id: crate::directory::ServerKey::from_seed([5u8; 32]).server_id(),
            seq: 1,
            expires: 1_900_000_000,
            published: false,
            fingerprint: format!("sha-256 {}", vec!["AB"; 32].join(":")),
            name: "home".into(),
            candidates: vec![],
            signal: None,
            relay: false,
        }
    }

    #[tokio::test]
    async fn signal_ignores_replayed_expired_and_malformed_messages() {
        let r = rig(500).await;
        let m = r.relay_request(serde_json::json!({}));
        assert!(text(r.signal.handle(1, &m).await).contains("ice"));
        assert!(
            r.signal.handle(1, &m).await.is_none(),
            "a replay is ignored"
        );
        assert!(
            r.signal.handle(2, &m).await.is_none(),
            "also from another client"
        );

        let expired = r.relay_request(serde_json::json!({"exp": now_secs() - 1}));
        assert!(r.signal.handle(1, &expired).await.is_none());
        let far = r.relay_request(serde_json::json!({"exp": now_secs() + 3600}));
        assert!(r.signal.handle(1, &far).await.is_none());
        let short = r.relay_request(serde_json::json!({"n": "abc"}));
        assert!(r.signal.handle(1, &short).await.is_none());
        assert!(r.signal.handle(1, b"not json").await.is_none());
        assert_eq!(r.calls.load(Ordering::SeqCst), 1, "only the first minted");
    }

    #[tokio::test]
    async fn signal_a_tampered_message_fails_its_signature() {
        let r = rig(500).await;
        let mut v: serde_json::Value =
            serde_json::from_slice(&r.relay_request(serde_json::json!({}))).unwrap();
        // The signature covers the kind: an offer cannot be made out of a signed relay request.
        v["t"] = "offer".into();
        let reply = text(r.signal.handle(1, &serde_json::to_vec(&v).unwrap()).await);
        assert!(reply.contains("error"), "{reply}");
        assert_eq!(r.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_overrides_accept_only_loopback_plaintext() {
        let cfg = "wss://w.workers.dev/s/abc";
        assert_eq!(dial_url(cfg, None), cfg);
        assert_eq!(
            dial_url(cfg, Some("ws://127.0.0.1:9/s/abc")),
            "ws://127.0.0.1:9/s/abc"
        );
        assert_eq!(
            dial_url(cfg, Some("ws://localhost/s/abc")),
            "ws://localhost/s/abc"
        );
        for bad in [
            "ws://evil.example/s",
            "ws://127.0.0.1.evil.example/s",
            "wss://127.0.0.1/s",
            "ws://10.0.0.1/s",
        ] {
            assert_eq!(dial_url(cfg, Some(bad)), cfg, "{bad}");
        }
        assert_eq!(credential_api(None), CLOUDFLARE_API);
        assert_eq!(
            credential_api(Some("http://127.0.0.1:8/")),
            "http://127.0.0.1:8/"
        );
        assert_eq!(
            credential_api(Some("https://api.example")),
            "https://api.example"
        );
        assert_eq!(credential_api(Some("http://evil.example")), CLOUDFLARE_API);
        assert_eq!(
            credential_api(Some("http://127.0.0.1.evil.example")),
            CLOUDFLARE_API
        );
    }

    #[test]
    fn signal_backoff_doubles_to_a_ceiling_and_resets() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(5));
        let waits: Vec<u64> = (0..5).map(|_| b.next().as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 5, 5]);
        b.reset();
        assert_eq!(b.next().as_secs(), 1);
    }

    /// A Worker that accepts a socket and closes it at once, to see how fast the server comes back.
    #[tokio::test]
    async fn signal_backs_off_after_a_closed_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/s/x", listener.local_addr().unwrap());
        let (times_tx, mut times) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let times_tx = times_tx.clone();
                tokio::spawn(async move {
                    if let Ok(mut ws) = tokio_tungstenite::accept_async(sock).await {
                        let _ = times_tx.send(Instant::now());
                        let _ = ws.close(None).await;
                    }
                });
            }
        });
        let r = rig(500).await;
        let task = tokio::spawn(run(
            url,
            Arc::new(r.signal),
            Backoff::new(Duration::from_millis(150), Duration::from_secs(2)),
        ));
        let mut at = vec![];
        for _ in 0..3 {
            at.push(
                tokio::time::timeout(Duration::from_secs(5), times.recv())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        task.abort();
        let (g1, g2) = (at[1] - at[0], at[2] - at[1]);
        assert!(g1 >= Duration::from_millis(150), "first retry after {g1:?}");
        assert!(
            g2 >= Duration::from_millis(300),
            "second retry after {g2:?}"
        );
    }

    #[tokio::test]
    async fn signal_answers_a_client_through_the_worker_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/s/x", listener.local_addr().unwrap());
        let r = rig(500).await;
        let request = r.relay_request(serde_json::json!({}));
        let task = tokio::spawn(run(
            url,
            Arc::new(r.signal),
            Backoff::new(Duration::from_millis(100), Duration::from_secs(1)),
        ));
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        let mut frame = vec![7u8];
        frame.extend(request);
        ws.send(Message::Binary(frame)).await.unwrap();
        let got = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Binary(b) = got else {
            panic!("{got:?}")
        };
        assert_eq!(b[0], 7, "the answer is addressed to the asking client");
        assert!(String::from_utf8_lossy(&b[1..]).contains("ice"));
        task.abort();
    }
}
