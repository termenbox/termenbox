use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

use serde_json::{json, Value};

use crate::types::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    Queued,
    Duplicate,
    Rejected,
    Forwarded,
}

#[derive(Debug, Clone)]
struct Queued {
    key: String,
    tool: String,
    permission: Option<String>,
    expires_at: f64,
}

/// Per-session state. It is derived from the log only (see `apply`).
#[derive(Debug, Clone)]
struct Session {
    principal: String,
    lineage: Vec<String>,
    key_id: String,
    scope: Vec<String>,
    scope_version: u64,
    cell: Cell,
    detach_deadline: Option<f64>,
    human_deadline: Option<f64>,
    token_event: Option<Event>,
    token_expires: f64,
    queue: Vec<Queued>,
    used_keys: HashSet<String>,
    approval_until: f64,
}

pub struct Bridge {
    storage: Rc<RefCell<Storage>>,
    clock: Rc<dyn Clock>,
    cfg: Config,
    sessions: BTreeMap<String, Session>,
    /// Calls handed to the agent runtime: (session, key, call).
    pub forwarded: Vec<(String, String, String)>,
}

type Item = (Kind, Value);

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

impl Bridge {
    pub fn new(
        storage: Rc<RefCell<Storage>>,
        clock: Rc<dyn Clock>,
        cfg: Config,
    ) -> Result<Bridge> {
        if cfg.t_token > cfg.t_detach {
            return Err(BridgeError::new("T_token must not exceed T_detach (I9)"));
        }
        Ok(Bridge { storage, clock, cfg, sessions: BTreeMap::new(), forwarded: vec![] })
    }

    // ---- commit / apply: the only way a cell changes ---------------------

    fn commit(&mut self, sid: &str, items: Vec<Item>) -> Result<Vec<Event>> {
        let ts = self.clock.now();
        let mut batch: Vec<Event> = Vec::with_capacity(items.len());
        {
            let st = self.storage.borrow();
            let log = st.logs.get(sid).map(Vec::as_slice).unwrap_or(&[]);
            let mut prev = log.last().map(Event::hash).unwrap_or_default();
            let mut seq = log.len() as u64;
            for (kind, payload) in items {
                let ev = Event {
                    kind,
                    session_id: sid.to_string(),
                    sequence: seq,
                    prev_hash: prev,
                    payload,
                    timestamp: ts,
                };
                prev = ev.hash();
                seq += 1;
                batch.push(ev);
            }
        }
        self.storage.borrow_mut().append_batch(sid, batch.clone())?; // durable first
        for ev in &batch {
            self.apply(ev);
        }
        Ok(batch)
    }

    fn apply(&mut self, ev: &Event) {
        let p = &ev.payload;
        if ev.kind == Kind::ATTACHED {
            self.sessions.insert(
                ev.session_id.clone(),
                Session {
                    principal: p["principal_id"].as_str().unwrap_or("").to_string(),
                    lineage: strs(&p["task_lineage"]),
                    key_id: p["client_key_id"].as_str().unwrap_or("").to_string(),
                    scope: strs(&p["authority_scope"]),
                    scope_version: 0,
                    cell: Cell { att: Attachment::Att, human: Human::None, life: Life::Active },
                    detach_deadline: None,
                    human_deadline: None,
                    token_event: None,
                    token_expires: 0.0,
                    queue: vec![],
                    used_keys: HashSet::new(),
                    approval_until: 0.0,
                },
            );
            return;
        }
        let (t_detach, t_human) = (self.cfg.t_detach, self.cfg.t_human);
        let Some(s) = self.sessions.get_mut(&ev.session_id) else { return };
        match ev.kind {
            Kind::DETACHMENT => {
                s.cell.att = Attachment::Det;
                s.detach_deadline = Some(ev.timestamp + t_detach);
            }
            Kind::RESUME_PREPARED => {
                s.token_expires = ev.timestamp + p["ttl_seconds"].as_f64().unwrap_or(0.0);
                s.token_event = Some(ev.clone());
            }
            Kind::RESUME_STARTED => {
                s.cell.att = Attachment::Res;
                s.detach_deadline = None;
            }
            Kind::RESUME_VERIFIED => s.cell.att = Attachment::Att,
            Kind::HUMAN_PENDING => {
                if s.cell.human == Human::None {
                    s.cell.human = Human::Pend;
                    s.human_deadline = Some(ev.timestamp + t_human);
                }
            }
            Kind::HUMAN_RESOLVED => {
                s.cell.human = Human::None;
                s.human_deadline = None;
                s.approval_until = p["valid_until"].as_f64().unwrap_or(0.0);
            }
            Kind::AUTHORITY_NARROWED => {
                s.scope = strs(&p["new_scope"]);
                s.scope_version = p["new_version"].as_u64().unwrap_or(s.scope_version);
            }
            Kind::TOOL_CALL_QUEUED => {
                let key = p["key"].as_str().unwrap_or("").to_string();
                s.used_keys.insert(key.clone());
                s.queue.push(Queued {
                    key,
                    tool: p["tool"].as_str().unwrap_or("").to_string(),
                    permission: p["permission"].as_str().map(String::from),
                    expires_at: p["expires_at"].as_f64().unwrap_or(0.0),
                });
            }
            Kind::TOOL_CALL_DRAINED | Kind::TOOL_CALL_REJECTED | Kind::TOOL_CALL_EXPIRED => {
                let key = p["key"].as_str().unwrap_or("");
                s.queue.retain(|c| c.key != key);
                s.used_keys.insert(key.to_string());
            }
            Kind::TERMINATION => s.cell.life = Life::Terminated,
            // Audit-only events do not move the cell.
            _ => {}
        }
    }

    // ---- sessions -----------------------------------------------------------

    pub fn attach(&mut self, id: &SessionIdentity, key: &ClientKey) -> Result<()> {
        let sid = id.session_id.as_str();
        if self.sessions.contains_key(sid) || self.storage.borrow().logs.contains_key(sid) {
            return Err(BridgeError::new("session already exists (I6)"));
        }
        self.storage.borrow_mut().client_keys.insert(key.key_id.clone(), key.secret.clone());
        self.commit(
            sid,
            vec![(
                Kind::ATTACHED,
                json!({
                    "principal_id": id.principal_id,
                    "authority_scope": id.authority_scope,
                    "task_lineage": id.task_lineage,
                    "client_key_id": key.key_id,
                    "scope_version": 0,
                }),
            )],
        )?;
        Ok(())
    }

    /// Known, not terminated (I6); timers evaluated first.
    fn enter(&mut self, sid: &str) -> Result<Session> {
        let terminated = |b: &Bridge| {
            b.sessions.get(sid).map(|s| s.cell.life == Life::Terminated).unwrap_or(false)
        };
        if !self.sessions.contains_key(sid) {
            return Err(BridgeError::new(format!("unknown session {sid}")));
        }
        if terminated(self) {
            return Err(BridgeError::new("session is TERMINATED (I6)"));
        }
        self.check_timers(sid)?;
        if terminated(self) {
            return Err(BridgeError::new("session is TERMINATED (I6)"));
        }
        Ok(self.sessions[sid].clone())
    }

    // ---- tool calls (I2, I13) ---------------------------------------------

    pub fn submit_tool_call(
        &mut self,
        sid: &str,
        tool: &str,
        key: Option<&str>,
        permission: Option<&str>,
    ) -> Result<ToolOutcome> {
        let s = self.enter(sid)?;
        if s.cell.human == Human::Pend {
            let key = key
                .map(String::from)
                .unwrap_or_else(|| format!("tc-{:04}", s.used_keys.len() + 1));
            if s.used_keys.contains(&key) {
                return Ok(ToolOutcome::Duplicate);
            }
            if s.queue.len() >= self.cfg.q_max {
                self.commit(
                    sid,
                    vec![(
                        Kind::TOOL_CALL_REJECTED,
                        json!({"key": key, "tool": tool, "reason": "queue_full"}),
                    )],
                )?;
                return Ok(ToolOutcome::Rejected);
            }
            let expires_at = self.clock.now() + self.cfg.t_call;
            self.commit(
                sid,
                vec![(
                    Kind::TOOL_CALL_QUEUED,
                    json!({"key": key, "tool": tool, "permission": permission,
                           "expires_at": expires_at}),
                )],
            )?;
            return Ok(ToolOutcome::Queued);
        }
        if s.cell.att == Attachment::Att {
            self.forwarded.push((sid.to_string(), key.unwrap_or("").to_string(), tool.to_string()));
            return Ok(ToolOutcome::Forwarded);
        }
        Err(BridgeError::new("tool call not allowed in this cell"))
    }

    /// Rule D: drain in arrival order, revalidating every call (I13).
    fn drain(&mut self, sid: &str) -> Result<()> {
        let s = self.sessions[sid].clone();
        if !(s.cell.att == Attachment::Att
            && s.cell.human == Human::None
            && s.cell.life == Life::Active
            && !s.queue.is_empty())
        {
            return Ok(());
        }
        let now = self.clock.now();
        let ah = authority_hash(&s.principal, &s.scope, &s.lineage);
        let items: Vec<Item> = s
            .queue
            .iter()
            .map(|c| {
                let base = |extra: (&str, &str)| {
                    json!({"key": c.key, "tool": c.tool, extra.0: extra.1})
                };
                if now >= c.expires_at {
                    (Kind::TOOL_CALL_EXPIRED, base(("reason", "call_ttl")))
                } else if now >= s.approval_until {
                    (Kind::TOOL_CALL_EXPIRED, base(("reason", "approval_ttl")))
                } else if c.permission.as_ref().is_some_and(|p| !s.scope.contains(p)) {
                    (Kind::TOOL_CALL_REJECTED, base(("reason", "scope")))
                } else {
                    (Kind::TOOL_CALL_DRAINED, base(("authority_hash", ah.as_str())))
                }
            })
            .collect();
        let batch = self.commit(sid, items)?;
        for ev in batch.iter().filter(|e| e.kind == Kind::TOOL_CALL_DRAINED) {
            // forwarded only after the DRAINED events are durable (I12)
            self.forwarded.push((
                sid.to_string(),
                ev.str_field("key").unwrap_or("").to_string(),
                ev.str_field("tool").unwrap_or("").to_string(),
            ));
        }
        Ok(())
    }

    // ---- detach, escalate, resolve ----------------------------------------

    /// RESUME_PREPARED carrying the token content (I1, I9, I10).
    fn prepared(
        &self,
        s: &Session,
        reason: &str,
        detach_deadline: f64,
        scope: Option<&[String]>,
        version: Option<u64>,
    ) -> Item {
        let remaining = detach_deadline - self.clock.now();
        let ttl = self.cfg.t_token.min(remaining).max(0.0);
        let scope = scope.unwrap_or(&s.scope);
        let calls: Vec<Value> =
            s.queue.iter().map(|c| json!({"key": c.key, "call": c.tool})).collect();
        (
            Kind::RESUME_PREPARED,
            json!({
                "authority_hash": authority_hash(&s.principal, scope, &s.lineage),
                "reason": reason,
                "authority_scope": scope,
                "scope_version": version.unwrap_or(s.scope_version),
                "pending_tool_calls": calls,
                "ttl_seconds": ttl,
                "client_key_id": s.key_id,
            }),
        )
    }

    pub fn client_disconnect(&mut self, sid: &str, reason: &str) -> Result<()> {
        let s = self.enter(sid)?;
        if s.cell.att == Attachment::Det {
            return Ok(());
        }
        let deadline = self.clock.now() + self.cfg.t_detach;
        let items = vec![
            (Kind::DETACHMENT, json!({"reason": reason})),
            self.prepared(&s, reason, deadline, None, None),
        ];
        self.commit(sid, items)?;
        self.check_timers(sid)
    }

    pub fn escalate_to_human(&mut self, sid: &str, reason: &str) -> Result<()> {
        let s = self.enter(sid)?;
        let mut items = vec![(Kind::HUMAN_PENDING, json!({"reason": reason}))];
        if s.cell.att == Attachment::Det {
            items.push(self.prepared(&s, "human_pending", s.detach_deadline.unwrap(), None, None));
        }
        self.commit(sid, items)?;
        self.check_timers(sid)
    }

    pub fn human_resolve(&mut self, sid: &str) -> Result<()> {
        let s = self.enter(sid)?;
        if s.cell.human != Human::Pend {
            return Err(BridgeError::new("human_resolve requires Human=Pend"));
        }
        let mut items =
            vec![(Kind::HUMAN_RESOLVED, json!({"valid_until": self.clock.now() + self.cfg.t_approve}))];
        if s.cell.att == Attachment::Det {
            items.push(self.prepared(&s, "human_resolved", s.detach_deadline.unwrap(), None, None));
        }
        self.commit(sid, items)?;
        self.drain(sid)
    }

    // ---- timers (L1, L2, I13) ------------------------------------------------

    pub fn tick(&mut self, sid: Option<&str>) -> Result<()> {
        let ids: Vec<String> = match sid {
            Some(x) => vec![x.to_string()],
            None => self.sessions.keys().cloned().collect(),
        };
        for id in ids {
            self.check_timers(&id)?;
        }
        Ok(())
    }

    fn check_timers(&mut self, sid: &str) -> Result<()> {
        let s = self.sessions[sid].clone();
        if s.cell.life == Life::Terminated {
            return Ok(());
        }
        let now = self.clock.now();
        let detach = s.cell.att == Attachment::Det && s.detach_deadline.is_some_and(|d| now >= d);
        let human = s.cell.human == Human::Pend && s.human_deadline.is_some_and(|d| now >= d);
        let items: Vec<Item> = match (detach, human) {
            (true, true) => vec![
                (Kind::HUMAN_TIMEOUT, json!({"reason": "human_timeout"})),
                (Kind::DETACH_TIMEOUT, json!({"reason": "detach_timeout"})),
                (
                    Kind::TERMINATION,
                    json!({"reason": "human_timeout+detach_timeout (simultaneous)"}),
                ),
            ],
            (false, true) => vec![
                (Kind::HUMAN_TIMEOUT, json!({"reason": "human_timeout"})),
                (Kind::TERMINATION, json!({"reason": "human_timeout"})),
            ],
            (true, false) => vec![
                (Kind::DETACH_TIMEOUT, json!({"reason": "detach_timeout"})),
                (Kind::TERMINATION, json!({"reason": "detach_timeout"})),
            ],
            (false, false) => s
                .queue
                .iter()
                .filter(|c| now >= c.expires_at)
                .map(|c| {
                    (
                        Kind::TOOL_CALL_EXPIRED,
                        json!({"key": c.key, "tool": c.tool, "reason": "call_ttl"}),
                    )
                })
                .collect(),
        };
        if items.is_empty() {
            return Ok(());
        }
        self.commit(sid, items).map(|_| ())
    }

    // ---- authenticated, single-use re-attach (I4, I8, I9, I11) -------------

    fn mac_key(&self) -> Result<Vec<u8>> {
        self.storage
            .borrow()
            .mac_key
            .clone()
            .ok_or_else(|| BridgeError::new("MAC key unavailable"))
    }

    fn mac(&self, t: &ResumeToken) -> Result<String> {
        Ok(hmac_hex(&self.mac_key()?, &canon(&t.fields())))
    }

    /// Fresh bridge-supplied nonce, bound to the current token.
    pub fn reattach_nonce(&self, sid: &str) -> Result<String> {
        let seq: i64 = self
            .sessions
            .get(sid)
            .and_then(|s| s.token_event.as_ref())
            .map(|e| e.sequence as i64)
            .unwrap_or(-1);
        Ok(hmac_hex(&self.mac_key()?, format!("nonce|{sid}|{seq}").as_bytes()))
    }

    pub fn current_token(&self, sid: &str) -> Result<Option<ResumeToken>> {
        let Some(s) = self.sessions.get(sid) else { return Ok(None) };
        let Some(ev) = &s.token_event else { return Ok(None) };
        let p = &ev.payload;
        let calls = p["pending_tool_calls"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|c| {
                        (
                            c["key"].as_str().unwrap_or("").to_string(),
                            c["call"].as_str().unwrap_or("").to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut t = ResumeToken {
            session_id: sid.to_string(),
            sequence: ev.sequence,
            event_hash: ev.hash(),
            authority_scope: strs(&p["authority_scope"]),
            scope_version: p["scope_version"].as_u64().unwrap_or(0),
            pending_tool_calls: calls,
            reason: p["reason"].as_str().unwrap_or("").to_string(),
            ttl_seconds: p["ttl_seconds"].as_f64().unwrap_or(0.0),
            client_key_id: p["client_key_id"].as_str().unwrap_or("").to_string(),
            mac: String::new(),
        };
        t.mac = self.mac(&t)?;
        Ok(Some(t))
    }

    fn reject_reason(
        &self,
        sid: &str,
        s: &Session,
        t: &ResumeToken,
        proof: &str,
        replay: bool,
    ) -> Result<Option<&'static str>> {
        if replay {
            return Ok(Some("transport_replay")); // I8
        }
        if self.mac(t)? != t.mac {
            return Ok(Some("bad_mac")); // I11(b)
        }
        if t.session_id != sid {
            return Ok(Some("wrong_session"));
        }
        // I11(d)/I4: the only acceptable token is the last one issued; every
        // entry to Det issues a fresh one, so a started token is never current.
        match &s.token_event {
            Some(e) if e.sequence == t.sequence => {}
            _ => return Ok(Some("superseded")),
        }
        if t.scope_version != s.scope_version {
            return Ok(Some("scope_version_mismatch")); // I10
        }
        let secret = self.storage.borrow().client_keys.get(&s.key_id).cloned();
        let want = secret.map(|k| hmac_hex(&k, format!("{}|{}", self.reattach_nonce(sid).unwrap_or_default(), t.sequence).as_bytes()));
        if want.as_deref() != Some(proof) {
            return Ok(Some("bad_proof")); // I11(c)
        }
        Ok(None)
    }

    /// Det -> Res on an authenticated re-attach; otherwise RESUME_REJECTED
    /// and no change of cell (I11e).
    pub fn begin_resume(
        &mut self,
        sid: &str,
        token: &ResumeToken,
        proof: &str,
        transport_replay: bool,
    ) -> Result<()> {
        let s = self.enter(sid)?;
        if s.cell.att != Attachment::Det {
            return Ok(()); // idempotent re-attach
        }
        if let Some(reason) = self.reject_reason(sid, &s, token, proof, transport_replay)? {
            self.commit(
                sid,
                vec![(
                    Kind::RESUME_REJECTED,
                    json!({"reason": reason, "claimed_seq": token.sequence}),
                )],
            )?;
            return Err(BridgeError::new(format!("re-attach rejected: {reason} (I4/I8/I11)")));
        }
        if self.clock.now() >= s.token_expires {
            // I9
            self.commit(
                sid,
                vec![
                    (Kind::RESUME_TOKEN_EXPIRED, json!({"reason": "resume_token_expired"})),
                    (Kind::TERMINATION, json!({"reason": "resume_token_expired"})),
                ],
            )?;
            return Err(BridgeError::new("resume token expired (I9)"));
        }
        self.commit(sid, vec![(Kind::RESUME_STARTED, json!({"sequence": token.sequence}))])?;
        Ok(())
    }

    /// Res -> Att once the hash chain verifies; otherwise terminate.
    pub fn complete_resume(&mut self, sid: &str) -> Result<ResumeToken> {
        let s = self.enter(sid)?;
        if s.cell.att != Attachment::Res {
            return Err(BridgeError::new("complete_resume requires Att=Res"));
        }
        if let Err(e) = self.verify_chain(sid) {
            self.commit(
                sid,
                vec![
                    (Kind::RESUME_FAILED, json!({"reason": "chain"})),
                    (Kind::TERMINATION, json!({"reason": "resume_fail"})),
                ],
            )?;
            return Err(e);
        }
        self.commit(sid, vec![(Kind::RESUME_VERIFIED, json!({}))])?;
        let token = self.current_token(sid)?.expect("a resumed session has a token");
        self.drain(sid)?;
        Ok(token)
    }

    // ---- authority (I3, I10) -----------------------------------------------

    fn reject_extension(&mut self, sid: &str, s: &Session, proposed: &[String]) -> Result<()> {
        let cell = format!("{:?}/{:?}", s.cell.att, s.cell.human);
        self.commit(
            sid,
            vec![(
                Kind::AUTHORITY_EXTENSION_REJECTED,
                json!({"proposed": proposed, "current": s.scope, "cell": cell}),
            )],
        )?;
        Err(BridgeError::new("authority extension rejected (I3)"))
    }

    pub fn attempt_authority_extension(&mut self, sid: &str, proposed: &[String]) -> Result<()> {
        let s = self.enter(sid)?;
        if proposed.iter().all(|p| s.scope.contains(p)) {
            return Ok(()); // a subset is not an extension
        }
        self.reject_extension(sid, &s, proposed)
    }

    pub fn narrow_authority_scope(&mut self, sid: &str, proposed: &[String]) -> Result<()> {
        let s = self.enter(sid)?;
        let narrower = proposed.iter().all(|p| s.scope.contains(p));
        if !narrower {
            return self.reject_extension(sid, &s, proposed);
        }
        let same = s.scope.iter().all(|c| proposed.contains(c));
        if same {
            self.commit(
                sid,
                vec![(
                    Kind::AUTHORITY_NARROWING_REJECTED,
                    json!({"proposed": proposed, "current": s.scope,
                           "reason": "not a strict narrowing"}),
                )],
            )?;
            return Err(BridgeError::new("authority narrowing rejected (I10)"));
        }
        let version = s.scope_version + 1;
        let mut items = vec![(
            Kind::AUTHORITY_NARROWED,
            json!({"new_scope": proposed, "new_version": version}),
        )];
        if s.cell.att == Attachment::Det {
            items.push(self.prepared(
                &s,
                "scope_narrowed",
                s.detach_deadline.unwrap(),
                Some(proposed),
                Some(version),
            ));
        }
        self.commit(sid, items).map(|_| ())
    }

    pub fn terminate(&mut self, sid: &str, reason: &str) -> Result<()> {
        match self.sessions.get(sid) {
            None => Err(BridgeError::new(format!("unknown session {sid}"))),
            Some(s) if s.cell.life == Life::Terminated => Ok(()),
            Some(_) => self.commit(sid, vec![(Kind::TERMINATION, json!({"reason": reason}))]).map(|_| ()),
        }
    }

    // ---- restart and recovery (Section 5.5) ---------------------------------

    /// Rebuild every session from the durable log. All transports died with
    /// the process, so each active session comes back detached with a fresh
    /// token; a session whose chain does not verify, or whose MAC key is
    /// missing, is terminated with RECOVERY_FAILED.
    pub fn recover(
        storage: Rc<RefCell<Storage>>,
        clock: Rc<dyn Clock>,
        cfg: Config,
    ) -> Result<Bridge> {
        let mut b = Bridge::new(storage.clone(), clock, cfg)?;
        let mut ids: Vec<String> = storage.borrow().logs.keys().cloned().collect();
        ids.sort();
        for sid in ids {
            let log: Vec<Event> = storage.borrow().logs[&sid].clone();
            for ev in &log {
                b.apply(ev);
            }
            if b.sessions[&sid].cell.life == Life::Terminated {
                continue;
            }
            let ok = b.mac_key().and_then(|_| b.verify_chain(&sid));
            if let Err(e) = ok {
                b.commit(
                    &sid,
                    vec![
                        (Kind::RECOVERY_FAILED, json!({"reason": e.to_string()})),
                        (Kind::TERMINATION, json!({"reason": "recovery_fail"})),
                    ],
                )?;
                continue;
            }
            let s = b.sessions[&sid].clone();
            let deadline = b.clock.now() + b.cfg.t_detach;
            let items = vec![
                (Kind::BRIDGE_RECOVERED, json!({"events": log.len()})),
                (Kind::DETACHMENT, json!({"reason": "bridge_restart"})),
                b.prepared(&s, "bridge_restart", deadline, None, None),
            ];
            b.commit(&sid, items)?;
            b.check_timers(&sid)?;
        }
        Ok(b)
    }

    // ---- introspection --------------------------------------------------------

    fn verify_chain(&self, sid: &str) -> Result<()> {
        let st = self.storage.borrow();
        let mut prev = String::new();
        for (i, ev) in st.logs.get(sid).map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
            if ev.sequence != i as u64 || ev.prev_hash != prev {
                return Err(BridgeError::new(format!("event chain broken at index {i} (I5)")));
            }
            prev = ev.hash();
        }
        Ok(())
    }

    pub fn events(&self, sid: &str) -> Vec<Event> {
        self.storage.borrow().logs.get(sid).cloned().unwrap_or_default()
    }

    pub fn cell_of(&self, sid: &str) -> Cell {
        self.sessions[sid].cell
    }

    pub fn scope_version(&self, sid: &str) -> u64 {
        self.sessions[sid].scope_version
    }

    pub fn scope_of(&self, sid: &str) -> Vec<String> {
        self.sessions[sid].scope.clone()
    }

    pub fn queue_keys(&self, sid: &str) -> Vec<String> {
        self.sessions[sid].queue.iter().map(|c| c.key.clone()).collect()
    }
}
