//! Runs the language-neutral vectors in `vectors/*.json` (schema in
//! `vectors/README.md`) against this crate.

use std::path::PathBuf;
use std::rc::Rc;

use bridge_contract::*;
use serde_json::Value;

struct Runner {
    storage: Rc<std::cell::RefCell<Storage>>,
    clock: Rc<FakeClock>,
    cfg: Config,
    key: ClientKey,
    bridge: Bridge,
    sid: String,
    tokens: Vec<ResumeToken>,
}

fn config_of(v: &Value) -> Config {
    let mut c = Config::default();
    if let Some(m) = v.as_object() {
        for (k, x) in m {
            match k.as_str() {
                "t_detach" => c.t_detach = x.as_f64().unwrap(),
                "t_human" => c.t_human = x.as_f64().unwrap(),
                "t_token" => c.t_token = x.as_f64().unwrap(),
                "t_call" => c.t_call = x.as_f64().unwrap(),
                "t_approve" => c.t_approve = x.as_f64().unwrap(),
                "q_max" => c.q_max = x.as_u64().unwrap() as usize,
                other => panic!("unknown config key {other}"),
            }
        }
    }
    c
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

impl Runner {
    fn new(vec: &Value) -> Result<Runner> {
        let cfg = config_of(&vec["config"]);
        let clock = FakeClock::new(1000.0);
        let storage = Storage::new(b"mac-key-for-vectors-0123456789ab");
        let s = &vec["session"];
        let key = ClientKey::new(s["client_key_id"].as_str().unwrap(), b"client-secret-0123456789abcdefgh");
        let mut bridge = Bridge::new(storage.clone(), clock.clone(), cfg.clone())?;
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
        let mut r = Runner { storage, clock, cfg, key, bridge, sid, tokens: vec![] };
        r.note_token();
        Ok(r)
    }

    fn note_token(&mut self) {
        if let Some(cur) = self.bridge.current_token(&self.sid).unwrap() {
            if self.tokens.last().map(|t| t.sequence) != Some(cur.sequence) {
                self.tokens.push(cur);
            }
        }
    }

    fn cell(&self) -> Vec<&'static str> {
        self.bridge.cell_of(&self.sid).names()
    }

    fn token(&self, which: &str) -> ResumeToken {
        let n = self.tokens.len();
        match which {
            "current" => self.tokens[n - 1].clone(),
            "previous" => self.tokens[n - 2].clone(),
            "tampered" => {
                let mut t = self.tokens[n - 1].clone();
                t.authority_scope = vec!["exec:sudo".to_string()];
                t
            }
            other => panic!("unknown token {other}"),
        }
    }

    fn act(&mut self, a: &Value) -> Result<()> {
        let sid = self.sid.clone();
        let r = match a["do"].as_str().unwrap() {
            "disconnect" => self.bridge.client_disconnect(&sid, "client_disconnect"),
            "escalate" => self.bridge.escalate_to_human(&sid, "human_unavailable"),
            "resolve" => self.bridge.human_resolve(&sid),
            "tool_call" => self
                .bridge
                .submit_tool_call(&sid, a["tool"].as_str().unwrap(), a["key"].as_str(), a["permission"].as_str())
                .map(|_| ()),
            "narrow" => self.bridge.narrow_authority_scope(&sid, &strings(&a["scope"])),
            "extend" => self.bridge.attempt_authority_extension(&sid, &strings(&a["scope"])),
            "reattach" => {
                let tok = self.token(a["token"].as_str().unwrap());
                let nonce = self.bridge.reattach_nonce(&sid)?;
                let proof = if a["proof"] == "valid" { self.key.sign(&nonce, tok.sequence) } else { "invalid".into() };
                let replay = a["transport_replay"].as_bool().unwrap_or(false);
                let mut r = self.bridge.begin_resume(&sid, &tok, &proof, replay);
                if r.is_ok()
                    && a["complete"].as_bool().unwrap_or(true)
                    && self.bridge.cell_of(&sid).att == Attachment::Res
                {
                    r = self.bridge.complete_resume(&sid).map(|_| ());
                }
                r
            }
            "complete_resume" => self.bridge.complete_resume(&sid).map(|_| ()),
            "advance" => {
                self.clock.advance(a["seconds"].as_f64().unwrap());
                self.bridge.tick(None)
            }
            "tick" => self.bridge.tick(None),
            "restart" => {
                self.clock.advance(a["downtime"].as_f64().unwrap_or(0.0));
                self.bridge = Bridge::recover(self.storage.clone(), self.clock.clone(), self.cfg.clone())?;
                Ok(())
            }
            "tamper_log" => {
                let i = a["index"].as_u64().unwrap() as usize;
                self.storage.borrow_mut().logs.get_mut(&sid).unwrap()[i].prev_hash = "00".repeat(32);
                Ok(())
            }
            "terminate" => self.bridge.terminate(&sid, "terminated"),
            other => panic!("unknown action {other}"),
        };
        self.note_token();
        r
    }
}

fn run_vector(vec: &Value) -> Vec<String> {
    let expect = &vec["expect"];
    if expect["config_error"].as_bool().unwrap_or(false) {
        return match Runner::new(vec) {
            Err(_) => vec![],
            Ok(_) => vec!["expected a configuration error".into()],
        };
    }
    let mut r = Runner::new(vec).expect("bridge builds");
    let mut fails = vec![];
    for (i, a) in vec["actions"].as_array().unwrap().iter().enumerate() {
        let refused = r.act(a).is_err();
        let want_err = a["error"].as_bool().unwrap_or(false);
        if refused != want_err {
            fails.push(format!("action {i} {}: {}, expected {}", a["do"], if refused { "refused" } else { "accepted" }, if want_err { "refusal" } else { "success" }));
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
        fails.push(format!("scope_version {} != {}", r.bridge.scope_version(&r.sid), expect["scope_version"]));
    }
    if let Some(q) = expect.get("queue") {
        if r.bridge.queue_keys(&r.sid) != strings(q) {
            fails.push(format!("queue {:?} != {}", r.bridge.queue_keys(&r.sid), q));
        }
    }
    if let Some(f) = expect.get("forwarded") {
        let got: Vec<String> = r.bridge.forwarded.iter().map(|(_, k, _)| k.clone()).collect();
        if got != strings(f) {
            fails.push(format!("forwarded {got:?} != {f}"));
        }
    }
    if let Some(m) = expect.get("last_event_payload").and_then(Value::as_object) {
        let last = log.last().unwrap();
        for (k, v) in m {
            if &last.payload[k] != v {
                fails.push(format!("last event payload[{k}] = {} != {v}", last.payload[k]));
            }
        }
    }
    fails
}

#[test]
fn all_vectors_pass() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vectors");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("vectors directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 20, "expected at least 20 vectors, found {}", files.len());
    let mut bad = 0;
    for f in &files {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let fails = run_vector(&v);
        println!("{} {} {}", if fails.is_empty() { "ok  " } else { "FAIL" }, v["id"], v["title"]);
        for m in &fails {
            println!("    {m}");
        }
        bad += !fails.is_empty() as usize;
    }
    assert_eq!(bad, 0, "{bad} vector(s) failed");
}

#[test]
fn default_config_respects_the_ttl_bound() {
    let c = Config::default();
    assert!(c.t_token <= c.t_detach);
    assert_eq!((c.t_token, c.t_detach), (900.0, 1800.0));
}

#[test]
fn failed_append_changes_nothing() {
    let clock = FakeClock::new(0.0);
    let storage = Storage::new(b"k");
    let key = ClientKey::new("ck", b"s");
    let mut b = Bridge::new(storage.clone(), clock, Config::default()).unwrap();
    let id = SessionIdentity { session_id: "s".into(), principal_id: "p".into(), authority_scope: vec!["a".into()], task_lineage: vec![] };
    b.attach(&id, &key).unwrap();
    let cell = b.cell_of("s");
    storage.borrow_mut().fail_next = true;
    assert!(b.client_disconnect("s", "x").is_err());
    assert_eq!(b.cell_of("s"), cell);
    assert_eq!(b.events("s").len(), 1);
}
