use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeError(pub String);

impl BridgeError {
    pub fn new(msg: impl Into<String>) -> Self {
        BridgeError(msg.into())
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for BridgeError {}

pub type Result<T> = std::result::Result<T, BridgeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attachment {
    Att,
    Det,
    Res,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Human {
    None,
    Pend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Life {
    Active,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub att: Attachment,
    pub human: Human,
    pub life: Life,
}

impl Cell {
    /// The names used by the test vectors: `["Att","None"]` or `["Terminated"]`.
    pub fn names(&self) -> Vec<&'static str> {
        if self.life == Life::Terminated {
            return vec!["Terminated"];
        }
        vec![
            match self.att {
                Attachment::Att => "Att",
                Attachment::Det => "Det",
                Attachment::Res => "Res",
            },
            match self.human {
                Human::None => "None",
                Human::Pend => "Pend",
            },
        ]
    }
}

macro_rules! kinds {
    ($($v:ident),+ $(,)?) => {
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Kind { $($v),+ }
        impl Kind {
            pub fn as_str(&self) -> &'static str {
                match self { $(Kind::$v => stringify!($v)),+ }
            }
            pub fn parse(s: &str) -> Option<Kind> {
                match s { $(stringify!($v) => Some(Kind::$v),)+ _ => None }
            }
        }
    };
}

kinds!(
    ATTACHED, CLIENT_JOINED, CLIENT_LEFT, CLIENT_ADDED, DETACHMENT, RESUME_PREPARED,
    RESUME_STARTED, RESUME_REJECTED, RESUME_VERIFIED, RESUME_FAILED, HUMAN_PENDING,
    HUMAN_RESOLVED, HUMAN_TIMEOUT, DETACH_TIMEOUT, AUTHORITY_EXTENSION_REJECTED,
    AUTHORITY_NARROWED, AUTHORITY_NARROWING_REJECTED, TERMINATION, TOOL_CALL_QUEUED,
    TOOL_CALL_ANSWERED, TOOL_CALL_DRAINED, TOOL_CALL_REJECTED, TOOL_CALL_EXPIRED,
    BRIDGE_RECOVERED, RECOVERY_FAILED,
);

/// A governance event, hash-chained to its predecessor (I5).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub kind: Kind,
    pub session_id: String,
    pub sequence: u64,
    pub prev_hash: String,
    pub payload: Value,
    pub timestamp: f64,
}

impl Event {
    /// Canonical JSON: sorted keys, no whitespace (serde_json maps are ordered).
    pub fn canonical(&self) -> Vec<u8> {
        let v = json!({
            "kind": self.kind.as_str(),
            "session_id": self.session_id,
            "sequence": self.sequence,
            "prev_hash": self.prev_hash,
            "payload": self.payload,
            "timestamp": self.timestamp,
        });
        serde_json::to_vec(&v).expect("event serialises")
    }

    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(self.canonical()))
    }

    pub fn str_field(&self, name: &str) -> Option<&str> {
        self.payload.get(name).and_then(Value::as_str)
    }
}

#[derive(Debug, Clone)]
pub struct SessionIdentity {
    pub session_id: String,
    pub principal_id: String,
    pub authority_scope: Vec<String>,
    pub task_lineage: Vec<String>,
}

/// A fresh single-use nonce (I11b).
#[derive(Debug, Clone, PartialEq)]
pub struct Challenge {
    pub nonce: String,
    pub ttl_seconds: f64,
}

/// A re-attach or join request (I4, I11): the last event the client
/// acknowledged, a bridge nonce, and a signature over all of them.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeRequest {
    pub key_id: String,
    pub nonce: String,
    pub seq: i64,
    pub hash: String,
    pub signature: String,
}

/// The message a client signs: canonical JSON of `[sid, nonce, seq, hash]`.
pub fn request_message(sid: &str, nonce: &str, seq: i64, hash: &str) -> Vec<u8> {
    serde_json::to_vec(&json!([sid, nonce, seq, hash])).expect("json serialises")
}

type HmacSha256 = Hmac<Sha256>;

pub(crate) fn hmac_hex(key: &[u8], msg: &[u8]) -> String {
    let mut m = HmacSha256::new_from_slice(key).expect("any key length");
    m.update(msg);
    hex::encode(m.finalize().into_bytes())
}

/// Client key (I11). HMAC stands in for a signature scheme so the crate
/// needs no asymmetric primitives; see `../vectors/README.md`. A consumer
/// supplies its own [`Verifier`] for real signatures.
#[derive(Debug, Clone)]
pub struct ClientKey {
    pub key_id: String,
    pub secret: Vec<u8>,
}

impl ClientKey {
    pub fn new(key_id: &str, secret: &[u8]) -> Self {
        ClientKey { key_id: key_id.to_string(), secret: secret.to_vec() }
    }

    pub fn sign(&self, sid: &str, nonce: &str, seq: i64, hash: &str) -> String {
        hmac_hex(&self.secret, &request_message(sid, nonce, seq, hash))
    }

    pub fn request(&self, sid: &str, nonce: &str, seq: i64, hash: &str) -> ResumeRequest {
        ResumeRequest {
            key_id: self.key_id.clone(),
            nonce: nonce.to_string(),
            seq,
            hash: hash.to_string(),
            signature: self.sign(sid, nonce, seq, hash),
        }
    }
}

/// Proof-of-possession check (I11c). `key_material` is what the registry
/// stored for the key at registration: the HMAC secret for the stand-in,
/// a public key for a real scheme.
pub trait Verifier: Send + Sync {
    fn verify(&self, key_material: &[u8], message: &[u8], signature: &str) -> bool;
}

/// The test default: `signature == hex(HMAC-SHA256(key_material, message))`.
#[derive(Debug, Default, Clone, Copy)]
pub struct HmacVerifier;

impl Verifier for HmacVerifier {
    fn verify(&self, key_material: &[u8], message: &[u8], signature: &str) -> bool {
        let want = hmac_hex(key_material, message);
        // constant-time comparison is not needed for a test stand-in, but
        // keep the length check explicit
        want.len() == signature.len()
            && want.bytes().zip(signature.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
    }
}

/// Timers in seconds. `T_call <= T_human <= T_detach` is a MUST; construct
/// through [`Config::new`] or a profile so the bounds are checked, and
/// [`crate::Bridge::new`] re-checks them.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub t_detach: f64,
    pub t_human: f64,
    pub t_call: f64,
    pub t_approve: f64,
    pub t_nonce: f64,
    pub q_max: usize,
}

impl Config {
    /// `t_human` and `t_call` default to `t_detach`.
    pub fn new(
        t_detach: f64,
        t_human: Option<f64>,
        t_call: Option<f64>,
        t_approve: f64,
        t_nonce: f64,
        q_max: usize,
    ) -> Result<Config> {
        let c = Config {
            t_detach,
            t_human: t_human.unwrap_or(t_detach),
            t_call: t_call.unwrap_or(t_detach),
            t_approve,
            t_nonce,
            q_max,
        };
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&self) -> Result<()> {
        if self.t_call <= self.t_human && self.t_human <= self.t_detach {
            Ok(())
        } else {
            Err(BridgeError::new("timer bounds: T_call <= T_human <= T_detach is a MUST"))
        }
    }

    /// 30 minutes everywhere (the default profile).
    pub fn interactive_short() -> Config {
        Config { t_detach: 1800.0, t_human: 1800.0, t_call: 1800.0, t_approve: 900.0, t_nonce: 60.0, q_max: 64 }
    }

    /// 7 days everywhere; approvals still expire after 15 minutes.
    pub fn persistent() -> Config {
        let d = 7.0 * 86_400.0;
        Config { t_detach: d, t_human: d, t_call: d, ..Config::interactive_short() }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::interactive_short()
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> f64;
}

/// Wall clock for production use.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
    }
}

/// Manually advanced clock for deterministic tests.
#[derive(Debug, Default)]
pub struct FakeClock {
    bits: AtomicU64,
}

impl FakeClock {
    pub fn new(start: f64) -> Arc<FakeClock> {
        Arc::new(FakeClock { bits: AtomicU64::new(start.to_bits()) })
    }

    pub fn advance(&self, seconds: f64) {
        let t = f64::from_bits(self.bits.load(Ordering::SeqCst)) + seconds;
        self.bits.store(t.to_bits(), Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> f64 {
        f64::from_bits(self.bits.load(Ordering::SeqCst))
    }
}

/// Durable medium (axiom A1). It outlives a bridge instance; a new
/// [`crate::Bridge`] over the same storage is a restart. All methods take
/// `&self`: an implementation synchronises internally.
pub trait Storage: Send + Sync {
    /// Atomic: all events of a transition are committed, or none (I12).
    /// Returns only once they are durable.
    fn append_batch(&self, sid: &str, events: Vec<Event>) -> Result<()>;
    /// The whole committed log of a session, in order (empty if unknown).
    fn read_log(&self, sid: &str) -> Result<Vec<Event>>;
    /// Every session that has a log.
    fn sessions(&self) -> Result<Vec<String>>;
    /// Register the verification material of a client key (I15d).
    fn register_key(&self, key_id: &str, material: &[u8]) -> Result<()>;
    fn key_material(&self, key_id: &str) -> Result<Option<Vec<u8>>>;
}

/// In-memory storage: the reference implementation of [`Storage`].
#[derive(Debug, Default)]
pub struct MemStorage {
    logs: Mutex<HashMap<String, Vec<Event>>>,
    keys: Mutex<HashMap<String, Vec<u8>>>,
    /// Test hook: refuse the next append.
    pub fail_next: AtomicBool,
}

impl MemStorage {
    pub fn new() -> Arc<MemStorage> {
        Arc::new(MemStorage::default())
    }

    /// Test hook: edit a stored event in place.
    pub fn tamper(&self, sid: &str, index: usize, f: impl FnOnce(&mut Event)) {
        f(&mut self.logs.lock().unwrap().get_mut(sid).unwrap()[index]);
    }

    /// Test hook: forget every registered key.
    pub fn forget_keys(&self) {
        self.keys.lock().unwrap().clear();
    }
}

impl Storage for MemStorage {
    fn append_batch(&self, sid: &str, events: Vec<Event>) -> Result<()> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(BridgeError::new("storage: append refused"));
        }
        self.logs.lock().unwrap().entry(sid.to_string()).or_default().extend(events);
        Ok(())
    }

    fn read_log(&self, sid: &str) -> Result<Vec<Event>> {
        Ok(self.logs.lock().unwrap().get(sid).cloned().unwrap_or_default())
    }

    fn sessions(&self) -> Result<Vec<String>> {
        Ok(self.logs.lock().unwrap().keys().cloned().collect())
    }

    fn register_key(&self, key_id: &str, material: &[u8]) -> Result<()> {
        self.keys.lock().unwrap().insert(key_id.to_string(), material.to_vec());
        Ok(())
    }

    fn key_material(&self, key_id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.keys.lock().unwrap().get(key_id).cloned())
    }
}

pub(crate) fn canon(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).expect("json serialises")
}

/// h(x): SHA-256 of the canonical JSON of `x` (I14).
pub fn content_hash(v: &Value) -> String {
    hex::encode(Sha256::digest(canon(v)))
}

pub(crate) fn digest(nonce: &str) -> String {
    hex::encode(Sha256::digest(nonce.as_bytes()))[..16].to_string()
}

pub(crate) fn authority_hash(principal: &str, scope: &[String], lineage: &[String]) -> String {
    let mut m = Map::new();
    m.insert("principal".into(), json!(principal));
    m.insert("scope".into(), json!(scope));
    m.insert("lineage".into(), json!(lineage));
    hex::encode(Sha256::digest(canon(&Value::Object(m))))[..16].to_string()
}
