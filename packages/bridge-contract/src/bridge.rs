use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::types::*;

/// Nonces remembered per session (volatile).
const NONCE_RECORDS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    Queued,
    Duplicate,
    Rejected,
    Forwarded,
}

/// Where an authority change came from (I3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Requested through the bridge: enforced.
    Requested,
    /// Announced by the runtime itself: observed, reported, revert requested.
    Observed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityOutcome {
    /// A subset of the current scope: nothing to do.
    NoChange,
    /// An observed widening was reported; the runtime should revert.
    Revert,
}

#[derive(Debug, Clone)]
struct Queued {
    key: String,
    tool: String,
    permission: Option<String>,
    expires_at: f64,
    shown_hash: String,
    /// (approve, hash the client displayed)
    answer: Option<(bool, String)>,
}

/// Per-session state. It is derived from the log only (see `apply`).
#[derive(Debug, Clone)]
struct Session {
    principal: String,
    lineage: Vec<String>,
    scope: Vec<String>,
    scope_version: u64,
    keys: BTreeSet<String>,
    conn: BTreeSet<String>,
    resuming: Option<String>,
    resume_from: i64,
    cell: Cell,
    detach_deadline: Option<f64>,
    human_deadline: Option<f64>,
    record: Option<Event>,
    queue: Vec<Queued>,
    used_keys: HashSet<String>,
    approval_until: f64,
}

#[derive(Debug, Clone)]
struct NonceRec {
    nonce: String,
    issued: f64,
    used: bool,
}

pub struct Bridge {
    storage: Arc<dyn Storage>,
    clock: Arc<dyn Clock>,
    verifier: Arc<dyn Verifier>,
    cfg: Config,
    sessions: BTreeMap<String, Session>,
    /// Volatile: void after a restart.
    nonces: BTreeMap<String, VecDeque<NonceRec>>,
    /// Calls handed to the agent runtime: (session, key, call).
    pub forwarded: Vec<(String, String, String)>,
    /// Instructions to revert an observed widening: (session, scope to revert to).
    pub revert_requests: Vec<(String, Vec<String>)>,
}

type Item = (Kind, Value);

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

impl Bridge {
    pub fn new(
        storage: Arc<dyn Storage>,
        clock: Arc<dyn Clock>,
        verifier: Arc<dyn Verifier>,
        cfg: Config,
    ) -> Result<Bridge> {
        cfg.validate()?;
        Ok(Bridge {
            storage,
            clock,
            verifier,
            cfg,
            sessions: BTreeMap::new(),
            nonces: BTreeMap::new(),
            forwarded: vec![],
            revert_requests: vec![],
        })
    }

    // ---- commit / apply: the only way a cell changes ---------------------

    fn commit(&mut self, sid: &str, items: Vec<Item>) -> Result<Vec<Event>> {
        let ts = self.clock.now();
        let log = self.storage.read_log(sid)?;
        let mut prev = log.last().map(Event::hash).unwrap_or_default();
        let mut seq = log.len() as u64;
        let mut batch: Vec<Event> = Vec::with_capacity(items.len());
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
        self.storage.append_batch(sid, batch.clone())?; // durable first
        for ev in &batch {
            self.apply(ev);
        }
        Ok(batch)
    }

    fn apply(&mut self, ev: &Event) {
        let p = &ev.payload;
        if ev.kind == Kind::ATTACHED {
            let key = p["client_key_id"].as_str().unwrap_or("").to_string();
            self.sessions.insert(
                ev.session_id.clone(),
                Session {
                    principal: p["principal_id"].as_str().unwrap_or("").to_string(),
                    lineage: strs(&p["task_lineage"]),
                    scope: strs(&p["authority_scope"]),
                    scope_version: 0,
                    keys: BTreeSet::from([key.clone()]),
                    conn: BTreeSet::from([key]),
                    resuming: None,
                    resume_from: 0,
                    cell: Cell { att: Attachment::Att, human: Human::None, life: Life::Active },
                    detach_deadline: None,
                    human_deadline: None,
                    record: None,
                    queue: vec![],
                    used_keys: HashSet::new(),
                    approval_until: 0.0,
                },
            );
            return;
        }
        let (t_detach, t_human) = (self.cfg.t_detach, self.cfg.t_human);
        let Some(s) = self.sessions.get_mut(&ev.session_id) else { return };
        let key_id = p["key_id"].as_str().unwrap_or("").to_string();
        match ev.kind {
            Kind::CLIENT_ADDED => {
                s.keys.insert(key_id);
            }
            Kind::CLIENT_JOINED => {
                s.conn.insert(key_id);
            }
            Kind::CLIENT_LEFT => {
                s.conn.remove(&key_id);
            }
            Kind::DETACHMENT => {
                s.cell.att = Attachment::Det;
                s.conn.clear();
                s.resuming = None;
                s.detach_deadline = Some(ev.timestamp + t_detach);
            }
            Kind::RESUME_PREPARED => s.record = Some(ev.clone()),
            Kind::RESUME_STARTED => {
                s.cell.att = Attachment::Res;
                s.resuming = Some(key_id);
                s.resume_from = p["seq"].as_i64().unwrap_or(0);
                s.detach_deadline = None;
            }
            Kind::RESUME_VERIFIED => {
                s.cell.att = Attachment::Att;
                s.conn = s.resuming.take().into_iter().collect();
            }
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
                    shown_hash: p["shown_hash"].as_str().unwrap_or("").to_string(),
                    answer: None,
                });
            }
            Kind::TOOL_CALL_ANSWERED => {
                let key = p["key"].as_str().unwrap_or("");
                for c in s.queue.iter_mut().filter(|c| c.key == key) {
                    c.answer = Some((
                        p["decision"].as_str() == Some("approve"),
                        p["shown_hash"].as_str().unwrap_or("").to_string(),
                    ));
                }
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
        if self.sessions.contains_key(sid) || !self.storage.read_log(sid)?.is_empty() {
            return Err(BridgeError::new("session already exists (I6)"));
        }
        self.storage.register_key(&key.key_id, &key.secret)?;
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

    /// CLIENT_ADDED: a connected client authorises a further key for the
    /// same principal (I15d). `material` is what the [`Verifier`] needs.
    pub fn add_client_key(&mut self, sid: &str, via_key_id: &str, new_key_id: &str, material: &[u8]) -> Result<()> {
        let s = self.enter(sid)?;
        if !s.conn.contains(via_key_id) {
            return Err(BridgeError::new("client key registration needs a connected client (I15)"));
        }
        if s.keys.contains(new_key_id) {
            return Err(BridgeError::new("client key already registered"));
        }
        self.storage.register_key(new_key_id, material)?;
        self.commit(sid, vec![(Kind::CLIENT_ADDED, json!({"key_id": new_key_id, "via": via_key_id}))])?;
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

    // ---- tool calls (I2, I13, I14) ----------------------------------------

    pub fn submit_tool_call(
        &mut self,
        sid: &str,
        tool: &str,
        key: Option<&str>,
        permission: Option<&str>,
        content: Option<&Value>,
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
            let default = json!({"tool": tool, "permission": permission});
            let shown_hash = content_hash(content.unwrap_or(&default));
            let expires_at = self.clock.now() + self.cfg.t_call;
            self.commit(
                sid,
                vec![(
                    Kind::TOOL_CALL_QUEUED,
                    json!({"key": key, "tool": tool, "permission": permission,
                           "shown_hash": shown_hash, "expires_at": expires_at}),
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

    /// h(content shown) as the bridge recorded it: what a faithful client
    /// that rendered the same content reports.
    pub fn shown_hash_of(&self, sid: &str, key: &str) -> Result<String> {
        self.sessions
            .get(sid)
            .and_then(|s| s.queue.iter().find(|c| c.key == key))
            .map(|c| c.shown_hash.clone())
            .ok_or_else(|| BridgeError::new(format!("no queued call {key:?}")))
    }

    /// Keys of queued calls that have no answer yet.
    pub fn unanswered_keys(&self, sid: &str) -> Vec<String> {
        self.sessions[sid].queue.iter().filter(|c| c.answer.is_none()).map(|c| c.key.clone()).collect()
    }

    /// TOOL_CALL_ANSWERED: the human's decision on a queued call, with the
    /// hash of what the client displayed (I14). The first answer is final.
    pub fn answer_tool_call(&mut self, sid: &str, key: &str, shown_hash: &str, approve: bool) -> Result<()> {
        let s = self.enter(sid)?;
        let Some(entry) = s.queue.iter().find(|c| c.key == key) else {
            return Err(BridgeError::new(format!("no queued call {key:?} to answer")));
        };
        if entry.answer.is_some() {
            return Err(BridgeError::new("call already answered (I14)"));
        }
        self.commit(
            sid,
            vec![(
                Kind::TOOL_CALL_ANSWERED,
                json!({"key": key, "shown_hash": shown_hash,
                       "decision": if approve { "approve" } else { "deny" }}),
            )],
        )?;
        Ok(())
    }

    /// Rule D: drain in arrival order, revalidating every call (I13, I14).
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
                let base = |reason: &str| json!({"key": c.key, "tool": c.tool, "reason": reason});
                if now >= c.expires_at {
                    (Kind::TOOL_CALL_EXPIRED, base("call_ttl"))
                } else if now >= s.approval_until {
                    (Kind::TOOL_CALL_EXPIRED, base("approval_ttl"))
                } else if c.permission.as_ref().is_some_and(|p| !s.scope.contains(p)) {
                    (Kind::TOOL_CALL_REJECTED, base("scope"))
                } else {
                    match &c.answer {
                        None => (Kind::TOOL_CALL_REJECTED, base("unanswered")),
                        Some((false, _)) => (Kind::TOOL_CALL_REJECTED, base("denied")),
                        Some((true, h)) if *h != c.shown_hash => {
                            (Kind::TOOL_CALL_REJECTED, base("answer_mismatch"))
                        }
                        Some(_) => (
                            Kind::TOOL_CALL_DRAINED,
                            json!({"key": c.key, "tool": c.tool, "authority_hash": ah,
                                   "shown_hash": c.shown_hash}),
                        ),
                    }
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

    /// RESUME_PREPARED: the bridge's own resume record, once per detachment (I1).
    fn prepared(&self, s: &Session, reason: &str) -> Item {
        let calls: Vec<Value> =
            s.queue.iter().map(|c| json!({"key": c.key, "call": c.tool})).collect();
        (
            Kind::RESUME_PREPARED,
            json!({
                "authority_hash": authority_hash(&s.principal, &s.scope, &s.lineage),
                "reason": reason,
                "authority_scope": s.scope,
                "scope_version": s.scope_version,
                "pending_tool_calls": calls,
            }),
        )
    }

    /// Transport loss of one client (`key_id`) or of every connected client
    /// (`None`). Only the last departure detaches (I15b).
    pub fn client_disconnect(&mut self, sid: &str, key_id: Option<&str>, reason: &str) -> Result<()> {
        let s = self.enter(sid)?;
        if s.cell.att == Attachment::Det {
            return Ok(());
        }
        let targets: BTreeSet<String> = if s.cell.att == Attachment::Res {
            match (key_id, &s.resuming) {
                (Some(k), Some(r)) if k != r => return Ok(()),
                (_, Some(r)) => BTreeSet::from([r.clone()]),
                _ => return Ok(()),
            }
        } else {
            match key_id {
                None => s.conn.clone(),
                Some(k) => s.conn.iter().filter(|c| c.as_str() == k).cloned().collect(),
            }
        };
        if targets.is_empty() {
            return Ok(());
        }
        let remaining: BTreeSet<_> = if s.cell.att == Attachment::Att {
            s.conn.difference(&targets).cloned().collect()
        } else {
            BTreeSet::new()
        };
        if !remaining.is_empty() {
            let items = targets
                .iter()
                .map(|k| (Kind::CLIENT_LEFT, json!({"key_id": k, "reason": reason})))
                .collect();
            self.commit(sid, items)?;
            return Ok(());
        }
        let items = vec![(Kind::DETACHMENT, json!({"reason": reason})), self.prepared(&s, reason)];
        self.commit(sid, items)?;
        self.check_timers(sid)
    }

    pub fn escalate_to_human(&mut self, sid: &str, reason: &str) -> Result<()> {
        self.enter(sid)?;
        self.commit(sid, vec![(Kind::HUMAN_PENDING, json!({"reason": reason}))])?;
        self.check_timers(sid)
    }

    pub fn human_resolve(&mut self, sid: &str) -> Result<()> {
        let s = self.enter(sid)?;
        if s.cell.human != Human::Pend {
            return Err(BridgeError::new("human_resolve requires Human=Pend"));
        }
        self.commit(
            sid,
            vec![(Kind::HUMAN_RESOLVED, json!({"valid_until": self.clock.now() + self.cfg.t_approve}))],
        )?;
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

    // ---- challenge-response re-attach and join (I4, I8, I9, I11, I15) --------

    /// Issue a fresh single-use nonce (I11b). Not a governance event;
    /// volatile, so a restart voids every nonce.
    pub fn challenge(&mut self, sid: &str) -> Result<Challenge> {
        match self.sessions.get(sid) {
            None => return Err(BridgeError::new(format!("unknown session {sid}"))),
            Some(s) if s.cell.life == Life::Terminated => {
                return Err(BridgeError::new("session is TERMINATED (I6)"))
            }
            _ => {}
        }
        let mut raw = [0u8; 16];
        getrandom::getrandom(&mut raw).map_err(|e| BridgeError::new(format!("no randomness: {e}")))?;
        let nonce = hex::encode(raw);
        let book = self.nonces.entry(sid.to_string()).or_default();
        book.push_back(NonceRec { nonce: nonce.clone(), issued: self.clock.now(), used: false });
        while book.len() > NONCE_RECORDS {
            book.pop_front();
        }
        Ok(Challenge { nonce, ttl_seconds: self.cfg.t_nonce })
    }

    /// Det -> Res on an authentic request; a request while Att is a join
    /// (the replay is returned); while Res it is refused. Every failure is
    /// RESUME_REJECTED with no change of cell (I11e).
    pub fn begin_resume(&mut self, sid: &str, req: &ResumeRequest, transport_replay: bool) -> Result<Vec<Event>> {
        let s = self.enter(sid)?;
        let mut reason = self.reject_reason(sid, &s, req, transport_replay)?;
        if reason.is_none() && s.cell.att == Attachment::Res {
            reason = Some("resume_in_progress");
        }
        if let Some(reason) = reason {
            self.commit(
                sid,
                vec![(
                    Kind::RESUME_REJECTED,
                    json!({"reason": reason, "key_id": req.key_id, "seq": req.seq}),
                )],
            )?;
            return Err(BridgeError::new(format!("re-attach rejected: {reason} (I4/I8/I9/I11)")));
        }
        let body = json!({"key_id": req.key_id, "seq": req.seq, "hash": req.hash,
                          "nonce_digest": digest(&req.nonce)});
        if s.cell.att == Attachment::Att {
            self.commit(sid, vec![(Kind::CLIENT_JOINED, body)])?;
            return self.replay_after(sid, req.seq);
        }
        self.commit(sid, vec![(Kind::RESUME_STARTED, body)])?;
        Ok(vec![])
    }

    /// A further client joins an attached session (I15c).
    pub fn join(&mut self, sid: &str, req: &ResumeRequest, transport_replay: bool) -> Result<Vec<Event>> {
        let s = self.enter(sid)?;
        if s.cell.att != Attachment::Att {
            return Err(BridgeError::new("join requires Att"));
        }
        self.begin_resume(sid, req, transport_replay)
    }

    fn reject_reason(
        &mut self,
        sid: &str,
        s: &Session,
        req: &ResumeRequest,
        replay: bool,
    ) -> Result<Option<&'static str>> {
        if replay {
            return Ok(Some("transport_replay")); // I8
        }
        if !s.keys.contains(&req.key_id) {
            return Ok(Some("unknown_key")); // I15
        }
        let ok = match self.storage.key_material(&req.key_id)? {
            Some(m) => self.verifier.verify(&m, &request_message(sid, &req.nonce, req.seq, &req.hash), &req.signature),
            None => false,
        };
        if !ok {
            return Ok(Some("bad_signature")); // I11c
        }
        let now = self.clock.now();
        let t_nonce = self.cfg.t_nonce;
        let Some(rec) = self.nonces.get_mut(sid).and_then(|b| b.iter_mut().find(|r| r.nonce == req.nonce)) else {
            return Ok(Some("nonce_unknown")); // I11b
        };
        let was_used = rec.used;
        rec.used = true; // I11d: spent by the first authentic presentation
        if was_used {
            return Ok(Some("nonce_used"));
        }
        if now - rec.issued >= t_nonce {
            return Ok(Some("nonce_expired")); // I9
        }
        let log = self.storage.read_log(sid)?;
        let on_chain = req.seq >= 0
            && log.get(req.seq as usize).is_some_and(|e| e.hash() == req.hash);
        if !on_chain {
            return Ok(Some("prefix_mismatch")); // I4
        }
        Ok(None)
    }

    /// The events a client that acknowledged `seq` has not seen (I4).
    pub fn replay_after(&self, sid: &str, seq: i64) -> Result<Vec<Event>> {
        Ok(self.storage.read_log(sid)?.into_iter().skip((seq + 1).max(0) as usize).collect())
    }

    /// Res -> Att once the hash chain verifies; otherwise terminate. Returns
    /// the replay of the events after the acknowledged prefix.
    pub fn complete_resume(&mut self, sid: &str) -> Result<Vec<Event>> {
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
        self.drain(sid)?;
        self.replay_after(sid, s.resume_from)
    }

    // ---- authority (I3, I10) -----------------------------------------------

    fn reject_extension(&mut self, sid: &str, s: &Session, proposed: &[String], source: &str) -> Result<()> {
        let cell = format!("{:?}/{:?}", s.cell.att, s.cell.human);
        self.commit(
            sid,
            vec![(
                Kind::AUTHORITY_EXTENSION_REJECTED,
                json!({"proposed": proposed, "current": s.scope, "cell": cell, "source": source}),
            )],
        )?;
        Ok(())
    }

    /// Reject any widening in every cell; a subset is not an extension.
    /// `Observed`: the runtime changed its own scope; the bridge cannot
    /// prevent that, so it records the rejection, keeps its record and asks
    /// for a revert (I3).
    pub fn attempt_authority_extension(
        &mut self,
        sid: &str,
        proposed: &[String],
        source: Source,
    ) -> Result<AuthorityOutcome> {
        let s = self.enter(sid)?;
        if proposed.iter().all(|p| s.scope.contains(p)) {
            return Ok(AuthorityOutcome::NoChange);
        }
        match source {
            Source::Observed => {
                self.reject_extension(sid, &s, proposed, "observed")?;
                self.revert_requests.push((sid.to_string(), s.scope.clone()));
                Ok(AuthorityOutcome::Revert)
            }
            Source::Requested => {
                self.reject_extension(sid, &s, proposed, "requested")?;
                Err(BridgeError::new("authority extension rejected (I3)"))
            }
        }
    }

    pub fn narrow_authority_scope(&mut self, sid: &str, proposed: &[String]) -> Result<()> {
        let s = self.enter(sid)?;
        let narrower = proposed.iter().all(|p| s.scope.contains(p));
        if !narrower {
            self.reject_extension(sid, &s, proposed, "requested")?;
            return Err(BridgeError::new("authority extension rejected (I3)"));
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
        self.commit(
            sid,
            vec![(Kind::AUTHORITY_NARROWED, json!({"new_scope": proposed, "new_version": version}))],
        )
        .map(|_| ())
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
    /// the process, so each active session comes back detached with an empty
    /// client set and a fresh resume record; nonces are void. A session
    /// whose chain does not verify, or whose key registry is missing a key
    /// the log refers to, is terminated with RECOVERY_FAILED.
    pub fn recover(
        storage: Arc<dyn Storage>,
        clock: Arc<dyn Clock>,
        verifier: Arc<dyn Verifier>,
        cfg: Config,
    ) -> Result<Bridge> {
        let mut b = Bridge::new(storage.clone(), clock, verifier, cfg)?;
        let mut ids = storage.sessions()?;
        ids.sort();
        for sid in ids {
            let log: Vec<Event> = storage.read_log(&sid)?;
            for ev in &log {
                b.apply(ev);
            }
            if b.sessions[&sid].cell.life == Life::Terminated {
                continue;
            }
            let mut check: Result<()> = Ok(());
            for k in &b.sessions[&sid].keys {
                if storage.key_material(k)?.is_none() {
                    check = Err(BridgeError::new(format!("client key registry lost {k}")));
                    break;
                }
            }
            let ok = check.and_then(|_| b.verify_chain(&sid));
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
            let items = vec![
                (Kind::BRIDGE_RECOVERED, json!({"events": log.len()})),
                (Kind::DETACHMENT, json!({"reason": "bridge_restart"})),
                b.prepared(&s, "bridge_restart"),
            ];
            b.commit(&sid, items)?;
            b.check_timers(&sid)?;
        }
        Ok(b)
    }

    // ---- introspection --------------------------------------------------------

    fn verify_chain(&self, sid: &str) -> Result<()> {
        let mut prev = String::new();
        for (i, ev) in self.storage.read_log(sid)?.iter().enumerate() {
            if ev.sequence != i as u64 || ev.prev_hash != prev {
                return Err(BridgeError::new(format!("event chain broken at index {i} (I5)")));
            }
            prev = ev.hash();
        }
        Ok(())
    }

    pub fn events(&self, sid: &str) -> Vec<Event> {
        self.storage.read_log(sid).unwrap_or_default()
    }

    pub fn cell_of(&self, sid: &str) -> Cell {
        self.sessions[sid].cell
    }

    pub fn client_set(&self, sid: &str) -> Vec<String> {
        self.sessions[sid].conn.iter().cloned().collect()
    }

    pub fn registered_keys(&self, sid: &str) -> Vec<String> {
        self.sessions[sid].keys.iter().cloned().collect()
    }

    /// The RESUME_PREPARED event of the current detachment, if any.
    pub fn resume_record(&self, sid: &str) -> Option<Event> {
        self.sessions[sid].record.clone()
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
