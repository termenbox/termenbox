//! The extension registry (decision 0136, rules checked in fact 0057).
//!
//! The wire is ACP v1 verbatim. Whatever termenbox adds rides in `_meta` under a `termenbox.dev/<name>` key, or as a
//! custom method named `_termenbox.dev/<name>`; nothing is ever added beside the standard fields (the schema crate
//! drops root extras and the specification forbids them). Each side lists what it implements in the `_meta` of its
//! `initialize` capabilities; only what both sides list is used, at the lower version. A peer that knows nothing of
//! this (any plain ACP agent) is simply spoken to in plain ACP.
//!
//! Shared vectors in `testdata/extension-vectors.json`; the TypeScript side is `packages/client/src/lib/extensions.ts`.
use std::collections::BTreeMap;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};

/// The version of the termenbox protocol: ACP v1 core plus this registry. A change to a core shape bumps it.
pub const PROTOCOL_VERSION: u32 = 2;
/// The namespace; also the `_meta` key of the capability object that holds the offer.
pub const NAMESPACE: &str = "termenbox.dev";
/// Prefix of an extension's `_meta` key.
const KEY_PREFIX: &str = "termenbox.dev/";
/// Prefix of an extension method (ACP reserves every name starting with `_` for extensions).
const METHOD_PREFIX: &str = "_termenbox.dev/";
/// JSON-RPC "Method not found", what ACP says an unknown request gets.
pub const METHOD_NOT_FOUND: i64 = -32601;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExtError {
    #[error("extension {0} is present but malformed: {1}")]
    Malformed(&'static str, String),
}

/// One extension: a name, the version of its shape, and the shape itself.
pub trait Extension: Serialize + DeserializeOwned {
    /// Short name; the `_meta` key is `termenbox.dev/<NAME>`.
    const NAME: &'static str;
    const VERSION: u32;
}

/// The `_meta` key of an extension.
pub fn key(name: &str) -> String {
    format!("{KEY_PREFIX}{name}")
}

/// The extension method name for `name`.
pub fn method(name: &str) -> String {
    format!("{METHOD_PREFIX}{name}")
}

/// What this side implements: extension name -> highest version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    versions: BTreeMap<String, u32>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with<E: Extension>(self) -> Self {
        self.with_name(E::NAME, E::VERSION)
    }

    pub fn with_name(mut self, name: &str, version: u32) -> Self {
        self.versions.insert(name.to_string(), version);
        self
    }

    /// The offer, as the value under `_meta["termenbox.dev"]` of the `initialize` capabilities.
    pub fn offer(&self) -> Value {
        serde_json::json!({ "protocol": PROTOCOL_VERSION, "extensions": self.versions })
    }

    /// Writes the offer into a capability object's `_meta`.
    pub fn advertise(&self, capability_meta: &mut Map<String, Value>) {
        capability_meta.insert(NAMESPACE.to_string(), self.offer());
    }

    /// Reads the other side's offer from its capability `_meta` and keeps what both sides know. A missing or
    /// unreadable offer is a plain ACP peer; an unreadable entry is skipped alone.
    pub fn negotiate(&self, remote_capability_meta: Option<&Map<String, Value>>) -> Negotiated {
        let remote = remote_capability_meta.and_then(|m| m.get(NAMESPACE));
        let Some(protocol) = remote
            .and_then(|o| o.get("protocol"))
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok())
        else {
            return Negotiated::default();
        };
        let mut versions = BTreeMap::new();
        if let Some(theirs) = remote
            .and_then(|o| o.get("extensions"))
            .and_then(Value::as_object)
        {
            for (name, ours) in &self.versions {
                if let Some(v) = theirs
                    .get(name)
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                {
                    versions.insert(name.clone(), v.min(*ours));
                }
            }
        }
        Negotiated {
            protocol: protocol.min(PROTOCOL_VERSION),
            versions,
        }
    }
}

/// The outcome of `initialize`: what may be used on this connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Negotiated {
    /// 0 for a plain ACP peer.
    pub protocol: u32,
    versions: BTreeMap<String, u32>,
}

impl Negotiated {
    pub fn version(&self, name: &str) -> Option<u32> {
        self.versions.get(name).copied()
    }

    pub fn supports(&self, name: &str) -> bool {
        self.versions.contains_key(name)
    }
}

/// The `seq` extension (decision 0139): the position of a `session/update` in its session's log and the server run
/// (`epoch`) that numbered it. A client that holds a different epoch starts over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeqStamp {
    pub seq: u64,
    pub epoch: u64,
}

impl Extension for SeqStamp {
    const NAME: &'static str = "seq";
    const VERSION: u32 = 1;
}

/// The `reset` extension: set on a replay that is not the tail of what the client holds but a compacted rebuild of
/// the whole session, so the client drops its copy first. It covers everything up to and including `last`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reset {
    pub last: u64,
}

impl Extension for Reset {
    const NAME: &'static str = "reset";
    const VERSION: u32 = 1;
}

/// A `_meta` object. Wraps the raw map so that keys of other namespaces (`zed.dev/...`, `traceparent`) travel
/// through untouched while ours are read and written by type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExtMeta(pub Map<String, Value>);

impl ExtMeta {
    /// The extension `E` if the peer negotiated it and sent it. Not negotiated, or absent: `None` (skipped). Present
    /// but of the wrong shape: an error the caller handles by dropping this extension alone.
    pub fn get<E: Extension>(&self, negotiated: &Negotiated) -> Result<Option<E>, ExtError> {
        if !negotiated.supports(E::NAME) {
            return Ok(None);
        }
        match self.0.get(&key(E::NAME)) {
            None => Ok(None),
            Some(v) => serde_json::from_value(v.clone())
                .map(Some)
                .map_err(|e| ExtError::Malformed(E::NAME, e.to_string())),
        }
    }

    /// Adds `ext` if the peer negotiated it; returns whether it was written. Never fails for a peer that lacks it.
    pub fn put<E: Extension>(&mut self, ext: &E, negotiated: &Negotiated) -> bool {
        if !negotiated.supports(E::NAME) {
            return false;
        }
        match serde_json::to_value(ext) {
            Ok(v) => {
                self.0.insert(key(E::NAME), v);
                true
            }
            Err(_) => false,
        }
    }

    /// Removes every extension key the peer did not negotiate (the send direction of "unknown is skipped"). The offer
    /// itself and other namespaces stay.
    pub fn strip_unnegotiated(&mut self, negotiated: &Negotiated) {
        self.0.retain(|k, _| match k.strip_prefix(KEY_PREFIX) {
            Some(name) => negotiated.supports(name),
            None => true,
        });
    }

    /// Removes every key of our namespace: what is left must be valid plain ACP (guardrail `protocol-core-stays-acp-v1`).
    pub fn strip_all(&mut self) {
        self.0
            .retain(|k, _| !k.starts_with(KEY_PREFIX) && k != NAMESPACE);
    }
}

/// What to do with an incoming method name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// A standard ACP method: the core handles it.
    Core,
    /// A negotiated extension method of ours.
    Extension(String),
    /// An unknown extension request: answer with `METHOD_NOT_FOUND`.
    Reject,
    /// An unknown extension notification: ignore it.
    Skip,
}

pub fn route(method_name: &str, is_request: bool, negotiated: &Negotiated) -> Route {
    if !method_name.starts_with('_') {
        return Route::Core;
    }
    if let Some(name) = method_name.strip_prefix(METHOD_PREFIX) {
        if negotiated.supports(name) {
            return Route::Extension(name.to_string());
        }
    }
    if is_request {
        Route::Reject
    } else {
        Route::Skip
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Seq {
        n: u64,
    }
    impl Extension for Seq {
        const NAME: &'static str = "seq";
        const VERSION: u32 = 2;
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Tier {
        tier: String,
    }
    impl Extension for Tier {
        const NAME: &'static str = "tier";
        const VERSION: u32 = 1;
    }

    fn meta(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn both() -> Negotiated {
        let us = Registry::new().with::<Seq>().with::<Tier>();
        let mut caps = Map::new();
        us.advertise(&mut caps);
        us.negotiate(Some(&caps))
    }

    #[test]
    fn a_known_extension_round_trips() {
        let n = both();
        let mut m = ExtMeta::default();
        assert!(m.put(&Seq { n: 9 }, &n));
        let wire = serde_json::to_string(&m).unwrap();
        assert_eq!(wire, r#"{"termenbox.dev/seq":{"n":9}}"#);
        let back: ExtMeta = serde_json::from_str(&wire).unwrap();
        assert_eq!(back.get::<Seq>(&n), Ok(Some(Seq { n: 9 })));
    }

    #[test]
    fn an_unknown_extension_is_skipped_when_reading() {
        // The peer sent an extension this side never negotiated, and one of another namespace: neither is an error.
        let m = ExtMeta(meta(
            json!({"termenbox.dev/hologram": {"x": 1}, "zed.dev/debugMode": true, "termenbox.dev/seq": {"n": 1}}),
        ));
        let only_tier = Registry::new().with::<Tier>().negotiate(Some(&meta(json!({
            "termenbox.dev": {"protocol": 1, "extensions": {"tier": 1}}
        }))));
        assert_eq!(m.get::<Seq>(&only_tier), Ok(None));
        assert_eq!(m.get::<Tier>(&only_tier), Ok(None));
    }

    #[test]
    fn an_unknown_extension_is_skipped_when_writing() {
        // The peer is plain ACP: nothing of ours is written, and the call does not fail.
        let plain = Registry::new().with::<Seq>().negotiate(None);
        let mut m = ExtMeta::default();
        assert!(!m.put(&Seq { n: 1 }, &plain));
        assert!(m.0.is_empty());
        // Leftovers of ours that the peer did not negotiate are removed; other namespaces stay.
        let mut m = ExtMeta(meta(
            json!({"termenbox.dev/seq": {"n": 1}, "termenbox.dev/tier": {"tier": "T0"}, "zed.dev/x": 1, "traceparent": "t"}),
        ));
        let seq_only = Registry::new().with::<Seq>().negotiate(Some(&meta(json!({
            "termenbox.dev": {"protocol": 1, "extensions": {"seq": 1}}
        }))));
        m.strip_unnegotiated(&seq_only);
        // Compared as sets: another crate in the workspace turns on serde_json's `preserve_order`.
        let mut keys = m.0.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys, ["termenbox.dev/seq", "traceparent", "zed.dev/x"]);
    }

    #[test]
    fn a_malformed_known_extension_is_an_error_for_that_extension_alone() {
        let n = both();
        let m = ExtMeta(meta(
            json!({"termenbox.dev/seq": {"n": "nine"}, "termenbox.dev/tier": {"tier": "T0"}}),
        ));
        assert!(matches!(
            m.get::<Seq>(&n),
            Err(ExtError::Malformed("seq", _))
        ));
        assert_eq!(m.get::<Tier>(&n), Ok(Some(Tier { tier: "T0".into() })));
    }

    #[test]
    fn stripping_every_extension_leaves_other_keys_alone() {
        let mut m = ExtMeta(meta(
            json!({"termenbox.dev": {"protocol": 1}, "termenbox.dev/seq": {"n": 1}, "zed.dev/x": 1}),
        ));
        m.strip_all();
        assert_eq!(m.0, meta(json!({"zed.dev/x": 1})));
    }

    #[test]
    fn negotiation_takes_the_lower_version_and_what_both_know() {
        let us = Registry::new().with::<Seq>().with::<Tier>(); // seq 2, tier 1
        let n = us.negotiate(Some(&meta(json!({
            "termenbox.dev": {"protocol": 3, "extensions": {"seq": 1, "tier": 5, "window": 1}}
        }))));
        assert_eq!(n.protocol, PROTOCOL_VERSION);
        assert_eq!(
            (n.version("seq"), n.version("tier"), n.version("window")),
            (Some(1), Some(1), None)
        );
    }

    #[test]
    fn no_offer_or_an_unreadable_one_is_a_plain_acp_peer() {
        let us = Registry::new().with::<Seq>();
        for caps in [
            None,
            Some(meta(json!({}))),
            Some(meta(json!({"termenbox.dev": "yes"}))),
            Some(meta(json!({"zed.dev": {"protocol": 1}}))),
        ] {
            let n = us.negotiate(caps.as_ref());
            assert_eq!(n, Negotiated::default());
            assert!(!n.supports("seq"));
        }
        // An unreadable entry costs only that entry.
        let n = us.negotiate(Some(&meta(
            json!({"termenbox.dev": {"protocol": 1, "extensions": {"seq": "two"}}}),
        )));
        assert_eq!((n.protocol, n.supports("seq")), (1, false));
    }

    #[test]
    fn methods_route_by_name_and_kind() {
        let n = both();
        assert_eq!(route("session/prompt", true, &n), Route::Core);
        assert_eq!(
            route(&method("seq"), true, &n),
            Route::Extension("seq".into())
        );
        assert_eq!(route(&method("window"), true, &n), Route::Reject);
        assert_eq!(route(&method("window"), false, &n), Route::Skip);
        assert_eq!(route("_zed.dev/x", true, &n), Route::Reject);
        assert_eq!(route("_zed.dev/x", false, &n), Route::Skip);
        assert_eq!(
            route(&method("seq"), true, &Negotiated::default()),
            Route::Reject
        );
    }

    /// The shared vectors: the TypeScript client must give the same answers.
    #[test]
    fn rust_and_typescript_agree_on_the_shared_vectors() {
        let v: Value =
            serde_json::from_str(include_str!("../testdata/extension-vectors.json")).unwrap();
        for c in v["negotiate"].as_array().unwrap() {
            let mut us = Registry::new();
            for (name, ver) in c["local"].as_object().unwrap() {
                us = us.with_name(name, ver.as_u64().unwrap() as u32);
            }
            let remote = c["remote_meta"].as_object();
            let n = us.negotiate(remote);
            assert_eq!(
                n.protocol as u64,
                c["expect"]["protocol"].as_u64().unwrap(),
                "{}",
                c["name"]
            );
            let got: Map<String, Value> = n
                .versions
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            assert_eq!(Value::Object(got), c["expect"]["versions"], "{}", c["name"]);
        }
        for c in v["route"].as_array().unwrap() {
            let mut n = Negotiated::default();
            for name in c["supports"].as_array().unwrap() {
                n.versions.insert(name.as_str().unwrap().into(), 1);
            }
            let got = match route(
                c["method"].as_str().unwrap(),
                c["request"].as_bool().unwrap(),
                &n,
            ) {
                Route::Core => "core".to_string(),
                Route::Extension(name) => format!("extension:{name}"),
                Route::Reject => "reject".into(),
                Route::Skip => "skip".into(),
            };
            assert_eq!(got, c["expect"].as_str().unwrap(), "{}", c["name"]);
        }
        for c in v["strip"].as_array().unwrap() {
            let mut n = Negotiated::default();
            for name in c["negotiated"].as_array().unwrap() {
                n.versions.insert(name.as_str().unwrap().into(), 1);
            }
            let mut a = ExtMeta(c["meta"].as_object().unwrap().clone());
            let mut b = a.clone();
            a.strip_unnegotiated(&n);
            b.strip_all();
            assert_eq!(
                Value::Object(a.0),
                c["unnegotiated_stripped"],
                "{}",
                c["name"]
            );
            assert_eq!(Value::Object(b.0), c["all_stripped"], "{}", c["name"]);
        }
    }
}
