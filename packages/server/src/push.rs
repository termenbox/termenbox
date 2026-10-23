//! Web Push (decision 0095, plan A.6): a push on the rising edge of an Attention item when no device shows the
//! session. VAPID (RFC 8292) and `aes128gcm` content encryption (RFC 8291) are done here with the crates the
//! server already has; no paid service. The payload names the session, the kind and a time, and never anything
//! an agent wrote (guardrail `notifications-carry-no-agent-text`).
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use chrono::Timelike;
use hmac::{Hmac, Mac};
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use p256::{ecdh::EphemeralSecret, EncodedPoint, PublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use termenbox_core::events::{Attention, AttentionKind};
use tracing::{info, warn};

use crate::events::{now_ms, EventHub, StatusChange};

/// Seconds a push may wait at the push service for a device that is offline.
const TTL_SECS: u32 = 4 * 60 * 60;
/// Subscriptions kept (one per device); the oldest go first.
pub const MAX_SUBSCRIPTIONS: usize = 20;
pub const DEFAULT_GRACE: Duration = Duration::from_secs(5);
const DEFAULT_SUBJECT: &str = "https://app.termenbox.dev";
/// Hosts a browser's push service can have. A subscription from the client names where this server will POST, so
/// anything else would turn the server into a request forger for the local network.
const PUSH_HOSTS: [&str; 5] = [
    "googleapis.com",
    "push.services.mozilla.com",
    "push.apple.com",
    "notify.windows.com",
    "push.microsoft.com",
];

/// Default initial wait before the first re-notify (10 minutes).
const DEFAULT_INITIAL_WAIT_SECS: u64 = 10 * 60;
/// Default maximum interval between re-notifies (1 hour).
const DEFAULT_MAX_INTERVAL_SECS: u64 = 60 * 60;
/// Default multiplier for growing intervals.
const DEFAULT_MULTIPLIER: f64 = 2.0;

/// Configuration for the escalation ladder.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EscalationConfig {
    /// Seconds to wait after the first push before the first re-notify.
    #[serde(default = "default_initial_wait")]
    pub initial_wait_secs: u64,
    /// Maximum seconds between re-notifies.
    #[serde(default = "default_max_interval")]
    pub max_interval_secs: u64,
    /// Multiplier applied to the interval after each fire.
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,
    /// Quiet hours as "HH:MM-HH:MM" (24h). Empty or None disables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<String>,
    /// Agent names to mute (no push for these agents).
    #[serde(default)]
    pub muted_agents: Vec<String>,
}

fn default_initial_wait() -> u64 {
    DEFAULT_INITIAL_WAIT_SECS
}
fn default_max_interval() -> u64 {
    DEFAULT_MAX_INTERVAL_SECS
}
fn default_multiplier() -> f64 {
    DEFAULT_MULTIPLIER
}

impl Default for EscalationConfig {
    fn default() -> Self {
        Self {
            initial_wait_secs: DEFAULT_INITIAL_WAIT_SECS,
            max_interval_secs: DEFAULT_MAX_INTERVAL_SECS,
            multiplier: DEFAULT_MULTIPLIER,
            quiet_hours: None,
            muted_agents: Vec::new(),
        }
    }
}

/// Internal state for a single ladder rung.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct LadderState {
    item_id: String,
    session: String,
    agent: String,
    kind: AttentionKind,
    next_fire_at: u64,
    interval_secs: u64,
    fires: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    pub endpoint: String,
    /// The browser's public key, base64url of the 65-byte uncompressed point.
    pub p256dh: String,
    /// The browser's 16-byte auth secret, base64url.
    pub auth: String,
}

/// The server's VAPID identity: loaded from the connectors file, held in memory.
pub struct Vapid {
    key: SigningKey,
    subject: String,
}

impl Vapid {
    pub fn from_private(b64: &str, subject: &str) -> Result<Self> {
        let raw = B64.decode(b64.trim())?;
        Ok(Self {
            key: SigningKey::from_slice(&raw).map_err(|_| anyhow!("not a P-256 private key"))?,
            subject: if subject.is_empty() {
                DEFAULT_SUBJECT.into()
            } else {
                subject.into()
            },
        })
    }

    /// The application server key a browser subscribes with (base64url of the uncompressed point).
    pub fn public_key(&self) -> String {
        B64.encode(self.key.verifying_key().to_encoded_point(false).as_bytes())
    }

    fn authorization(&self, endpoint: &str, now_secs: u64) -> Result<String> {
        let aud = origin_of(endpoint).ok_or_else(|| anyhow!("bad endpoint"))?;
        let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = B64.encode(
            serde_json::json!({"aud": aud, "exp": now_secs + 12 * 3600, "sub": self.subject})
                .to_string(),
        );
        let signing_input = format!("{header}.{claims}");
        let sig: Signature = self.key.sign(signing_input.as_bytes());
        Ok(format!(
            "vapid t={signing_input}.{},k={}",
            B64.encode(sig.to_bytes()),
            self.public_key()
        ))
    }
}

/// A new key, as the line the owner adds to `connectors.env`.
pub fn new_key_line() -> (String, String) {
    let key = SigningKey::random(&mut rand::thread_rng());
    let public = B64.encode(key.verifying_key().to_encoded_point(false).as_bytes());
    (
        format!("TERMENBOX_VAPID_PRIVATE_KEY={}", B64.encode(key.to_bytes())),
        public,
    )
}

/// `connectors.env` (decision 0080): `KEY=value` lines in a file only its owner can read.
pub fn load_connectors(dir: &Path) -> HashMap<String, String> {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("connectors.env");
    let Ok(meta) = std::fs::metadata(&path) else {
        return HashMap::new();
    };
    if meta.permissions().mode() & 0o077 != 0 {
        warn!(
            "connectors.env is readable by others (mode {:o}); ignoring it. Run chmod 600 on it",
            meta.permissions().mode() & 0o777
        );
        return HashMap::new();
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    text.lines()
        .filter_map(|l| l.trim().split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .collect()
}

/// `termenbox-server --setup-push`: writes a new VAPID key into `connectors.env` (0600) unless one is there.
/// The private key goes from this process into that file and nowhere else; only the public key is printed.
pub fn setup(dir: &Path) -> Result<String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = dir.join("connectors.env");
    if let Some(k) = load_connectors(dir).get("TERMENBOX_VAPID_PRIVATE_KEY") {
        let v = Vapid::from_private(k, "")?;
        return Ok(format!(
            "Web Push is already set up. Public key: {}",
            v.public_key()
        ));
    }
    if path.exists() && std::fs::metadata(&path)?.permissions().mode_bits() & 0o077 != 0 {
        return Err(anyhow!(
            "{} is readable by others; run chmod 600 on it first",
            path.display()
        ));
    }
    std::fs::create_dir_all(dir)?;
    let (line, public) = new_key_line();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(&path)?;
    writeln!(f, "{line}")?;
    Ok(format!(
        "Web Push key written to {}.\nPublic key: {public}\nRestart termenbox-server, then turn on notifications in the app.",
        path.display()
    ))
}

trait ModeBits {
    fn mode_bits(&self) -> u32;
}
impl ModeBits for std::fs::Permissions {
    fn mode_bits(&self) -> u32 {
        std::os::unix::fs::PermissionsExt::mode(self)
    }
}

fn origin_of(endpoint: &str) -> Option<String> {
    // Subscribing already insists on https and a vendor host; a test service is plain http on loopback.
    let (scheme, rest) = endpoint.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty() && matches!(scheme, "https" | "http")).then(|| format!("{scheme}://{host}"))
}

fn host_of(endpoint: &str) -> Option<&str> {
    let rest = endpoint.strip_prefix("https://")?;
    let auth = rest.split(['/', '?', '#']).next()?;
    let host = auth.rsplit('@').next()?;
    host.split(':').next()
}

/// Whether a browser vendor's push service could be at this endpoint.
pub fn endpoint_allowed(endpoint: &str) -> bool {
    host_of(endpoint).is_some_and(|h| {
        let h = h.to_ascii_lowercase();
        PUSH_HOSTS
            .iter()
            .any(|p| h == *p || h.ends_with(&format!(".{p}")))
    })
}

fn hmac(key: &[u8], data: &[&[u8]]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
    for d in data {
        m.update(d);
    }
    m.finalize().into_bytes().into()
}

/// HKDF with SHA-256 for outputs of at most 32 bytes (one block).
fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let prk = hmac(salt, &[ikm]);
    hmac(&prk, &[info, &[1]])[..len].to_vec()
}

/// RFC 8291 `aes128gcm` body for one message (a single record).
pub fn encrypt(sub: &Subscription, payload: &[u8]) -> Result<Vec<u8>> {
    let ua_public = B64.decode(&sub.p256dh)?;
    let auth = B64.decode(&sub.auth)?;
    let ua = PublicKey::from_sec1_bytes(&ua_public).map_err(|_| anyhow!("bad p256dh"))?;
    let secret = EphemeralSecret::random(&mut rand::thread_rng());
    let as_public = EncodedPoint::from(secret.public_key());
    let mut salt = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut salt);
    let shared = secret.diffie_hellman(&ua);
    body(
        shared.raw_secret_bytes(),
        &ua_public,
        as_public.as_bytes(),
        &auth,
        &salt,
        payload,
    )
}

fn body(
    shared: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    auth: &[u8],
    salt: &[u8; 16],
    payload: &[u8],
) -> Result<Vec<u8>> {
    let info = [b"WebPush: info\0".as_slice(), ua_public, as_public].concat();
    let ikm = hkdf(auth, shared, &info, 32);
    let cek = hkdf(salt, &ikm, b"Content-Encoding: aes128gcm\0", 16);
    let nonce = hkdf(salt, &ikm, b"Content-Encoding: nonce\0", 12);
    let mut plain = payload.to_vec();
    plain.push(2); // the delimiter of the last record
    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|_| anyhow!("key"))?;
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| anyhow!("encrypt"))?;
    let mut out = Vec::with_capacity(86 + sealed.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&4096u32.to_be_bytes());
    out.push(as_public.len() as u8);
    out.extend_from_slice(as_public);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// What a notification says: the session, the kind, the time, and a deep link. Nothing an agent wrote.
pub fn payload(session: &str, kind: AttentionKind, at: u64, session_key: &str) -> Vec<u8> {
    let kind = match kind {
        AttentionKind::NeedsInput => "needs_input",
        AttentionKind::FinishedUnseen => "finished",
        AttentionKind::Failed => "failed",
        AttentionKind::Unknown => "attention",
    };
    let deep_link = format!("/#/home?focus={}", session_key);
    serde_json::json!({"session": session, "kind": kind, "at": at, "tag": session, "deep_link": deep_link})
        .to_string()
        .into_bytes()
}

/// The part of a session id a person recognises: without the manager prefix.
fn display_name(session: &str) -> &str {
    session.rsplit(':').next().unwrap_or(session)
}

pub enum Sent {
    Ok,
    /// 404 or 410: the subscription is dead.
    Gone,
    Failed(String),
}

pub struct Push {
    vapid: Option<Vapid>,
    subs: Mutex<Vec<Subscription>>,
    path: PathBuf,
    foreground: Mutex<HashMap<u64, String>>,
    client: reqwest::Client,
    /// Tests post to a local fake service; production only to the browser vendors' hosts.
    allow_any_endpoint: bool,
    pub grace: Duration,
    /// Escalation ladder configuration (persisted to push_settings.json).
    config: Arc<Mutex<EscalationConfig>>,
    /// Active ladders keyed by Attention item_id.
    ladders: Mutex<HashMap<String, LadderState>>,
    /// Channel to signal ladder cancellation when Attention clears.
    ladder_cancel: broadcast::Sender<String>,
}

impl Push {
    /// Returns the sender for ladder cancellation signals.
    pub fn ladder_cancel_sender(&self) -> broadcast::Sender<String> {
        self.ladder_cancel.clone()
    }

    pub fn load(dir: &Path) -> Arc<Self> {
        let cfg = load_connectors(dir);
        let vapid = cfg.get("TERMENBOX_VAPID_PRIVATE_KEY").and_then(|k| {
            Vapid::from_private(
                k,
                cfg.get("TERMENBOX_VAPID_SUBJECT")
                    .map(String::as_str)
                    .unwrap_or(""),
            )
            .map_err(|e| warn!("TERMENBOX_VAPID_PRIVATE_KEY is not usable: {e}"))
            .ok()
        });
        let path = dir.join("push_subscriptions.json");
        let subs = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // Load escalation config from push_settings.json
        let settings_path = dir.join("push_settings.json");
        let config = std::fs::read(&settings_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Arc::new(Self::new(vapid, subs, path, config))
    }

    pub fn new(vapid: Option<Vapid>, subs: Vec<Subscription>, path: PathBuf, config: EscalationConfig) -> Self {
        let (ladder_cancel_tx, _) = broadcast::channel(16);
        Self {
            vapid,
            subs: Mutex::new(subs),
            path,
            foreground: Mutex::default(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            allow_any_endpoint: false,
            grace: DEFAULT_GRACE,
            config: Arc::new(Mutex::new(config)),
            ladders: Mutex::default(),
            ladder_cancel: ladder_cancel_tx,
        }
    }

    pub fn configured(&self) -> bool {
        self.vapid.is_some()
    }

    pub fn public_key(&self) -> Option<String> {
        self.vapid.as_ref().map(Vapid::public_key)
    }

    pub fn subscription_count(&self) -> usize {
        self.subs.lock().unwrap().len()
    }

    pub fn has(&self, endpoint: &str) -> bool {
        self.subs
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.endpoint == endpoint)
    }

    fn save(&self, subs: &[Subscription]) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let Ok(json) = serde_json::to_vec(subs) else {
            return;
        };
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.path)?;
            f.write_all(&json)
        };
        if let Err(e) = write() {
            warn!("cannot save push subscriptions: {e}");
        }
    }

    /// Saves the escalation config to push_settings.json.
    fn save_config(&self, config: &EscalationConfig) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let Ok(json) = serde_json::to_vec(config) else {
            return;
        };
        let path = self.path.with_file_name("push_settings.json");
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            f.write_all(&json)
        };
        if let Err(e) = write() {
            warn!("cannot save push settings: {e}");
        }
    }

    /// Returns a clone of the current escalation config.
    pub fn get_config(&self) -> EscalationConfig {
        self.config.lock().unwrap().clone()
    }

    /// Updates the escalation config and persists it.
    pub fn set_config(&self, config: EscalationConfig) {
        *self.config.lock().unwrap() = config.clone();
        self.save_config(&config);
    }

    /// Checks if the current time falls within quiet hours.
    fn in_quiet_hours(&self, now_ms: u64) -> bool {
        let config = self.config.lock().unwrap();
        let Some(qh) = &config.quiet_hours else {
            return false;
        };
        // Parse "HH:MM-HH:MM"
        let parts: Vec<&str> = qh.split('-').collect();
        if parts.len() != 2 {
            return false;
        }
        let parse = |s: &str| -> Option<u32> {
            let hm: Vec<&str> = s.split(':').collect();
            if hm.len() != 2 {
                return None;
            }
            let h = hm[0].parse::<u32>().ok()?;
            let m = hm[1].parse::<u32>().ok()?;
            if h > 23 || m > 59 {
                return None;
            }
            Some(h * 60 + m)
        };
        let Some(start) = parse(parts[0]) else { return false; };
        let Some(end) = parse(parts[1]) else { return false; };
        let Some(now) = chrono::DateTime::from_timestamp_millis(now_ms as i64) else { return false; };
        let now_min = now.time().hour() * 60 + now.time().minute();
        if start <= end {
            now_min >= start && now_min < end
        } else {
            // Overnight: e.g., 22:00-07:00
            now_min >= start || now_min < end
        }
    }

    /// Checks if an agent is muted.
    fn is_agent_muted(&self, agent: &str) -> bool {
        let config = self.config.lock().unwrap();
        config.muted_agents.iter().any(|a| a == agent)
    }

    /// One subscription per endpoint; a device that subscribes again replaces its old entry.
    pub fn subscribe(&self, sub: Subscription) -> Result<()> {
        if self.vapid.is_none() {
            return Err(anyhow!("Push is not set up on this server"));
        }
        if !(self.allow_any_endpoint || endpoint_allowed(&sub.endpoint)) {
            return Err(anyhow!("Not a push service address"));
        }
        let keys_ok = B64
            .decode(&sub.p256dh)
            .map(|k| k.len() == 65)
            .unwrap_or(false)
            && B64
                .decode(&sub.auth)
                .map(|a| a.len() == 16)
                .unwrap_or(false);
        if !keys_ok || sub.endpoint.len() > 2048 {
            return Err(anyhow!("Bad subscription"));
        }
        let mut subs = self.subs.lock().unwrap();
        subs.retain(|s| s.endpoint != sub.endpoint);
        subs.push(sub);
        while subs.len() > MAX_SUBSCRIPTIONS {
            subs.remove(0);
        }
        self.save(&subs);
        info!(target: "audit", event = "push_subscribed", count = subs.len());
        Ok(())
    }

    pub fn unsubscribe(&self, endpoint: &str) {
        let mut subs = self.subs.lock().unwrap();
        let before = subs.len();
        subs.retain(|s| s.endpoint != endpoint);
        if subs.len() != before {
            self.save(&subs);
            info!(target: "audit", event = "push_unsubscribed", count = subs.len());
        }
    }

    /// A connection says which session it shows in the foreground, or none.
    pub fn set_foreground(&self, conn: u64, session: Option<String>) {
        let mut fg = self.foreground.lock().unwrap();
        match session {
            Some(s) => fg.insert(conn, s),
            None => fg.remove(&conn),
        };
    }

    fn shown(&self, session: &str) -> bool {
        self.foreground
            .lock()
            .unwrap()
            .values()
            .any(|s| s == session)
    }

    async fn send_one(&self, sub: &Subscription, body: &[u8], topic: &str) -> Sent {
        let Some(v) = &self.vapid else {
            return Sent::Failed("not set up".into());
        };
        let auth = match v.authorization(&sub.endpoint, now_ms() / 1000) {
            Ok(a) => a,
            Err(e) => return Sent::Failed(e.to_string()),
        };
        let encrypted = match encrypt(sub, body) {
            Ok(b) => b,
            Err(e) => return Sent::Failed(e.to_string()),
        };
        let res = self
            .client
            .post(&sub.endpoint)
            .header("Authorization", auth)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("TTL", TTL_SECS.to_string())
            .header("Urgency", "high")
            .header("Topic", topic)
            .body(encrypted)
            .send()
            .await;
        match res {
            Ok(r) if r.status().is_success() => Sent::Ok,
            Ok(r) if matches!(r.status().as_u16(), 404 | 410) => Sent::Gone,
            Ok(r) => Sent::Failed(format!("push service answered {}", r.status())),
            Err(e) => Sent::Failed(e.to_string()),
        }
    }

    /// Sends to every device; a dead subscription is removed. Returns how many accepted it.
    pub async fn notify(&self, session: &str, kind: AttentionKind, at: u64, session_key: &str) -> usize {
        let name = display_name(session);
        let body = payload(name, kind, at, session_key);
        // The service keeps one waiting message per topic: a newer one for the session replaces the older.
        let topic = B64.encode(&Sha256::digest(session.as_bytes())[..16]);
        let subs = self.subs.lock().unwrap().clone();
        let mut ok = 0;
        for s in subs {
            match self.send_one(&s, &body, &topic).await {
                Sent::Ok => ok += 1,
                Sent::Gone => {
                    self.unsubscribe(&s.endpoint);
                }
                Sent::Failed(why) => warn!("push failed: {why}"),
            }
        }
        // Session and kind only (audit-logging-no-secrets).
        info!(target: "audit", event = "push_sent", session = %session, kind = ?kind, accepted = ok);
        ok
    }
}

/// Watches Attention items: for each new one, waits out the grace, and pushes unless it is gone by then or a
/// device shows the session. A hint never reaches here: only items raised by a hook exist.
/// After the first push, re-notifies at growing intervals (escalation ladder) until the Attention clears.
pub fn spawn_watcher(push: Arc<Push>, hub: EventHub) {
    let mut rx = hub.subscribe_status();
    let mut ladder_cancel_rx = push.ladder_cancel.subscribe();
    tokio::spawn(async move {
        let mut seen: HashSet<String> = HashSet::new();
        loop {
            tokio::select! {
                // Handle Attention events
                items_result = rx.recv() => {
                    let items: Vec<Attention> = match items_result {
                        Ok(StatusChange::Attention(items)) => items,
                        Ok(_) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => hub.attention_items(),
                        Err(_) => return,
                    };
                    for it in items {
                        if !seen.insert(it.id.clone()) {
                            continue;
                        }
                        if seen.len() > 1000 {
                            seen.clear();
                        }
                        if !push.configured() || push.subscription_count() == 0 {
                            continue;
                        }
                        let (push, hub, it) = (push.clone(), hub.clone(), it.clone());
                        tokio::spawn(async move {
                            tokio::time::sleep(push.grace).await;
                            let still_open = hub.attention_items().iter().any(|a| a.id == it.id);
                            if still_open && !push.shown(&it.session) {
                                // First push
                                push.notify(&it.session, it.kind, it.raised_at, &it.session).await;
                                // Start the escalation ladder
                                start_ladder(push, hub, it).await;
                            }
                        });
                    }
                }
                // Handle ladder cancellation when Attention clears
                cancel_result = ladder_cancel_rx.recv() => {
                    if let Ok(item_id) = cancel_result {
                        let mut ladders = push.ladders.lock().unwrap();
                        ladders.remove(&item_id);
                    }
                }
            }
        }
    });
}

/// Starts the escalation ladder for an Attention item.
async fn start_ladder(push: Arc<Push>, hub: EventHub, item: Attention) {
    // We need the agent name for mute checking. Get it from the hub.
    let agent = hub.agent(&item.session).unwrap_or_default();

    let config = push.get_config();
    let interval_secs = config.initial_wait_secs;
    let now = crate::events::now_ms();
    let next_fire_at = now + interval_secs * 1000;

    let ladder_state = LadderState {
        item_id: item.id.clone(),
        session: item.session.clone(),
        agent,
        kind: item.kind,
        next_fire_at,
        interval_secs,
        fires: 1, // First push already sent
    };

    push.ladders.lock().unwrap().insert(item.id.clone(), ladder_state);

    // Spawn the ladder task
    tokio::spawn(async move {
        let mut cancel_rx = push.ladder_cancel.subscribe();
        loop {
            // Get the next fire time and check if ladder still exists (without holding lock across await)
            let (should_check, wait_ms, item_id, session, kind, raised_at) = {
                let mut ladders = push.ladders.lock().unwrap();
                let Some(state) = ladders.get_mut(&item.id) else {
                    return; // Ladder was cancelled
                };
                let now = crate::events::now_ms();
                if now < state.next_fire_at {
                    // Not time yet
                    let wait_ms = state.next_fire_at - now;
                    (false, wait_ms, item.id.clone(), item.session.clone(), item.kind, item.raised_at)
                } else {
                    // Time to fire
                    let should_fire = !push.in_quiet_hours(now) && !push.is_agent_muted(&state.agent);
                    let config = push.get_config();
                    state.fires += 1;
                    state.interval_secs = (state.interval_secs as f64 * config.multiplier).ceil() as u64;
                    state.interval_secs = state.interval_secs.min(config.max_interval_secs);
                    state.next_fire_at = now + state.interval_secs * 1000;
                    let next_fire_at = state.next_fire_at;
                    (should_fire, next_fire_at.saturating_sub(now), item.id.clone(), item.session.clone(), item.kind, item.raised_at)
                }
            };

            if !should_check {
                // Wait until next fire time or cancellation
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {}
                    _ = cancel_rx.recv() => return,
                }
                continue;
            }

            // Check if the Attention item still exists and session not shown
            let still_open = hub.attention_items().iter().any(|a| a.id == item_id);
            if still_open && !push.shown(&session) {
                push.notify(&session, kind, raised_at, &session).await;
            } else {
                // Attention cleared or session shown - stop ladder
                push.ladders.lock().unwrap().remove(&item_id);
                return;
            }

            // Wait for next interval or cancellation
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {}
                _ = cancel_rx.recv() => {
                    push.ladders.lock().unwrap().remove(&item_id);
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::SecretKey;

    /// What a browser does with the body: the receiving half of RFC 8291.
    fn decrypt(ua: &SecretKey, auth: &[u8], body: &[u8]) -> Vec<u8> {
        let salt: [u8; 16] = body[..16].try_into().unwrap();
        let idlen = body[20] as usize;
        let as_public = &body[21..21 + idlen];
        let sealed = &body[21 + idlen..];
        let peer = PublicKey::from_sec1_bytes(as_public).unwrap();
        let shared = p256::ecdh::diffie_hellman(ua.to_nonzero_scalar(), peer.as_affine());
        let ua_public = EncodedPoint::from(ua.public_key());
        let info = [
            b"WebPush: info\0".as_slice(),
            ua_public.as_bytes(),
            as_public,
        ]
        .concat();
        let ikm = hkdf(auth, shared.raw_secret_bytes(), &info, 32);
        let cek = hkdf(&salt, &ikm, b"Content-Encoding: aes128gcm\0", 16);
        let nonce = hkdf(&salt, &ikm, b"Content-Encoding: nonce\0", 12);
        let plain = Aes128Gcm::new_from_slice(&cek)
            .unwrap()
            .decrypt(Nonce::from_slice(&nonce), sealed)
            .unwrap();
        assert_eq!(plain.last(), Some(&2));
        plain[..plain.len() - 1].to_vec()
    }

    fn device() -> (SecretKey, Vec<u8>, Subscription) {
        let ua = SecretKey::random(&mut rand::thread_rng());
        let mut auth = vec![0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut auth);
        let public = EncodedPoint::from(ua.public_key());
        let sub = Subscription {
            endpoint: "https://fcm.googleapis.com/fcm/send/abc".into(),
            p256dh: B64.encode(public.as_bytes()),
            auth: B64.encode(&auth),
        };
        (ua, auth, sub)
    }

    #[test]
    fn the_rfc_8291_example_encrypts_to_the_documented_bytes() {
        // RFC 8291 appendix A.
        let d = |s: &str| B64.decode(s).unwrap();
        let shared = d("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs");
        let ua_public = d("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4");
        let as_public = d("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8");
        let auth = d("BTBZMqHH6r4Tts7J_aSIgg");
        let salt: [u8; 16] = d("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let out = body(
            &shared,
            &ua_public,
            &as_public,
            &auth,
            &salt,
            b"When I grow up, I want to be a watermelon",
        )
        .unwrap();
        assert_eq!(
            B64.encode(&out),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn a_device_can_read_what_encrypt_made() {
        let (ua, auth, sub) = device();
        let body = encrypt(&sub, b"hello").unwrap();
        assert_eq!(decrypt(&ua, &auth, &body), b"hello");
    }

    #[test]
    fn the_payload_has_no_room_for_agent_text() {
        let p = String::from_utf8(payload("api-refactor", AttentionKind::NeedsInput, 5, "api-refactor")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&p).unwrap();
        assert_eq!(v["at"], 5);
        assert_eq!(v["kind"], "needs_input");
        assert_eq!(v["session"], "api-refactor");
        assert_eq!(v["tag"], "api-refactor");
        assert_eq!(v["deep_link"], "/#/home?focus=api-refactor");
        assert_eq!(display_name("tmux:work"), "work");
    }

    #[test]
    fn only_browser_vendor_hosts_are_pushed_to() {
        for ok in [
            "https://fcm.googleapis.com/fcm/send/x",
            "https://updates.push.services.mozilla.com/wpush/v2/x",
            "https://web.push.apple.com/x",
        ] {
            assert!(endpoint_allowed(ok), "{ok}");
        }
        for bad in [
            "http://fcm.googleapis.com/x",
            "https://127.0.0.1/x",
            "https://evilgoogleapis.com/x",
            "https://fcm.googleapis.com@evil.example/x",
            "https://localhost:9/x",
        ] {
            assert!(!endpoint_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn a_vapid_header_carries_a_signature_the_public_key_verifies() {
        use p256::ecdsa::{signature::Verifier, VerifyingKey};
        let (line, public) = new_key_line();
        let key = line.strip_prefix("TERMENBOX_VAPID_PRIVATE_KEY=").unwrap();
        let v = Vapid::from_private(key, "mailto:o@example.org").unwrap();
        assert_eq!(v.public_key(), public);
        let h = v
            .authorization("https://fcm.googleapis.com/fcm/send/x", 1000)
            .unwrap();
        let t = h
            .strip_prefix("vapid t=")
            .unwrap()
            .split(",k=")
            .next()
            .unwrap();
        let (input, sig) = t.rsplit_once('.').unwrap();
        let vk = VerifyingKey::from_sec1_bytes(&B64.decode(&public).unwrap()).unwrap();
        let sig = Signature::from_slice(&B64.decode(sig).unwrap()).unwrap();
        vk.verify(input.as_bytes(), &sig).unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&B64.decode(input.split('.').nth(1).unwrap()).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["sub"], "mailto:o@example.org");
    }

    #[test]
    fn setup_writes_a_private_key_file_once_and_prints_only_the_public_key() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("termenbox-push-{}", uuid::Uuid::new_v4()));
        let msg = setup(&dir).unwrap();
        let mode = std::fs::metadata(dir.join("connectors.env"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let key = &load_connectors(&dir)["TERMENBOX_VAPID_PRIVATE_KEY"];
        assert!(
            !msg.contains(key.as_str()),
            "the private key is never printed"
        );
        let public = Vapid::from_private(key, "").unwrap().public_key();
        assert!(msg.contains(&public));
        assert!(setup(&dir).unwrap().contains("already set up"));
        assert_eq!(load_connectors(&dir).len(), 1);
    }

    #[test]
    fn the_connectors_file_is_ignored_unless_only_its_owner_can_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("termenbox-push-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("connectors.env");
        std::fs::write(
            &f,
            "# note\nTERMENBOX_VAPID_PRIVATE_KEY=abc\nOTHER = \"x y\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_connectors(&dir).is_empty());
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        let c = load_connectors(&dir);
        assert_eq!(c["TERMENBOX_VAPID_PRIVATE_KEY"], "abc");
        assert_eq!(c["OTHER"], "x y");
    }

    // --- against a fake push service ----------------------------------------------------------------

    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
        Router,
    };
    use tokio::sync::mpsc;

    type Hit = (HeaderMap, Vec<u8>);

    /// A push service that records each request and answers with the status in `reply`.
    async fn fake_service(reply: Arc<Mutex<StatusCode>>) -> (String, mpsc::UnboundedReceiver<Hit>) {
        let (tx, rx) = mpsc::unbounded_channel();
        async fn handler(
            State((tx, reply)): State<(mpsc::UnboundedSender<Hit>, Arc<Mutex<StatusCode>>)>,
            h: HeaderMap,
            b: axum::body::Bytes,
        ) -> StatusCode {
            let _ = tx.send((h, b.to_vec()));
            *reply.lock().unwrap()
        }
        let app = Router::new()
            .route("/push", post(handler))
            .with_state((tx, reply));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        // Plain http on loopback: only possible because the test lifts the host check.
        (format!("http://{addr}/push"), rx)
    }

    fn push_for(endpoint: &str, ua: &SecretKey, auth: &[u8]) -> (Push, Subscription) {
        let (line, _) = new_key_line();
        let v = Vapid::from_private(
            line.strip_prefix("TERMENBOX_VAPID_PRIVATE_KEY=").unwrap(),
            "",
        )
        .unwrap();
        let dir = std::env::temp_dir().join(format!("termenbox-push-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let public = EncodedPoint::from(ua.public_key());
        let sub = Subscription {
            endpoint: endpoint.into(),
            p256dh: B64.encode(public.as_bytes()),
            auth: B64.encode(auth),
        };
        let mut p = Push::new(Some(v), vec![], dir.join("subs.json"), EscalationConfig::default());
        p.allow_any_endpoint = true;
        p.grace = Duration::from_millis(50);
        p.subs.lock().unwrap().push(sub.clone());
        (p, sub)
    }

    #[tokio::test]
    async fn a_push_carries_no_marker_text_from_the_title_or_prompt() {
        // The guardrail's test: an event whose title and prompt hold a marker, then the bytes the service received.
        use termenbox_core::events::{Confidence, EventKind, Source};
        let (ua, auth, _keys) = device();
        let (url, mut rx) = fake_service(Arc::new(Mutex::new(StatusCode::CREATED))).await;
        let (push, _) = push_for(&url, &ua, &auth);
        let push = Arc::new(push);
        let hub = EventHub::new([3; 32]);
        spawn_watcher(push.clone(), hub.clone());
        const MARKER: &str = "zebra-marker-7781";
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::Task {
                title: MARKER.into(),
            },
            1_000,
        );
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: termenbox_core::events::AgentState::NeedsInput,
            },
            1_001,
        );
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::Message {
                summary: format!("{MARKER} please approve"),
            },
            1_002,
        );
        let (headers, body) = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(headers["authorization"]
            .to_str()
            .unwrap()
            .starts_with("vapid t="));
        assert_eq!(headers["content-encoding"], "aes128gcm");
        assert!(headers.contains_key("topic") && headers["ttl"] == "14400");
        assert!(!String::from_utf8_lossy(&body).contains(MARKER));
        let plain = String::from_utf8(decrypt(&ua, &auth, &body)).unwrap();
        assert!(!plain.contains(MARKER), "{plain}");
        let v: serde_json::Value = serde_json::from_str(&plain).unwrap();
        assert_eq!(
            (v["session"].as_str(), v["kind"].as_str()),
            (Some("work"), Some("needs_input"))
        );
        // One push per edge: the later message text did not make a second one.
        assert!(tokio::time::timeout(Duration::from_millis(400), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_device_showing_the_session_suppresses_the_push_and_so_does_a_quick_ack() {
        use termenbox_core::events::{AgentState, Confidence, EventKind, Source};
        let (ua, auth, _keys) = device();
        let (url, mut rx) = fake_service(Arc::new(Mutex::new(StatusCode::CREATED))).await;
        let (push, _) = push_for(&url, &ua, &auth);
        let push = Arc::new(push);
        let hub = EventHub::new([3; 32]);
        spawn_watcher(push.clone(), hub.clone());
        push.set_foreground(1, Some("work".into()));
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::NeedsInput,
            },
            1_000,
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(400), rx.recv())
                .await
                .is_err(),
            "viewed session"
        );
        push.set_foreground(1, None);
        hub.push(
            "quick",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::Working,
            },
            1_000,
        );
        hub.push(
            "quick",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::Done,
            },
            1_001,
        );
        let id = hub
            .attention_items()
            .iter()
            .find(|a| a.session == "quick")
            .unwrap()
            .id
            .clone();
        assert!(hub.ack(&id));
        assert!(
            tokio::time::timeout(Duration::from_millis(400), rx.recv())
                .await
                .is_err(),
            "acked within the grace"
        );
    }

    #[tokio::test]
    async fn an_expired_subscription_is_removed_and_a_hint_pushes_nothing() {
        use termenbox_core::events::{AgentState, Confidence, EventKind, Source};
        let (ua, auth, _keys) = device();
        let (url, mut rx) = fake_service(Arc::new(Mutex::new(StatusCode::GONE))).await;
        let (push, _) = push_for(&url, &ua, &auth);
        let push = Arc::new(push);
        let hub = EventHub::new([3; 32]);
        spawn_watcher(push.clone(), hub.clone());
        hub.push(
            "s",
            Source::Screen,
            Confidence::Low,
            EventKind::State {
                state: AgentState::NeedsInput,
            },
            1_000,
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(400), rx.recv())
                .await
                .is_err(),
            "a screen hint is never pushed"
        );
        assert_eq!(push.subscription_count(), 1);
        hub.push(
            "s",
            Source::Hook,
            Confidence::High,
            EventKind::State {
                state: AgentState::NeedsInput,
            },
            1_001,
        );
        tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        for _ in 0..50 {
            if push.subscription_count() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            push.subscription_count(),
            0,
            "a 410 removes the subscription"
        );
    }

    #[test]
    fn subscribing_needs_a_key_a_push_host_and_well_formed_keys() {
        let (_, _, keys) = device();
        let dir = std::env::temp_dir().join(format!("termenbox-push-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let off = Push::new(None, vec![], dir.join("push_subscriptions.json"), EscalationConfig::default());
        assert!(off.subscribe(keys.clone()).is_err(), "not set up");
        let (line, _) = new_key_line();
        let v = Vapid::from_private(
            line.strip_prefix("TERMENBOX_VAPID_PRIVATE_KEY=").unwrap(),
            "",
        )
        .unwrap();
        let on = Push::new(Some(v), vec![], dir.join("push_subscriptions.json"), EscalationConfig::default());
        assert!(on
            .subscribe(Subscription {
                endpoint: "https://127.0.0.1/x".into(),
                ..keys.clone()
            })
            .is_err());
        assert!(on
            .subscribe(Subscription {
                auth: "short".into(),
                ..keys.clone()
            })
            .is_err());
        on.subscribe(keys.clone()).unwrap();
        on.subscribe(keys.clone()).unwrap();
        assert_eq!(on.subscription_count(), 1);
        assert_eq!(
            Push::load(&dir).subscription_count(),
            1,
            "kept across a restart"
        );
        on.unsubscribe(&keys.endpoint);
        assert_eq!(on.subscription_count(), 0);
    }

    // --- Escalation ladder tests ----------------------------------------------------------------

    #[tokio::test]
    async fn escalation_ladder_timing() {
        use termenbox_core::events::{AgentState, Confidence, EventKind, Source};
        let (ua, auth, _) = device();
        let (url, mut rx) = fake_service(Arc::new(Mutex::new(StatusCode::CREATED))).await;
        let (push, _) = push_for(&url, &ua, &auth);
        let mut push = push;
        // Configure short intervals for testing (using milliseconds by setting very small values)
        // Note: initial_wait_secs is in seconds, so we use 1 second for testing
        push.set_config(EscalationConfig {
            initial_wait_secs: 1,
            max_interval_secs: 5,
            multiplier: 2.0,
            quiet_hours: None,
            muted_agents: vec![],
        });
        // Reduce grace period for faster test
        push.grace = Duration::from_millis(50);
        let push = Arc::new(push);
        let hub = EventHub::new([3; 32]);
        spawn_watcher(push.clone(), hub.clone());

        // Trigger attention
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::State { state: AgentState::NeedsInput },
            1_000,
        );

        // Wait for grace period + first push
        let (_, body1) = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&body1).is_empty());

        // Wait for initial_wait (1 second) + second push
        let (_, body2) = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&body2).is_empty());

        // Wait for multiplier * initial_wait = 2 seconds + third push
        let (_, body3) = tokio::time::timeout(Duration::from_secs(4), rx.recv()).await.unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&body3).is_empty());

        // Wait for multiplier * 2 = 4 seconds, but capped at max_interval (5 seconds)
        let (_, body4) = tokio::time::timeout(Duration::from_secs(7), rx.recv()).await.unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&body4).is_empty());

        // Next should be at max_interval (5 seconds)
        let (_, body5) = tokio::time::timeout(Duration::from_secs(7), rx.recv()).await.unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&body5).is_empty());
    }

    #[tokio::test]
    async fn escalation_ladder_answer_cancels() {
        use termenbox_core::events::{AgentState, Confidence, EventKind, Source, AttentionKind};
        let (ua, auth, _) = device();
        let (url, mut rx) = fake_service(Arc::new(Mutex::new(StatusCode::CREATED))).await;
        let (push, _) = push_for(&url, &ua, &auth);
        let mut push = push;
        push.set_config(EscalationConfig {
            initial_wait_secs: 1,
            max_interval_secs: 5,
            multiplier: 2.0,
            quiet_hours: None,
            muted_agents: vec![],
        });
        push.grace = Duration::from_millis(50);
        let push = Arc::new(push);
        let hub = EventHub::new([3; 32]);
        spawn_watcher(push.clone(), hub.clone());

        // Trigger attention with FinishedUnseen (can be acknowledged)
        hub.raise_attention_for_test("work", AttentionKind::FinishedUnseen, 1_000);

        // Wait for grace period + first push
        let _ = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.unwrap().unwrap();

        // Get the attention item ID
        let attention_id = hub.attention_items()[0].id.clone();

        // Acknowledge (answer) the attention
        assert!(hub.ack(&attention_id));

        // Wait longer than initial_wait - should not send more pushes
        assert!(
            tokio::time::timeout(Duration::from_secs(4), rx.recv()).await.is_err()
        );
    }

    #[tokio::test]
    async fn escalation_ladder_quiet_hours_suppress() {
        use termenbox_core::events::{AgentState, Confidence, EventKind, Source};
        let (ua, auth, _) = device();
        let (url, mut rx) = fake_service(Arc::new(Mutex::new(StatusCode::CREATED))).await;
        let (push, _) = push_for(&url, &ua, &auth);
        let mut push = push;
        // Set quiet hours to cover the test time
        push.set_config(EscalationConfig {
            initial_wait_secs: 1,
            max_interval_secs: 5,
            multiplier: 2.0,
            quiet_hours: Some("00:00-23:59".into()), // All day quiet
            muted_agents: vec![],
        });
        push.grace = Duration::from_millis(50);
        let push = Arc::new(push);
        let hub = EventHub::new([3; 32]);
        spawn_watcher(push.clone(), hub.clone());

        // Test in_quiet_hours directly
        let test_time = chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap().and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp_millis() as u64;
        assert!(push.in_quiet_hours(test_time));

        // Trigger attention
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::State { state: AgentState::NeedsInput },
            1_000,
        );

        // Wait for grace period + first push (grace period bypasses quiet hours)
        let _ = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.unwrap().unwrap();

        // Wait for initial_wait - second push should be suppressed by quiet hours
        assert!(
            tokio::time::timeout(Duration::from_secs(4), rx.recv()).await.is_err()
        );
    }

    #[tokio::test]
    async fn escalation_ladder_mute_suppress() {
        use termenbox_core::events::{AgentState, Confidence, EventKind, Source};
        let (ua, auth, _) = device();
        let (url, mut rx) = fake_service(Arc::new(Mutex::new(StatusCode::CREATED))).await;
        let (push, _) = push_for(&url, &ua, &auth);
        let mut push = push;
        push.set_config(EscalationConfig {
            initial_wait_secs: 1,
            max_interval_secs: 5,
            multiplier: 2.0,
            quiet_hours: None,
            muted_agents: vec!["claude".into()],
        });
        push.grace = Duration::from_millis(50);
        let push = Arc::new(push);
        let hub = EventHub::new([3; 32]);
        spawn_watcher(push.clone(), hub.clone());

        // Set agent to "claude" (muted)
        hub.set_agent("work", "claude");

        // Trigger attention
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::State { state: AgentState::NeedsInput },
            1_000,
        );

        // Wait for grace period + first push
        let _ = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.unwrap().unwrap();

        // Wait for initial_wait - second push should be suppressed by mute
        assert!(
            tokio::time::timeout(Duration::from_secs(4), rx.recv()).await.is_err()
        );
    }

    #[tokio::test]
    async fn escalation_payload_no_leak() {
        use termenbox_core::events::{AgentState, Confidence, EventKind, Source};
        let (ua, auth, _) = device();
        let (url, mut rx) = fake_service(Arc::new(Mutex::new(StatusCode::CREATED))).await;
        let (push, _) = push_for(&url, &ua, &auth);
        let mut push = push;
        push.grace = Duration::from_millis(50);
        let push = Arc::new(push);
        let hub = EventHub::new([3; 32]);
        spawn_watcher(push.clone(), hub.clone());

        const MARKER: &str = "escalation-marker-999";
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::Task { title: MARKER.into() },
            1_000,
        );
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::State { state: AgentState::NeedsInput },
            1_001,
        );
        hub.push(
            "work",
            Source::Hook,
            Confidence::High,
            EventKind::Message { summary: format!("{MARKER} please approve") },
            1_002,
        );

        // Wait for the watcher to process the attention and send the push
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check that attention was raised
        let attention_items = hub.attention_items();
        assert!(!attention_items.is_empty(), "No attention items raised");

        // Wait longer for the push to be processed and sent
        let (_, body) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.unwrap().unwrap();
        
        // Decrypt the body (it's encrypted with AES-GCM)
        let plain_bytes = decrypt(&ua, &auth, &body);
        let plain = String::from_utf8_lossy(&plain_bytes);
        
        // Verify marker text never reaches push
        assert!(!plain.contains(MARKER), "Marker text leaked in push payload: {plain}");
        
        // Verify payload structure
        let v: serde_json::Value = serde_json::from_str(&plain).unwrap();
        assert_eq!(v["session"], "work");
        assert_eq!(v["kind"], "needs_input");
        assert_eq!(v["deep_link"], "/#/home?focus=work");
        assert!(!v.as_object().unwrap().keys().any(|k| k == "title" || k == "prompt" || k == "summary"));
    }
}
