//! Announces this server to the signed directory (DECISION-0056). The directory is a metadata-only
//! rendezvous: it learns the server's public key, address and DTLS fingerprint, never any traffic,
//! and cannot forge a record because clients pin the public key (the `server_id`).
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::auth::AuthState;
use crate::rtc::RtcInfo;

/// A claimed server re-announces only when something changed (a new or ended claim) and otherwise once a
/// week, so the records stay fresh while the directory sees about one request per server per week.
const KEEPALIVE: Duration = Duration::from_secs(7 * 24 * 3600);
/// The address, port and fingerprint are fixed for the process, so a change of any of them is a restart
/// and announces on start.
const UNCLAIMED_REFRESH: Duration = Duration::from_secs(300);

/// The server's long-lived identity. Its public key is the id clients pin.
pub struct ServerKey(SigningKey);

impl ServerKey {
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let path = dir.join("server_key");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(seed) = <[u8; 32]>::try_from(bytes.as_slice()) {
                return Ok(Self(SigningKey::from_bytes(&seed)));
            }
        }
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        crate::auth::write_private(&path, &seed)?;
        Ok(Self(SigningKey::from_bytes(&seed)))
    }

    #[cfg(test)]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&seed))
    }

    pub fn server_id(&self) -> String {
        B64.encode(self.0.verifying_key().to_bytes())
    }

    /// Signs candidate bundles (`candidates.rs`) with the same key the directory records use.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.0.sign(message).to_bytes()
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub server_id: String,
    pub seq: u64,
    pub ip: String,
    pub udp_port: u16,
    pub fingerprint: String,
    pub ws_url: String,
    pub claim_hash: String,
    pub claim_proof: String,
    pub sig: String,
}

impl Record {
    /// The exact bytes that are signed. Keep in sync with canonicalMessage() in
    /// packages/client/functions/api/directory/_lib.ts.
    pub fn canonical(&self) -> String {
        [
            "termenbox-announce-v1",
            &self.server_id,
            &self.seq.to_string(),
            &self.ip,
            &self.udp_port.to_string(),
            &self.fingerprint,
            &self.ws_url,
            &self.claim_hash,
            &self.claim_proof,
        ]
        .join("\n")
    }
}

pub fn claim_hash(secret: &str) -> String {
    B64.encode(Sha256::digest(secret.as_bytes()))
}

/// Proof that this server knows the claim secret, without revealing it: the client, which generated
/// the secret, recomputes this to learn that `server_id` really is the machine it just set up.
pub fn claim_proof(
    secret: &str,
    server_id: &str,
    fingerprint: &str,
    ip: &str,
    port: u16,
) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(format!("termenbox-claim-v1\n{server_id}\n{fingerprint}\n{ip}:{port}").as_bytes());
    B64.encode(mac.finalize().into_bytes())
}

pub fn build(
    key: &ServerKey,
    seq: u64,
    info: &RtcInfo,
    ws_url: &str,
    claim: Option<&str>,
) -> Record {
    let server_id = key.server_id();
    let ip = info.ip.to_string();
    let (claim_hash, claim_proof) = match claim {
        Some(secret) => (
            claim_hash(secret),
            claim_proof(secret, &server_id, &info.fingerprint, &ip, info.port),
        ),
        None => (String::new(), String::new()),
    };
    let mut record = Record {
        server_id,
        seq,
        ip,
        udp_port: info.port,
        fingerprint: info.fingerprint.clone(),
        ws_url: ws_url.to_string(),
        claim_hash,
        claim_proof,
        sig: String::new(),
    };
    record.sig = B64.encode(key.0.sign(record.canonical().as_bytes()).to_bytes());
    record
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(1)
}

/// Keeps the directory current: on start, after failures with backoff, on a claim change, and every 7 days.
/// `seq` is the wall clock in milliseconds (forced to increase), so a restart needs no stored counter.
pub async fn run(
    key: Arc<ServerKey>,
    origin: String,
    info: RtcInfo,
    ws_url: String,
    auth: Arc<AuthState>,
    http: reqwest::Client,
) {
    let url = format!("{}/api/directory/announce", origin.trim_end_matches('/'));
    let mut last_seq = 0u64;
    let mut backoff = Duration::from_secs(10);
    loop {
        last_seq = now_ms().max(last_seq + 1);
        let claim = auth.active_claim();
        let record = build(&key, last_seq, &info, &ws_url, claim.as_deref());
        let announced = match http.post(&url).json(&record).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!("Announced to the directory (seq {last_seq})");
                backoff = Duration::from_secs(10);
                true
            }
            Ok(resp) => {
                warn!("Directory refused the announcement: HTTP {}", resp.status());
                false
            }
            Err(e) => {
                warn!("Directory unreachable: {e}");
                false
            }
        };

        // Wait for the next announcement. A new claim (installer re-run, --claim) is published within
        // seconds; until an owner exists we also refresh often so the claim window never lapses.
        let wait = if !announced {
            let w = backoff;
            backoff = (backoff * 2).min(Duration::from_secs(600));
            w
        } else if !auth.is_claimed() {
            UNCLAIMED_REFRESH
        } else {
            KEEPALIVE
        };
        wait_or_change(wait, || announced && auth.active_claim() != claim).await;
    }
}

/// Starts the publisher, unless `enabled` is false (LAN-only, no public address, or no directory): then the
/// directory is never contacted (GUARDRAIL-lan-addresses-never-reach-the-directory). True when it started.
pub fn spawn_publisher(
    enabled: bool,
    origin: String,
    key: Arc<ServerKey>,
    info: RtcInfo,
    auth: Arc<AuthState>,
    http: reqwest::Client,
) -> bool {
    if !enabled || origin.is_empty() {
        return false;
    }
    tokio::spawn(run(key, origin, info, String::new(), auth, http));
    true
}

/// Sleeps for `wait`, returning early as soon as `changed` reports true (checked every 5 s).
async fn wait_or_change(wait: Duration, changed: impl Fn() -> bool) {
    let started = tokio::time::Instant::now();
    while started.elapsed() < wait {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if changed() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    use std::sync::Arc;

    fn info() -> RtcInfo {
        RtcInfo {
            ip: "185.199.108.153".parse().unwrap(),
            port: 4443,
            fingerprint: format!("sha-256 {}", vec!["AB"; 32].join(":")),
        }
    }

    fn fixed_key() -> ServerKey {
        ServerKey::from_seed([7u8; 32])
    }

    fn verify(r: &Record) -> bool {
        let pk: [u8; 32] = B64.decode(&r.server_id).unwrap().try_into().unwrap();
        let sig: [u8; 64] = B64.decode(&r.sig).unwrap().try_into().unwrap();
        VerifyingKey::from_bytes(&pk)
            .unwrap()
            .verify(r.canonical().as_bytes(), &Signature::from_bytes(&sig))
            .is_ok()
    }

    #[test]
    fn records_verify_and_tampering_is_detected() {
        let r = build(&fixed_key(), 1_700_000_000_000, &info(), "", None);
        assert!(verify(&r));
        let mut forged = r.clone();
        forged.ip = "198.51.100.1".into();
        assert!(!verify(&forged));
        let mut replayed = r;
        replayed.seq += 1;
        assert!(!verify(&replayed));
    }

    #[test]
    fn claim_fields_are_signed_and_bind_the_secret_to_the_key() {
        let r = build(&fixed_key(), 5, &info(), "", Some("s3cret"));
        assert!(verify(&r));
        assert_eq!(r.claim_hash, claim_hash("s3cret"));
        assert_eq!(
            r.claim_proof,
            claim_proof("s3cret", &r.server_id, &r.fingerprint, &r.ip, r.udp_port)
        );
        assert_ne!(
            r.claim_proof,
            claim_proof("other", &r.server_id, &r.fingerprint, &r.ip, r.udp_port)
        );
    }

    /// Both sides read this file: the JS directory verifies it (directory.test.ts) and this test
    /// proves the Rust signer reproduces it byte for byte. Ed25519 signatures are deterministic.
    /// Regenerate with UPDATE_FIXTURE=1 cargo test -p termenbox-server fixture.
    #[test]
    fn the_signed_fixture_matches_what_the_directory_verifies() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/announce.json");
        let record = build(
            &fixed_key(),
            1_700_000_000_000,
            &info(),
            "https://x.trycloudflare.com",
            Some("fixture-secret"),
        );
        let json = serde_json::to_string_pretty(&record).unwrap() + "\n";
        if std::env::var("UPDATE_FIXTURE").is_ok() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &json).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
    }

    /// With a fake clock: an unchanged claimed server waits the whole week, a change ends the wait within
    /// seconds.
    #[tokio::test(start_paused = true)]
    async fn an_idle_claimed_server_announces_once_a_week_and_a_change_announces_at_once() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let changed = AtomicBool::new(false);
        let t0 = tokio::time::Instant::now();
        let day = Duration::from_secs(24 * 3600);
        let mut wait = Box::pin(wait_or_change(KEEPALIVE, || changed.load(Ordering::SeqCst)));
        tokio::select! {
            _ = &mut wait => panic!("announced before the keepalive"),
            _ = tokio::time::sleep(KEEPALIVE - day) => {}
        }
        changed.store(true, Ordering::SeqCst);
        wait.await;
        assert!(
            t0.elapsed() < KEEPALIVE,
            "a change must not wait for the keepalive"
        );

        let t1 = tokio::time::Instant::now();
        wait_or_change(KEEPALIVE, || false).await;
        let waited = t1.elapsed();
        assert!(waited >= KEEPALIVE && waited < KEEPALIVE + Duration::from_secs(10));
    }

    /// The announcer against a stand-in directory: records verify, `seq` only grows, and the claim
    /// fields appear exactly while a claim is active.
    #[tokio::test]
    async fn the_announcer_posts_signed_records_with_the_claim_while_it_is_active() {
        use axum::{extract::State, routing::post, Json, Router};
        use std::sync::Mutex;

        let seen: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(vec![]));
        async fn take(
            State(seen): State<Arc<Mutex<Vec<serde_json::Value>>>>,
            Json(v): Json<serde_json::Value>,
        ) {
            seen.lock().unwrap().push(v);
        }
        let app = Router::new()
            .route("/api/directory/announce", post(take))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let dir = std::env::temp_dir().join(format!("termenbox-ann-{}", uuid::Uuid::new_v4()));
        let auth = Arc::new(
            AuthState::new(dir.clone(), crate::cert::CertKeys::default(), "srv".into()).unwrap(),
        );
        auth.provide_claim("one-time").unwrap();
        let key = Arc::new(ServerKey::load_or_create(&dir).unwrap());
        let task = tokio::spawn(run(
            key.clone(),
            format!("http://127.0.0.1:{port}"),
            info(),
            "".into(),
            auth,
            reqwest::Client::new(),
        ));
        tokio::time::sleep(Duration::from_millis(500)).await;
        task.abort();

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let rec = &seen[0];
        assert_eq!(rec["server_id"], key.server_id());
        assert_eq!(rec["claim_hash"], claim_hash("one-time"));
        assert!(rec["seq"].as_u64().unwrap() > 1_700_000_000_000);
        let r = Record {
            server_id: rec["server_id"].as_str().unwrap().into(),
            seq: rec["seq"].as_u64().unwrap(),
            ip: rec["ip"].as_str().unwrap().into(),
            udp_port: rec["udp_port"].as_u64().unwrap() as u16,
            fingerprint: rec["fingerprint"].as_str().unwrap().into(),
            ws_url: rec["ws_url"].as_str().unwrap().into(),
            claim_hash: rec["claim_hash"].as_str().unwrap().into(),
            claim_proof: rec["claim_proof"].as_str().unwrap().into(),
            sig: rec["sig"].as_str().unwrap().into(),
        };
        assert!(verify(&r));
    }

    /// GUARDRAIL-lan-addresses-never-reach-the-directory: even with a bundle full of private candidates around, the
    /// record the directory gets has the fields it always had and none of those addresses.
    #[test]
    fn record_has_no_private_candidates() {
        use termenbox_core::bundle::CandidateKind;
        let list = crate::candidates::candidates(&crate::candidates::Inputs {
            port: 4443,
            lan: vec!["192.168.1.20".parse().unwrap()],
            overlay: vec!["100.64.1.2".parse().unwrap()],
            public4: Some("185.199.108.153".parse().unwrap()),
            ..Default::default()
        });
        assert!(list.iter().any(|c| c.kind == CandidateKind::Lan));
        assert!(list.iter().any(|c| c.kind == CandidateKind::Overlay));
        let record = build(&fixed_key(), 1, &info(), "", Some("s"));
        let json = serde_json::to_value(&record).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "claim_hash",
                "claim_proof",
                "fingerprint",
                "ip",
                "seq",
                "server_id",
                "sig",
                "udp_port",
                "ws_url"
            ]
        );
        let text = json.to_string();
        for c in list.iter().filter(|c| c.kind.is_private()) {
            assert!(!text.contains(&c.addr), "{} leaked into the record", c.addr);
        }
    }

    /// GUARDRAIL-lan-addresses-never-reach-the-directory: with LAN-only on, a recording stand-in directory sees no
    /// request at all, while the same call with LAN-only off does reach it (so the stand-in really records).
    #[tokio::test]
    async fn lan_only_makes_no_directory_calls() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let counter = hits.clone();
        tokio::spawn(async move {
            while listener.accept().await.is_ok() {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        let dir = std::env::temp_dir().join(format!("termenbox-lanonly-{}", uuid::Uuid::new_v4()));
        let auth = Arc::new(
            AuthState::new(dir.clone(), crate::cert::CertKeys::default(), "srv".into()).unwrap(),
        );
        let key = Arc::new(ServerKey::load_or_create(&dir).unwrap());

        let lan_only = true;
        let started = spawn_publisher(
            !lan_only,
            origin.clone(),
            key.clone(),
            info(),
            auth.clone(),
            reqwest::Client::new(),
        );
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!started);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "LAN-only reached the directory"
        );

        assert!(spawn_publisher(
            true,
            origin,
            key,
            info(),
            auth,
            reqwest::Client::new()
        ));
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            hits.load(Ordering::SeqCst) >= 1,
            "the control case never connected"
        );
    }

    #[test]
    fn the_server_key_is_stable_across_restarts() {
        let dir = std::env::temp_dir().join(format!("termenbox-key-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            ServerKey::load_or_create(&dir).unwrap().server_id(),
            ServerKey::load_or_create(&dir).unwrap().server_id()
        );
    }
}
