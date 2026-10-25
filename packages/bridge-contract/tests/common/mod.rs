//! A file-backed [`Storage`] for tests: one JSON-lines log per session, the
//! key registry in one file, `sync_all` after every append. It proves the
//! [`Storage`] trait is implementable over a real medium; it is not meant
//! for production (no locking across processes, no compaction).

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bridge_contract::*;
use serde_json::{json, Value};

pub struct FileStorage {
    dir: PathBuf,
    lock: Mutex<()>,
    fail_next: AtomicBool,
}

fn io(e: impl std::fmt::Display) -> BridgeError {
    BridgeError::new(format!("file storage: {e}"))
}

fn encode(ev: &Event) -> String {
    json!({
        "kind": ev.kind.as_str(), "session_id": ev.session_id, "sequence": ev.sequence,
        "prev_hash": ev.prev_hash, "payload": ev.payload, "timestamp": ev.timestamp,
    })
    .to_string()
}

fn decode(line: &str) -> Result<Event> {
    let v: Value = serde_json::from_str(line).map_err(io)?;
    Ok(Event {
        kind: Kind::parse(v["kind"].as_str().unwrap_or("")).ok_or_else(|| io("unknown event kind"))?,
        session_id: v["session_id"].as_str().unwrap_or("").to_string(),
        sequence: v["sequence"].as_u64().unwrap_or(0),
        prev_hash: v["prev_hash"].as_str().unwrap_or("").to_string(),
        payload: v["payload"].clone(),
        timestamp: v["timestamp"].as_f64().unwrap_or(0.0),
    })
}

impl FileStorage {
    pub fn new(dir: PathBuf) -> Arc<FileStorage> {
        fs::create_dir_all(dir.join("logs")).expect("storage directory");
        Arc::new(FileStorage { dir, lock: Mutex::new(()), fail_next: AtomicBool::new(false) })
    }

    fn log_path(&self, sid: &str) -> PathBuf {
        self.dir.join("logs").join(format!("{}.jsonl", hex::encode(sid)))
    }

    fn keys_path(&self) -> PathBuf {
        self.dir.join("keys.json")
    }

    fn read_keys(&self) -> HashMap<String, String> {
        fs::read_to_string(self.keys_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Test hook: rewrite the stored `prev_hash` of one event.
    pub fn tamper_prev_hash(&self, sid: &str, index: usize) {
        let mut log = self.read_log(sid).unwrap();
        log[index].prev_hash = "00".repeat(32);
        let body: String = log.iter().map(|e| encode(e) + "\n").collect();
        fs::write(self.log_path(sid), body).unwrap();
    }

    pub fn fail_next(&self) {
        self.fail_next.store(true, Ordering::SeqCst);
    }
}

impl Storage for FileStorage {
    fn append_batch(&self, sid: &str, events: Vec<Event>) -> Result<()> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(io("append refused"));
        }
        let _g = self.lock.lock().unwrap();
        // one write of the whole batch, then fsync: the batch is committed
        // when `sync_all` returns
        let body: String = events.iter().map(|e| encode(e) + "\n").collect();
        let mut f = OpenOptions::new().create(true).append(true).open(self.log_path(sid)).map_err(io)?;
        f.write_all(body.as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)
    }

    fn read_log(&self, sid: &str) -> Result<Vec<Event>> {
        let _g = self.lock.lock().unwrap();
        let Ok(f) = File::open(self.log_path(sid)) else { return Ok(vec![]) };
        BufReader::new(f).lines().map(|l| decode(&l.map_err(io)?)).collect()
    }

    fn sessions(&self) -> Result<Vec<String>> {
        let mut out = vec![];
        for e in fs::read_dir(self.dir.join("logs")).map_err(io)? {
            let name = e.map_err(io)?.file_name().to_string_lossy().to_string();
            if let Some(h) = name.strip_suffix(".jsonl") {
                out.push(String::from_utf8(hex::decode(h).map_err(io)?).map_err(io)?);
            }
        }
        Ok(out)
    }

    fn register_key(&self, key_id: &str, material: &[u8]) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut keys = self.read_keys();
        keys.insert(key_id.to_string(), hex::encode(material));
        let mut f = File::create(self.keys_path()).map_err(io)?;
        f.write_all(serde_json::to_string(&keys).map_err(io)?.as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)
    }

    fn key_material(&self, key_id: &str) -> Result<Option<Vec<u8>>> {
        let _g = self.lock.lock().unwrap();
        Ok(self.read_keys().get(key_id).and_then(|h| hex::decode(h).ok()))
    }
}
