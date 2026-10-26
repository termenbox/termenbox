//! `--setup-cloudflare` and `--teardown` (DECISION-0140): put the owner's side of the assisted connection into the
//! owner's own Cloudflare account, and take it out again.
//!
//! Setup creates a Realtime TURN key, uploads the embedded signaling Worker (`infra/owner-relay/dist/`, a SQLite
//! Durable Object) as `termenbox-relay-<id>`, turns on its `workers.dev` route and writes `relay.json`, the file
//! `signal::Config::load` reads. The account API token comes in as an argument from stdin, is used for this run and
//! is never stored, logged or put in an error. Only the TURN key id and the token that mints credentials from it are
//! kept, mode 0600 (guardrail rule 4). Both directions are idempotent.
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::signal::CONFIG_FILE;

pub const API: &str = "https://api.cloudflare.com/client/v4";

/// The Worker and its Durable Object migration, built from `infra/owner-relay` (a test there fails when this copy is
/// stale).
pub const WORKER_JS: &[u8] = include_bytes!("../../../infra/owner-relay/dist/worker.js");
pub const MIGRATIONS: &str = include_str!("../../../infra/owner-relay/dist/migrations.json");

const COMPATIBILITY_DATE: &str = "2024-09-23";

pub struct Params<'a> {
    pub api: &'a str,
    pub dir: &'a Path,
    pub server_id: &'a str,
    /// Which account, by name or id, when the token sees several.
    pub account: Option<&'a str>,
    /// The `workers.dev` subdomain to create when the account has none.
    pub subdomain: Option<&'a str>,
    pub cap_gb: u64,
}

/// The route the Worker serves for one server: a hash, so the URL says nothing about the key it belongs to.
pub fn route_id(server_id: &str) -> String {
    B64.encode(Sha256::digest(server_id.as_bytes()))
}

/// Worker names are lowercase letters, digits and dashes.
pub fn worker_name(server_id: &str) -> String {
    let d = Sha256::digest(server_id.as_bytes());
    let hex: String = d.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("termenbox-relay-{hex}")
}

struct Api {
    http: reqwest::Client,
    base: String,
    token: String,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Value,
    #[serde(default)]
    errors: Vec<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    message: String,
}

impl Api {
    fn new(base: &str, token: &str) -> Api {
        Api {
            // Cloudflare answers 403 (error 1010) to a client without a User-Agent (FACT-0059).
            http: reqwest::Client::builder()
                .user_agent(concat!("termenbox-server/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    /// `Ok(None)` for a 404. Any other failure names the call (`what`) and, for 401 and 403, the missing permission.
    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Body>,
        what: &str,
    ) -> Result<Option<Value>> {
        let mut req = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .bearer_auth(&self.token);
        match body {
            Some(Body::Json(v)) => req = req.json(&v),
            Some(Body::Multipart(bytes, ctype)) => {
                req = req.header("content-type", ctype).body(bytes)
            }
            None => {}
        }
        let resp = req
            .send()
            .await
            .map_err(|e| anyhow!("{what}: cannot reach Cloudflare ({})", e.without_url()))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let env: Envelope = serde_json::from_str(&text).unwrap_or(Envelope {
            result: Value::Null,
            errors: vec![],
        });
        if !status.is_success() {
            let msgs: Vec<_> = env.errors.iter().map(|e| e.message.as_str()).collect();
            let hint = if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                " (the token lacks a permission for this call)"
            } else {
                ""
            };
            bail!(
                "{what}: HTTP {}: {}{hint}",
                status.as_u16(),
                msgs.join("; ")
            );
        }
        Ok(Some(env.result))
    }
}

enum Body {
    Json(Value),
    Multipart(Vec<u8>, String),
}

/// A `multipart/form-data` body of file parts, built by hand so no extra reqwest feature is needed.
fn multipart(parts: &[(&str, &str, &str, &[u8])]) -> (Vec<u8>, String) {
    let boundary = format!("----tb{}", B64.encode(rand::random::<[u8; 9]>()));
    let mut out = Vec::new();
    for (name, file, ctype, content) in parts {
        out.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{file}\"\r\nContent-Type: {ctype}\r\n\r\n").as_bytes());
        out.extend(*content);
        out.extend(b"\r\n");
    }
    out.extend(format!("--{boundary}--\r\n").as_bytes());
    (out, format!("multipart/form-data; boundary={boundary}"))
}

/// What the file holds. Read loosely so a teardown still works on a file an older version wrote.
#[derive(Default, Deserialize)]
struct Saved {
    #[serde(default)]
    turn_key_id: String,
    #[serde(default)]
    turn_key_token: String,
    #[serde(default)]
    worker: String,
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

fn load_saved(dir: &Path) -> Saved {
    std::fs::read_to_string(config_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

async fn account_id(api: &Api, want: Option<&str>) -> Result<String> {
    let list = api
        .call(
            Method::GET,
            "/accounts",
            None,
            "list accounts (needs Account Settings: Read)",
        )
        .await?
        .unwrap_or(Value::Null);
    let mut accounts: Vec<(String, String)> = list
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x["id"].as_str()?.to_string(),
                        x["name"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if accounts.is_empty() {
        bail!("the token sees no Cloudflare account");
    }
    if let Some(w) = want {
        accounts.retain(|(id, name)| id == w || name == w);
        if accounts.is_empty() {
            bail!("no account named or numbered {w:?} is visible to the token");
        }
    }
    if accounts.len() > 1 {
        let names: Vec<_> = accounts.iter().map(|(_, n)| n.as_str()).collect();
        bail!(
            "the token sees several accounts ({}); pick one with --cloudflare-account NAME",
            names.join(", ")
        );
    }
    Ok(accounts.remove(0).0)
}

#[derive(Debug)]
pub struct Report {
    pub url: String,
    pub worker: String,
    pub created_key: bool,
    pub cap_gb: u64,
}

impl Report {
    pub fn render(&self) -> String {
        format!(
            "Worker:        {} (deployed)\nTURN key:      {}\nSignaling URL: {}\n\n\
             Cloudflare's free plan includes 1,000 GB of TURN traffic a month; Termenbox stops asking for a relay at \
             {} GB (TERMENBOX_RELAY_CAP_GB). The account token was not stored; relay.json holds only the TURN key id \
             and its credential token.\nRestart the server to use it. Undo everything with: termenbox-server --teardown\n",
            self.worker,
            if self.created_key { "created" } else { "kept from the last run" },
            self.url,
            self.cap_gb,
        )
    }
}

pub async fn setup(token: &str, p: &Params<'_>) -> Result<Report> {
    setup_with(token, p, WORKER_JS, MIGRATIONS).await
}

async fn setup_with(
    token: &str,
    p: &Params<'_>,
    worker: &[u8],
    migrations: &str,
) -> Result<Report> {
    let api = Api::new(p.api, token);
    let acc = account_id(&api, p.account).await?;
    let name = worker_name(p.server_id);
    let saved = load_saved(p.dir);

    // The Worker's name is derived from the server key, so a script of that name that this directory never deployed
    // belongs to someone else (or to a lost directory): do not overwrite it.
    let exists = api
        .call(
            Method::GET,
            &format!("/accounts/{acc}/workers/scripts/{name}/settings"),
            None,
            "look for an existing Worker (needs Workers Scripts: Edit)",
        )
        .await?
        .is_some();
    if exists && saved.worker != name {
        bail!(
            "a Worker named {name} already exists in this account and this server did not deploy it; \
             run --teardown to remove it, or rename it in the Cloudflare dashboard"
        );
    }

    let sub = api
        .call(
            Method::GET,
            &format!("/accounts/{acc}/workers/subdomain"),
            None,
            "read the workers.dev subdomain (needs Workers Scripts: Edit)",
        )
        .await?
        .and_then(|v| v["subdomain"].as_str().map(str::to_string));
    let sub = match (sub, p.subdomain) {
        (Some(s), _) => s,
        (None, Some(want)) => {
            api.call(
                Method::PUT,
                &format!("/accounts/{acc}/workers/subdomain"),
                Some(Body::Json(json!({ "subdomain": want }))),
                "create the workers.dev subdomain",
            )
            .await?;
            want.to_string()
        }
        (None, None) => bail!(
            "this account has no workers.dev subdomain; run again with --workers-subdomain NAME to create one"
        ),
    };

    // The TURN key's secret is shown once, at creation: keep the one we have.
    let have_key = !saved.turn_key_id.is_empty() && !saved.turn_key_token.is_empty();
    let (key_id, key_token) = if have_key {
        (saved.turn_key_id.clone(), saved.turn_key_token.clone())
    } else {
        let k = api
            .call(
                Method::POST,
                &format!("/accounts/{acc}/calls/turn_keys"),
                Some(Body::Json(json!({ "name": name }))),
                "create the TURN key (needs Cloudflare Calls: Edit)",
            )
            .await?
            .unwrap_or(Value::Null);
        match (k["uid"].as_str(), k["secret"].as_str()) {
            (Some(u), Some(s)) => (u.to_string(), s.to_string()),
            _ => bail!("create the TURN key: Cloudflare's answer holds no key"),
        }
    };

    let deployed = deploy(&api, &acc, &name, worker, migrations).await;
    if let Err(e) = deployed {
        // A key made by this run that nothing refers to would be lost for good (its secret is gone): remove it.
        if !have_key {
            let _ = api
                .call(
                    Method::DELETE,
                    &format!("/accounts/{acc}/calls/turn_keys/{key_id}"),
                    None,
                    "delete the TURN key",
                )
                .await;
        }
        return Err(e);
    }

    let url = format!("wss://{name}.{sub}.workers.dev/s/{}", route_id(p.server_id));
    let file = json!({
        "signal": url,
        "turn_key_id": key_id,
        "turn_key_token": key_token,
        "worker": name,
    });
    crate::auth::write_private(
        &config_path(p.dir),
        serde_json::to_string_pretty(&file)?.as_bytes(),
    )
    .context("cannot write relay.json")?;
    Ok(Report {
        url,
        worker: name,
        created_key: !have_key,
        cap_gb: p.cap_gb,
    })
}

async fn deploy(api: &Api, acc: &str, name: &str, worker: &[u8], migrations: &str) -> Result<()> {
    let migs: Value =
        serde_json::from_str(migrations).context("the embedded migration is invalid")?;
    let first = migs
        .get(0)
        .ok_or_else(|| anyhow!("the embedded migration is empty"))?;
    let meta = json!({
        "main_module": "worker.js",
        "compatibility_date": COMPATIBILITY_DATE,
        "bindings": [{ "type": "durable_object_namespace", "name": "RELAY", "class_name": "Relay" }],
        "migrations": { "new_tag": first["tag"], "new_sqlite_classes": first["new_sqlite_classes"] },
    });
    let meta = serde_json::to_vec(&meta)?;
    let (body, ctype) = multipart(&[
        ("metadata", "metadata.json", "application/json", &meta),
        (
            "worker.js",
            "worker.js",
            "application/javascript+module",
            worker,
        ),
    ]);
    api.call(
        Method::PUT,
        &format!("/accounts/{acc}/workers/scripts/{name}"),
        Some(Body::Multipart(body, ctype)),
        "upload the Worker (needs Workers Scripts: Edit; a free account can use SQLite Durable Objects)",
    )
    .await?;
    api.call(
        Method::POST,
        &format!("/accounts/{acc}/workers/scripts/{name}/subdomain"),
        Some(Body::Json(json!({ "enabled": true }))),
        "turn on the workers.dev route",
    )
    .await?;
    Ok(())
}

/// Removes the Worker (and with it its Durable Object), the TURN key and `relay.json`. Whatever is already gone is
/// skipped, so a second run, or one after a half-finished setup, ends clean.
pub async fn teardown(token: &str, p: &Params<'_>) -> Result<Vec<String>> {
    let api = Api::new(p.api, token);
    let acc = account_id(&api, p.account).await?;
    let saved = load_saved(p.dir);
    let name = if saved.worker.is_empty() {
        worker_name(p.server_id)
    } else {
        saved.worker.clone()
    };
    let mut said = Vec::new();
    let gone = api
        .call(
            Method::DELETE,
            &format!("/accounts/{acc}/workers/scripts/{name}?force=true"),
            None,
            "delete the Worker (needs Workers Scripts: Edit)",
        )
        .await?
        .is_none();
    said.push(format!(
        "Worker {name}: {}",
        if gone { "was not there" } else { "deleted" }
    ));
    if !saved.turn_key_id.is_empty() {
        let gone = api
            .call(
                Method::DELETE,
                &format!("/accounts/{acc}/calls/turn_keys/{}", saved.turn_key_id),
                None,
                "delete the TURN key (needs Cloudflare Calls: Edit)",
            )
            .await?
            .is_none();
        said.push(format!(
            "TURN key: {}",
            if gone { "was not there" } else { "deleted" }
        ));
    }
    for f in [CONFIG_FILE, "relay_usage.json"] {
        if std::fs::remove_file(p.dir.join(f)).is_ok() {
            said.push(format!("removed {f}"));
        }
    }
    Ok(said)
}

/// The account token: one line from stdin; on a terminal, asked for with echo off.
pub fn read_token() -> Result<String> {
    use std::io::{BufRead, IsTerminal, Write};
    let tty = std::io::stdin().is_terminal();
    #[cfg(unix)]
    let _echo = if tty {
        eprint!("Cloudflare account API token (hidden): ");
        let _ = std::io::stderr().flush();
        EchoOff::new()
    } else {
        None
    };
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if tty {
        eprintln!();
    }
    let token = line.trim().to_string();
    if token.is_empty() {
        bail!("no token given: pipe the Cloudflare API token in on stdin");
    }
    Ok(token)
}

#[cfg(unix)]
struct EchoOff(libc::termios);

#[cfg(unix)]
impl EchoOff {
    fn new() -> Option<EchoOff> {
        // SAFETY: tcgetattr/tcsetattr on stdin with a zeroed termios that tcgetattr fills.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return None;
            }
            let saved = t;
            t.c_lflag &= !libc::ECHO;
            libc::tcsetattr(0, libc::TCSANOW, &t);
            Some(EchoOff(saved))
        }
    }
}

#[cfg(unix)]
impl Drop for EchoOff {
    fn drop(&mut self) {
        // SAFETY: restores the attributes read in `new`.
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path as AxPath, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    const ACCOUNT_TOKEN: &str = "ACCOUNT-TOKEN-never-on-disk";

    #[derive(Default)]
    struct Cf {
        accounts: Vec<(String, String)>,
        subdomain: Option<String>,
        scripts: HashSet<String>,
        keys: HashMap<String, String>,
        uploads: usize,
        /// Path fragments the token may not call (403), standing in for a missing scope.
        deny: Vec<String>,
        next_key: usize,
    }

    type Shared = Arc<Mutex<Cf>>;

    fn denied(cf: &Cf, headers: &HeaderMap, path: &str) -> Option<(StatusCode, Json<Value>)> {
        let authed = headers
            .get("authorization")
            .is_some_and(|v| v == format!("Bearer {ACCOUNT_TOKEN}").as_str())
            && headers.contains_key("user-agent");
        if !authed || cf.deny.iter().any(|d| path.contains(d.as_str())) {
            return Some((
                StatusCode::FORBIDDEN,
                Json(
                    json!({ "success": false, "errors": [{ "message": "Authentication error" }] }),
                ),
            ));
        }
        None
    }

    fn ok(v: Value) -> (StatusCode, Json<Value>) {
        (
            StatusCode::OK,
            Json(json!({ "success": true, "result": v })),
        )
    }

    fn missing() -> (StatusCode, Json<Value>) {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "errors": [] })),
        )
    }

    async fn fake(cf: Cf) -> (String, Shared) {
        let shared: Shared = Arc::new(Mutex::new(cf));
        let app = Router::new()
            .route(
                "/accounts",
                get(|State(s): State<Shared>, h: HeaderMap| async move {
                    let cf = s.lock().unwrap();
                    denied(&cf, &h, "/accounts").unwrap_or_else(|| {
                        ok(Value::Array(
                            cf.accounts
                                .iter()
                                .map(|(i, n)| json!({ "id": i, "name": n }))
                                .collect(),
                        ))
                    })
                }),
            )
            .route(
                "/accounts/:a/workers/subdomain",
                get(|State(s): State<Shared>, h: HeaderMap| async move {
                    let cf = s.lock().unwrap();
                    denied(&cf, &h, "workers").unwrap_or_else(|| match &cf.subdomain {
                        Some(sd) => ok(json!({ "subdomain": sd })),
                        None => missing(),
                    })
                })
                .put(
                    |State(s): State<Shared>, h: HeaderMap, Json(b): Json<Value>| async move {
                        let mut cf = s.lock().unwrap();
                        if let Some(d) = denied(&cf, &h, "workers") {
                            return d;
                        }
                        cf.subdomain = b["subdomain"].as_str().map(str::to_string);
                        ok(b)
                    },
                ),
            )
            .route(
                "/accounts/:a/workers/scripts/:n/settings",
                get(
                    |State(s): State<Shared>,
                     h: HeaderMap,
                     AxPath((_, n)): AxPath<(String, String)>| async move {
                        let cf = s.lock().unwrap();
                        denied(&cf, &h, "workers").unwrap_or_else(|| {
                            if cf.scripts.contains(&n) {
                                ok(json!({}))
                            } else {
                                missing()
                            }
                        })
                    },
                ),
            )
            .route(
                "/accounts/:a/workers/scripts/:n",
                axum::routing::put(
                    |State(s): State<Shared>,
                     h: HeaderMap,
                     AxPath((_, n)): AxPath<(String, String)>,
                     body: axum::body::Bytes| async move {
                        let mut cf = s.lock().unwrap();
                        if let Some(d) = denied(&cf, &h, "workers") {
                            return d;
                        }
                        let text = String::from_utf8_lossy(&body);
                        assert!(text.contains("new_sqlite_classes") && text.contains("worker.js"));
                        cf.scripts.insert(n);
                        cf.uploads += 1;
                        ok(json!({}))
                    },
                )
                .delete(
                    |State(s): State<Shared>,
                     h: HeaderMap,
                     AxPath((_, n)): AxPath<(String, String)>| async move {
                        let mut cf = s.lock().unwrap();
                        denied(&cf, &h, "workers").unwrap_or_else(|| {
                            if cf.scripts.remove(&n) {
                                ok(json!({}))
                            } else {
                                missing()
                            }
                        })
                    },
                ),
            )
            .route(
                "/accounts/:a/workers/scripts/:n/subdomain",
                post(|State(s): State<Shared>, h: HeaderMap| async move {
                    let cf = s.lock().unwrap();
                    denied(&cf, &h, "workers").unwrap_or_else(|| ok(json!({ "enabled": true })))
                }),
            )
            .route(
                "/accounts/:a/calls/turn_keys",
                post(|State(s): State<Shared>, h: HeaderMap| async move {
                    let mut cf = s.lock().unwrap();
                    if let Some(d) = denied(&cf, &h, "calls") {
                        return d;
                    }
                    cf.next_key += 1;
                    let (id, secret) = (
                        format!("key-{}", cf.next_key),
                        format!("secret-{}", cf.next_key),
                    );
                    cf.keys.insert(id.clone(), secret.clone());
                    ok(json!({ "uid": id, "secret": secret }))
                }),
            )
            .route(
                "/accounts/:a/calls/turn_keys/:k",
                axum::routing::delete(
                    |State(s): State<Shared>,
                     h: HeaderMap,
                     AxPath((_, k)): AxPath<(String, String)>| async move {
                        let mut cf = s.lock().unwrap();
                        denied(&cf, &h, "calls").unwrap_or_else(|| {
                            if cf.keys.remove(&k).is_some() {
                                ok(json!({}))
                            } else {
                                missing()
                            }
                        })
                    },
                ),
            )
            .with_state(shared.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, shared)
    }

    fn account() -> Vec<(String, String)> {
        vec![("acc1".into(), "Owner".into())]
    }

    fn params<'a>(api: &'a str, dir: &'a Path) -> Params<'a> {
        Params {
            api,
            dir,
            server_id: "SERVERID",
            account: None,
            subdomain: None,
            cap_gb: 500,
        }
    }

    fn base_cf() -> Cf {
        Cf {
            accounts: account(),
            subdomain: Some("owner".into()),
            ..Cf::default()
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tb-cf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn setup_writes_the_route_and_the_key() {
        let dir = tmp("route");
        let (api, cf) = fake(base_cf()).await;
        let r = setup(ACCOUNT_TOKEN, &params(&api, &dir)).await.unwrap();
        assert!(r.url.starts_with(&format!(
            "wss://{}.owner.workers.dev/s/",
            worker_name("SERVERID")
        )));
        let cfg = crate::signal::Config::load(&dir)
            .unwrap()
            .expect("relay.json loads");
        assert_eq!(cfg.signal, r.url);
        assert_eq!(
            (cfg.turn_key_id.as_str(), cfg.turn_key_token.as_str()),
            ("key-1", "secret-1")
        );
        assert_eq!(cf.lock().unwrap().scripts.len(), 1);
        assert!(r.render().contains("1,000 GB") && r.render().contains("500 GB"));
    }

    #[tokio::test]
    async fn setup_token_not_persisted() {
        let dir = tmp("token");
        let (api, _cf) = fake(base_cf()).await;
        setup(ACCOUNT_TOKEN, &params(&api, &dir)).await.unwrap();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let text = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(!String::from_utf8_lossy(&text).contains(ACCOUNT_TOKEN));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(CONFIG_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn setup_cloudflare_rerun_changes_nothing() {
        let dir = tmp("rerun");
        let (api, cf) = fake(base_cf()).await;
        let first = setup(ACCOUNT_TOKEN, &params(&api, &dir)).await.unwrap();
        let file = std::fs::read_to_string(dir.join(CONFIG_FILE)).unwrap();
        let second = setup(ACCOUNT_TOKEN, &params(&api, &dir)).await.unwrap();
        assert_eq!(first.url, second.url);
        assert!(!second.created_key);
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).unwrap(),
            file
        );
        let cf = cf.lock().unwrap();
        assert_eq!((cf.keys.len(), cf.scripts.len()), (1, 1));
    }

    #[tokio::test]
    async fn setup_cloudflare_names_the_missing_scope_and_leaves_nothing() {
        let dir = tmp("scope");
        let (api, cf) = fake(Cf {
            deny: vec!["workers".into()],
            ..base_cf()
        })
        .await;
        let e = setup(ACCOUNT_TOKEN, &params(&api, &dir))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("lacks a permission") && e.contains("Workers Scripts"),
            "{e}"
        );
        assert!(!e.contains(ACCOUNT_TOKEN));
        assert!(!dir.join(CONFIG_FILE).exists());
        assert!(cf.lock().unwrap().keys.is_empty());

        // The key scope missing: the Worker is not deployed either, and the error names the Calls scope.
        let (api, cf) = fake(Cf {
            deny: vec!["calls".into()],
            ..base_cf()
        })
        .await;
        let e = setup(ACCOUNT_TOKEN, &params(&api, &dir))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("Cloudflare Calls"), "{e}");
        assert!(cf.lock().unwrap().scripts.is_empty());
    }

    #[tokio::test]
    async fn setup_cloudflare_refuses_a_worker_it_did_not_deploy() {
        let dir = tmp("collide");
        let mut cf = base_cf();
        cf.scripts.insert(worker_name("SERVERID"));
        let (api, cf) = fake(cf).await;
        let e = setup(ACCOUNT_TOKEN, &params(&api, &dir))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("already exists"), "{e}");
        let cf = cf.lock().unwrap();
        assert_eq!(cf.uploads, 0);
        assert!(cf.keys.is_empty());
    }

    #[tokio::test]
    async fn setup_cloudflare_removes_a_new_key_when_the_upload_fails() {
        let dir = tmp("halfway");
        let (api, cf) = fake(base_cf()).await;
        let r = setup_with(ACCOUNT_TOKEN, &params(&api, &dir), b"x", "not json").await;
        assert!(r.is_err());
        assert!(cf.lock().unwrap().keys.is_empty());
        assert!(!dir.join(CONFIG_FILE).exists());
    }

    #[tokio::test]
    async fn setup_cloudflare_needs_a_subdomain_or_a_choice_of_account() {
        let dir = tmp("choices");
        let (api, _cf) = fake(Cf {
            subdomain: None,
            ..base_cf()
        })
        .await;
        let e = setup(ACCOUNT_TOKEN, &params(&api, &dir))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("--workers-subdomain"), "{e}");
        let mut p = params(&api, &dir);
        p.subdomain = Some("fresh");
        assert!(setup(ACCOUNT_TOKEN, &p)
            .await
            .unwrap()
            .url
            .contains(".fresh.workers.dev/"));

        let mut two = base_cf();
        two.accounts.push(("acc2".into(), "Other".into()));
        let (api, _cf) = fake(two).await;
        let e = setup(ACCOUNT_TOKEN, &params(&api, &dir))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("--cloudflare-account"), "{e}");
    }

    #[tokio::test]
    async fn teardown_leaves_nothing_behind() {
        let dir = tmp("teardown");
        let (api, cf) = fake(base_cf()).await;
        setup(ACCOUNT_TOKEN, &params(&api, &dir)).await.unwrap();
        let said = teardown(ACCOUNT_TOKEN, &params(&api, &dir)).await.unwrap();
        assert!(said.iter().any(|l| l.contains("deleted")));
        {
            let cf = cf.lock().unwrap();
            assert!(cf.scripts.is_empty() && cf.keys.is_empty());
        }
        assert!(!dir.join(CONFIG_FILE).exists());
        // Again: nothing to delete, nothing fails.
        let said = teardown(ACCOUNT_TOKEN, &params(&api, &dir)).await.unwrap();
        assert!(said[0].contains("was not there"));
    }

    #[test]
    fn names_fit_cloudflare_and_the_route() {
        let n = worker_name("a/b+c");
        assert!(
            n.len() <= 63
                && n.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
        let r = route_id("a/b+c");
        assert!(
            (16..=64).contains(&r.len())
                && r.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn the_embedded_worker_is_the_built_one() {
        assert!(!WORKER_JS.is_empty());
        assert!(MIGRATIONS.contains("Relay"));
    }
}
