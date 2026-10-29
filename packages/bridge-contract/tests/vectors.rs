//! Runs the language-neutral vectors in `vectors/*.json` (schema in
//! `vectors/README.md`) against this crate.

mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bridge_contract::*;
use serde_json::Value;

/// A [`Storage`] plus the hooks the vectors need to damage it.
trait TestStorage: Storage {
    fn tamper_prev_hash(&self, sid: &str, index: usize);
    fn fail_next(&self);
}

impl TestStorage for MemStorage {
    fn tamper_prev_hash(&self, sid: &str, index: usize) {
        self.tamper(sid, index, |e| e.prev_hash = "00".repeat(32));
    }
    fn fail_next(&self) {
        self.fail_next
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl TestStorage for common::FileStorage {
    fn tamper_prev_hash(&self, sid: &str, index: usize) {
        common::FileStorage::tamper_prev_hash(self, sid, index);
    }
    fn fail_next(&self) {
        common::FileStorage::fail_next(self);
    }
}

struct Runner {
    storage: Arc<dyn TestStorage>,
    clock: Arc<FakeClock>,
    cfg: Config,
    bridge: Bridge,
    sid: String,
    first: String,
    keys: HashMap<String, ClientKey>,
    seen: HashMap<String, (i64, String)>,
    nonces: HashMap<String, String>,
}

fn config_of(v: &Value) -> Result<Config> {
    let mut c = (
        Config::default().t_detach,
        None,
        None,
        None,
        900.0,
        60.0,
        64usize,
    );
    let (mut policy, mut allow) = (DetachedCalls::Refuse, Vec::<String>::new());
    if let Some(m) = v.as_object() {
        for (k, x) in m {
            match k.as_str() {
                "t_detach" => c.0 = x.as_f64().unwrap(),
                "t_human" => c.1 = Some(x.as_f64().unwrap()),
                "t_call" => c.2 = Some(x.as_f64().unwrap()),
                "t_total" => c.3 = Some(x.as_f64().unwrap()),
                "t_approve" => c.4 = x.as_f64().unwrap(),
                "t_nonce" => c.5 = x.as_f64().unwrap(),
                "q_max" => c.6 = x.as_u64().unwrap() as usize,
                "detached_calls" => {
                    policy = match x.as_str().unwrap() {
                        "refuse" => DetachedCalls::Refuse,
                        "queue" => DetachedCalls::Queue,
                        "forward" => DetachedCalls::Forward,
                        other => panic!("unknown detached_calls {other}"),
                    }
                }
                "detached_allow" => allow = strings(x),
                other => panic!("unknown config key {other}"),
            }
        }
    }
    Ok(Config::new(c.0, c.1, c.2, c.3, c.4, c.5, c.6)?.with_detached_calls(policy, allow))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

fn secret_of(key_id: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(format!("client-seed-{key_id}")).into()
}

impl Runner {
    fn new(vec: &Value, storage: Arc<dyn TestStorage>) -> Result<Runner> {
        let cfg = config_of(&vec["config"])?;
        let clock = FakeClock::new(1000.0);
        let s = &vec["session"];
        let first = s["client_key_id"].as_str().unwrap().to_string();
        let key = ClientKey::new(&first, &secret_of(&first));
        let mut bridge = Bridge::new(
            storage.clone(),
            clock.clone(),
            Arc::new(Ed25519Verifier),
            cfg.clone(),
        )?;
        let sid = s["session_id"].as_str().unwrap().to_string();
        bridge.attach(
            &SessionIdentity {
                session_id: sid.clone(),
                principal_id: s["principal_id"].as_str().unwrap().to_string(),
                authority_scope: strings(&s["authority_scope"]),
                task_lineage: vec![],
            },
            &key,
        )?;
        let mut r = Runner {
            storage,
            clock,
            cfg,
            bridge,
            sid,
            first: first.clone(),
            keys: HashMap::from([(first.clone(), key)]),
            seen: HashMap::new(),
            nonces: HashMap::new(),
        };
        r.sync(&first);
        Ok(r)
    }

    fn cell(&self) -> Vec<&'static str> {
        self.bridge.cell_of(&self.sid).names()
    }

    fn tip(&self) -> (i64, String) {
        let ev = self.bridge.events(&self.sid).pop().unwrap();
        (ev.sequence as i64, ev.hash())
    }

    fn sync(&mut self, client: &str) {
        let t = self.tip();
        self.seen.insert(client.to_string(), t);
    }

    fn key(&mut self, client: &str) -> ClientKey {
        self.keys
            .entry(client.to_string())
            .or_insert_with(|| ClientKey::new(client, &secret_of(client)))
            .clone()
    }

    fn request(&mut self, a: &Value) -> Result<ResumeRequest> {
        let client = a["client"].as_str().unwrap_or(&self.first).to_string();
        let key = self.key(&client);
        let nonce = match a["nonce"].as_str().unwrap_or("fresh") {
            "fresh" => self.bridge.challenge(&self.sid)?.nonce,
            "unknown" => "ab".repeat(16),
            name => self.nonces[name].clone(),
        };
        if let Some(n) = a["as"].as_str() {
            self.nonces.insert(n.to_string(), nonce.clone());
        }
        let (tip_seq, tip_hash) = self.tip();
        let genesis = self.bridge.events(&self.sid)[0].hash();
        let (mut seq, mut h) = self.seen.get(&client).cloned().unwrap_or((0, genesis));
        match a["prefix"].as_str().unwrap_or("seen") {
            "seen" => {}
            "tip" => (seq, h) = (tip_seq, tip_hash),
            "hash-mismatch" => h = "00".repeat(32),
            "beyond" => seq = tip_seq + 5,
            other => panic!("unknown prefix {other}"),
        }
        let mut req = key.request(&self.sid, &nonce, seq, &h);
        if a["proof"] == "invalid" {
            req.signature = "invalid".into();
        }
        Ok(req)
    }

    fn act(&mut self, a: &Value) -> Result<()> {
        let sid = self.sid.clone();
        match a["do"].as_str().unwrap() {
            "disconnect" => {
                let gone: Vec<String> = match a["client"].as_str() {
                    Some(c) => vec![c.to_string()],
                    None => self.bridge.client_set(&sid),
                };
                for c in &gone {
                    self.sync(c);
                }
                self.bridge
                    .client_disconnect(&sid, a["client"].as_str(), "client_disconnect")
            }
            "sync" => {
                self.sync(a["client"].as_str().unwrap());
                Ok(())
            }
            "escalate" => self.bridge.escalate_to_human(&sid, "human_unavailable"),
            "resolve" => {
                if a["answer"].as_str().unwrap_or("all") == "all" {
                    for k in self.bridge.unanswered_keys(&sid) {
                        let h = self.bridge.shown_hash_of(&sid, &k)?;
                        self.bridge.answer_tool_call(&sid, &k, &h, true)?;
                    }
                }
                self.bridge.human_resolve(&sid)
            }
            "answer" => {
                let key = a["key"].as_str().unwrap();
                let h = if a["hash"].as_str().unwrap_or("shown") == "shown" {
                    self.bridge.shown_hash_of(&sid, key)?
                } else {
                    content_hash(&serde_json::json!({"other": true}))
                };
                self.bridge
                    .answer_tool_call(&sid, key, &h, a["approve"].as_bool().unwrap_or(true))
            }
            "tool_call" => self
                .bridge
                .submit_tool_call(
                    &sid,
                    a["tool"].as_str().unwrap(),
                    a["key"].as_str(),
                    a["permission"].as_str(),
                    a.get("content"),
                )
                .map(|_| ()),
            "delivered" => self.bridge.delivered(&sid, a["key"].as_str().unwrap()),
            "completed" => self.bridge.completed(
                &sid,
                a["key"].as_str().unwrap(),
                a["result_hash"].as_str().unwrap_or(""),
            ),
            "narrow" => self
                .bridge
                .narrow_authority_scope(&sid, &strings(&a["scope"])),
            "extend" => {
                let source = match a["source"].as_str().unwrap_or("requested") {
                    "observed" => Source::Observed,
                    _ => Source::Requested,
                };
                self.bridge
                    .attempt_authority_extension(&sid, &strings(&a["scope"]), source)
                    .map(|_| ())
            }
            "add_client" => {
                let c = a["client"].as_str().unwrap().to_string();
                let via = self
                    .bridge
                    .client_set(&sid)
                    .first()
                    .cloned()
                    .unwrap_or_else(|| self.first.clone());
                let key = ClientKey::new(&c, &secret_of(&c));
                self.bridge
                    .add_client_key(&sid, &via, &c, &key.public_key())?;
                self.keys.insert(c.clone(), key);
                self.sync(&c);
                Ok(())
            }
            "challenge" => {
                let n = self.bridge.challenge(&sid)?.nonce;
                self.nonces.insert(a["as"].as_str().unwrap().to_string(), n);
                Ok(())
            }
            kind @ ("reattach" | "join") => {
                let req = self.request(a)?;
                let replay = a["transport_replay"].as_bool().unwrap_or(false);
                if kind == "join" {
                    return self.bridge.join(&sid, &req, replay).map(|_| ());
                }
                self.bridge.begin_resume(&sid, &req, replay)?;
                if a["complete"].as_bool().unwrap_or(true)
                    && self.bridge.cell_of(&sid).att == Attachment::Res
                {
                    self.bridge.complete_resume(&sid)?;
                }
                Ok(())
            }
            "complete_resume" => self.bridge.complete_resume(&sid).map(|_| ()),
            "advance" => {
                self.clock.advance(a["seconds"].as_f64().unwrap());
                self.bridge.tick(None)
            }
            "tick" => self.bridge.tick(None),
            "restart" => {
                self.clock.advance(a["downtime"].as_f64().unwrap_or(0.0));
                self.bridge = Bridge::recover(
                    self.storage.clone(),
                    self.clock.clone(),
                    Arc::new(Ed25519Verifier),
                    self.cfg.clone(),
                )?;
                Ok(())
            }
            "tamper_log" => {
                self.storage
                    .tamper_prev_hash(&sid, a["index"].as_u64().unwrap() as usize);
                Ok(())
            }
            "terminate" => self.bridge.terminate(&sid, "terminated"),
            other => panic!("unknown action {other}"),
        }
    }
}

fn run_vector(vec: &Value, storage: Arc<dyn TestStorage>) -> Vec<String> {
    let expect = &vec["expect"];
    if expect["config_error"].as_bool().unwrap_or(false) {
        return match Runner::new(vec, storage) {
            Err(_) => vec![],
            Ok(_) => vec!["expected a configuration error".into()],
        };
    }
    let mut r = Runner::new(vec, storage).expect("bridge builds");
    let mut fails = vec![];
    for (i, a) in vec["actions"].as_array().unwrap().iter().enumerate() {
        let refused = r.act(a).is_err();
        let want_err = a["error"].as_bool().unwrap_or(false);
        if refused != want_err {
            fails.push(format!(
                "action {i} {}: {}, expected {}",
                a["do"],
                if refused { "refused" } else { "accepted" },
                if want_err { "refusal" } else { "success" }
            ));
        }
        if let Some(c) = a.get("then_cell") {
            let want: Vec<String> = strings(c);
            if r.cell() != want {
                fails.push(format!("action {i}: cell {:?} != {:?}", r.cell(), want));
            }
        }
    }
    let log = r.bridge.events(&r.sid);
    let got: Vec<&str> = log.iter().map(|e| e.kind.as_str()).collect();
    let want = strings(&expect["events"]);
    if got != want.iter().map(String::as_str).collect::<Vec<_>>() {
        fails.push(format!("events\n  got  {got:?}\n  want {want:?}"));
    }
    if r.cell() != strings(&expect["cell"]) {
        fails.push(format!("cell {:?} != {}", r.cell(), expect["cell"]));
    }
    if r.bridge.scope_version(&r.sid) != expect["scope_version"].as_u64().unwrap() {
        fails.push(format!(
            "scope_version {} != {}",
            r.bridge.scope_version(&r.sid),
            expect["scope_version"]
        ));
    }
    if let Some(q) = expect.get("queue") {
        if r.bridge.queue_keys(&r.sid) != strings(q) {
            fails.push(format!("queue {:?} != {}", r.bridge.queue_keys(&r.sid), q));
        }
    }
    if let Some(f) = expect.get("forwarded") {
        let got: Vec<String> = r
            .bridge
            .forwarded
            .iter()
            .map(|(_, k, _)| k.clone())
            .collect();
        if got != strings(f) {
            fails.push(format!("forwarded {got:?} != {f}"));
        }
    }
    if let Some(c) = expect.get("clients") {
        let mut got = r.bridge.client_set(&r.sid);
        got.sort();
        if got != strings(c) {
            fails.push(format!("clients {got:?} != {c}"));
        }
    }
    if let Some(q) = expect.get("quarantined") {
        if r.bridge.is_quarantined(&r.sid) != q.as_bool().unwrap() {
            fails.push(format!(
                "quarantined {} != {q}",
                r.bridge.is_quarantined(&r.sid)
            ));
        }
    }
    if let Some(n) = expect.get("reverts") {
        if r.bridge.revert_requests.len() as u64 != n.as_u64().unwrap() {
            fails.push(format!("reverts {} != {n}", r.bridge.revert_requests.len()));
        }
    }
    if let Some(m) = expect.get("last_event_payload").and_then(Value::as_object) {
        let last = log.last().unwrap();
        for (k, v) in m {
            if &last.payload[k] != v {
                fails.push(format!(
                    "last event payload[{k}] = {} != {v}",
                    last.payload[k]
                ));
            }
        }
    }
    fails
}

fn vector_files() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vectors");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("vectors directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
}

fn run_all(label: &str, make: impl Fn() -> Arc<dyn TestStorage>) {
    let files = vector_files();
    assert!(
        files.len() >= 28,
        "expected at least 28 vectors, found {}",
        files.len()
    );
    let mut bad = 0;
    for f in &files {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let fails = run_vector(&v, make());
        println!(
            "{label} {} {} {}",
            if fails.is_empty() { "ok  " } else { "FAIL" },
            v["id"],
            v["title"]
        );
        for m in &fails {
            println!("    {m}");
        }
        bad += !fails.is_empty() as usize;
    }
    assert_eq!(bad, 0, "{bad} vector(s) failed on {label}");
}

#[test]
fn all_vectors_pass_on_memory_storage() {
    run_all("mem", || MemStorage::new());
}

#[test]
fn all_vectors_pass_on_file_storage() {
    let root = std::env::temp_dir().join(format!("bridge-contract-vectors-{}", std::process::id()));
    let n = std::sync::atomic::AtomicUsize::new(0);
    run_all("file", || {
        let dir = root.join(format!(
            "v{}",
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        common::FileStorage::new(dir)
    });
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn bridge_and_collaborators_are_send_and_sync() {
    fn send<T: Send>() {}
    fn send_sync<T: Send + Sync>() {}
    send::<Bridge>();
    send_sync::<MemStorage>();
    send_sync::<common::FileStorage>();
    send_sync::<FakeClock>();
    send_sync::<SystemClock>();
    send_sync::<Ed25519Verifier>();
    send_sync::<Config>();
}

#[test]
fn a_shared_bridge_serves_sessions_from_several_threads() {
    use std::sync::Mutex;
    let storage = MemStorage::new();
    let bridge = Arc::new(Mutex::new(
        Bridge::new(
            storage.clone(),
            FakeClock::new(0.0),
            Arc::new(Ed25519Verifier),
            Config::default(),
        )
        .unwrap(),
    ));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let bridge = bridge.clone();
            std::thread::spawn(move || {
                let sid = format!("s{i}");
                let key = ClientKey::new("ck", &[i as u8; 32]);
                let id = SessionIdentity {
                    session_id: sid.clone(),
                    principal_id: "p".into(),
                    authority_scope: vec!["a".into()],
                    task_lineage: vec![],
                };
                let mut b = bridge.lock().unwrap();
                b.attach(&id, &key).unwrap();
                b.escalate_to_human(&sid, "x").unwrap();
                b.client_disconnect(&sid, None, "x").unwrap();
                let genesis = b.events(&sid)[0].hash();
                let nonce = b.challenge(&sid).unwrap().nonce;
                let req = key.request(&sid, &nonce, 0, &genesis);
                b.begin_resume(&sid, &req, false).unwrap();
                b.complete_resume(&sid).unwrap();
                assert_eq!(b.cell_of(&sid).names(), vec!["Att", "Pend"]);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(storage.sessions().unwrap().len(), 8);
}

#[test]
fn default_config_satisfies_the_timer_bounds() {
    let c = Config::default();
    assert!(c.t_call <= c.t_human && c.t_detach <= c.t_total);
    assert_eq!(
        (c.t_detach, c.t_human, c.t_call, c.t_total),
        (1800.0, 1800.0, 1800.0, 7200.0)
    );
    let p = Config::persistent();
    assert_eq!(
        (p.t_detach, p.t_human, p.t_call),
        (604_800.0, 604_800.0, 604_800.0)
    );
    assert_eq!(p.t_total, 28.0 * 86_400.0);
    // T_human may exceed T_detach; T_call may not exceed T_human; T_total >= T_detach.
    assert!(Config::new(1800.0, Some(3600.0), None, None, 900.0, 60.0, 64).is_ok());
    assert!(Config::new(1800.0, Some(600.0), Some(900.0), None, 900.0, 60.0, 64).is_err());
    assert!(Config::new(1800.0, None, None, Some(900.0), 900.0, 60.0, 64).is_err());
}

#[test]
fn failed_append_changes_nothing() {
    let clock = FakeClock::new(0.0);
    let storage = MemStorage::new();
    let key = ClientKey::new("ck", &[7u8; 32]);
    let mut b = Bridge::new(
        storage.clone(),
        clock,
        Arc::new(Ed25519Verifier),
        Config::default(),
    )
    .unwrap();
    let id = SessionIdentity {
        session_id: "s".into(),
        principal_id: "p".into(),
        authority_scope: vec!["a".into()],
        task_lineage: vec![],
    };
    b.attach(&id, &key).unwrap();
    let cell = b.cell_of("s");
    storage.fail_next();
    assert!(b.client_disconnect("s", None, "x").is_err());
    assert_eq!(b.cell_of("s"), cell);
    assert_eq!(b.events("s").len(), 1);
}

#[test]
fn registry_holds_public_keys_only() {
    let seed = [9u8; 32];
    let key = ClientKey::new("ck", &seed);
    let storage = MemStorage::new();
    let mut b = Bridge::new(
        storage.clone(),
        FakeClock::new(0.0),
        Arc::new(Ed25519Verifier),
        Config::default(),
    )
    .unwrap();
    let id = SessionIdentity {
        session_id: "s".into(),
        principal_id: "p".into(),
        authority_scope: vec!["a".into()],
        task_lineage: vec![],
    };
    b.attach(&id, &key).unwrap();
    let stored = storage.key_material("ck").unwrap().unwrap();
    assert_eq!(stored, key.public_key());
    assert_ne!(stored, seed.to_vec());
}
