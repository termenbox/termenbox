use std::cell::{Cell as StdCell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

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
        }
    };
}

kinds!(
    ATTACHED, DETACHMENT, RESUME_PREPARED, RESUME_STARTED, RESUME_REJECTED,
    RESUME_VERIFIED, RESUME_FAILED, HUMAN_PENDING, HUMAN_RESOLVED,
    HUMAN_TIMEOUT, DETACH_TIMEOUT, AUTHORITY_EXTENSION_REJECTED,
    AUTHORITY_NARROWED, AUTHORITY_NARROWING_REJECTED, RESUME_TOKEN_EXPIRED,
    TERMINATION, TOOL_CALL_QUEUED, TOOL_CALL_DRAINED, TOOL_CALL_REJECTED,
    TOOL_CALL_EXPIRED, BRIDGE_RECOVERED, RECOVERY_FAILED,
);

/// A governance event, hash-chained to its predecessor (I5).
#[derive(Debug, Clone)]
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

/// The resume token R of Definition 4.2 (ten fields; `mac` covers the rest).
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeToken {
    pub session_id: String,
    pub sequence: u64,
    pub event_hash: String,
    pub authority_scope: Vec<String>,
    pub scope_version: u64,
    /// (idempotency key, call)
    pub pending_tool_calls: Vec<(String, String)>,
    pub reason: String,
    pub ttl_seconds: f64,
    pub client_key_id: String,
    pub mac: String,
}

impl ResumeToken {
    /// Every field except `mac`: the MAC input.
    pub fn fields(&self) -> Value {
        let calls: Vec<Value> = self
            .pending_tool_calls
            .iter()
            .map(|(k, c)| json!({"key": k, "call": c}))
            .collect();
        json!({
            "session_id": self.session_id,
            "sequence": self.sequence,
            "event_hash": self.event_hash,
            "authority_scope": self.authority_scope,
            "scope_version": self.scope_version,
            "pending_tool_calls": calls,
            "reason": self.reason,
            "ttl_seconds": self.ttl_seconds,
            "client_key_id": self.client_key_id,
        })
    }
}

type HmacSha256 = Hmac<Sha256>;

pub(crate) fn hmac_hex(key: &[u8], msg: &[u8]) -> String {
    let mut m = HmacSha256::new_from_slice(key).expect("any key length");
    m.update(msg);
    hex::encode(m.finalize().into_bytes())
}

/// Client key registered at attach (I11). HMAC stands in for a signature
/// scheme so the crate needs no asymmetric primitives; see vectors/README.md.
#[derive(Debug, Clone)]
pub struct ClientKey {
    pub key_id: String,
    pub secret: Vec<u8>,
}

impl ClientKey {
    pub fn new(key_id: &str, secret: &[u8]) -> Self {
        ClientKey { key_id: key_id.to_string(), secret: secret.to_vec() }
    }

    /// Proof of possession over the bridge nonce and the token sequence.
    pub fn sign(&self, nonce: &str, sequence: u64) -> String {
        hmac_hex(&self.secret, format!("{nonce}|{sequence}").as_bytes())
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub t_detach: f64,
    pub t_human: f64,
    pub t_token: f64,
    pub t_call: f64,
    pub t_approve: f64,
    pub q_max: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            t_detach: 1800.0,
            t_human: 86_400.0,
            t_token: 900.0,
            t_call: 3600.0,
            t_approve: 900.0,
            q_max: 64,
        }
    }
}

pub trait Clock {
    fn now(&self) -> f64;
}

/// Manually advanced clock for deterministic tests.
#[derive(Debug, Default)]
pub struct FakeClock {
    t: StdCell<f64>,
}

impl FakeClock {
    pub fn new(start: f64) -> Rc<FakeClock> {
        Rc::new(FakeClock { t: StdCell::new(start) })
    }

    pub fn advance(&self, seconds: f64) {
        self.t.set(self.t.get() + seconds);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> f64 {
        self.t.get()
    }
}

/// Durable medium (axiom A1). It outlives a bridge instance; a new
/// [`crate::Bridge`] over the same storage is a restart.
#[derive(Debug)]
pub struct Storage {
    pub logs: HashMap<String, Vec<Event>>,
    pub mac_key: Option<Vec<u8>>,
    pub client_keys: HashMap<String, Vec<u8>>,
    /// Test hook: refuse the next append.
    pub fail_next: bool,
}

impl Storage {
    pub fn new(mac_key: &[u8]) -> Rc<RefCell<Storage>> {
        Rc::new(RefCell::new(Storage {
            logs: HashMap::new(),
            mac_key: Some(mac_key.to_vec()),
            client_keys: HashMap::new(),
            fail_next: false,
        }))
    }

    /// Atomic: all events of a transition are committed, or none (I12).
    pub fn append_batch(&mut self, sid: &str, events: Vec<Event>) -> Result<()> {
        if self.fail_next {
            self.fail_next = false;
            return Err(BridgeError::new("storage: append refused"));
        }
        self.logs.entry(sid.to_string()).or_default().extend(events);
        Ok(())
    }
}

pub(crate) fn canon(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).expect("json serialises")
}

pub(crate) fn authority_hash(principal: &str, scope: &[String], lineage: &[String]) -> String {
    let mut m = Map::new();
    m.insert("principal".into(), json!(principal));
    m.insert("scope".into(), json!(scope));
    m.insert("lineage".into(), json!(lineage));
    hex::encode(Sha256::digest(canon(&Value::Object(m))))[..16].to_string()
}
