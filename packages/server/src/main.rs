use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::HeaderMap,
    response::Response,
    routing::get,
    Router,
};
use clap::Parser;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

mod acp;
mod acp_stream;
mod adapters;
mod agent_hooks;
mod agentwatch;
mod attention_meter;
mod auth;
mod backend;
mod bundle;
mod candidates;
mod cert;
mod checkpoint;
mod claude_hooks;
mod cloudflare;
mod command_match;
mod conductor;
mod conductor_link;
mod conductor_terminal;
mod connection;
mod container;
mod contract;
mod control;
mod crash;
mod directory;
#[cfg(test)]
mod e2e;
mod event_log;
mod events;
mod health;
mod helper;
mod history;
mod journal;
mod layout;
mod legacy;
mod link;
mod listdir;
mod locale;
mod logging;
mod netutil;
mod opencode_hooks;
mod owner;
mod ports;
mod profile;
mod pty;
mod push;
mod reach;
mod reducer;
mod registry;
mod release;
mod requests;
mod rtc;
mod signal;
mod stun;
mod tabstatus;
mod terminfo;
mod tmuxconf;
mod transcript;
mod turn;
mod uninstall;
mod update_state;
mod updater;
mod websocket;
mod workspaces;

use auth::AuthState;
use backend::{CommandBackend, RunMode, TmuxBackend, WorkspaceBackend};
use health::HealthCollector;
use websocket::handle_websocket;
use workspaces::WorkspaceManager;

#[derive(Parser, Debug)]
#[command(name = "termenbox-server", version = RELEASE_VERSION)]
struct Args {
    #[arg(long, env = "TERMENBOX_HOST", default_value = "0.0.0.0")]
    host: String,

    #[arg(long, env = "TERMENBOX_PORT", default_value = "8080")]
    port: u16,

    /// Where the owner, keys and certificate are kept (a volume in Docker). Default: ~/.config/termenbox
    #[arg(long, env = "TERMENBOX_CONFIG_DIR", default_value_os_t = default_config_dir())]
    config_dir: PathBuf,

    /// Name shown in the client and typed to confirm a remote uninstall. Default: the machine's hostname.
    #[arg(long, env = "TERMENBOX_SERVER_NAME", default_value_t = default_server_name())]
    server_name: String,

    /// Client that opens the registration link.
    #[arg(
        long,
        env = "TERMENBOX_CLIENT_URL",
        default_value = "https://app.termenbox.dev"
    )]
    client_url: String,

    /// Workspace manager: `tmux` (built in), `acp` (agents the server starts), `command` (any CLI, see
    /// TERMENBOX_BACKEND_*), a comma list of them, or `auto` (tmux, plus acp when TERMENBOX_ACP=on).
    #[arg(long, env = "TERMENBOX_BACKEND", default_value = "auto")]
    backend: String,

    /// `on` lets `auto` offer the built-in ACP agents that are installed; `off` (default) keeps `auto` at tmux, so workspace
    /// ids stay bare (with two managers they are `tmux:<name>`). Off until the adapters' terms of use are settled (0137).
    #[arg(long, env = "TERMENBOX_ACP", default_value = "off")]
    acp: String,

    /// For `command`: a label shown in logs, e.g. zellij.
    #[arg(long, env = "TERMENBOX_BACKEND_NAME", default_value = "command")]
    backend_name: String,

    /// For `command`: prints one workspace per line (`id` or `id|name`).
    #[arg(long, env = "TERMENBOX_BACKEND_LIST")]
    backend_list: Option<String>,

    /// For `command`: creates a detached workspace; `{name}` is replaced.
    #[arg(long, env = "TERMENBOX_BACKEND_CREATE")]
    backend_create: Option<String>,

    /// For `command`: attaches a terminal to a workspace; `{id}` is replaced.
    #[arg(long, env = "TERMENBOX_BACKEND_ATTACH")]
    backend_attach: Option<String>,

    /// Where workspaces run: `container` (default, isolated), `native` (the server runs on the host as a
    /// systemd service, decision 0075) or `host` (the retired privileged-container mode, needs --pid=host --privileged).
    #[arg(long, env = "TERMENBOX_RUN", default_value = "container")]
    run: String,

    /// For `host`: the user workspaces run as, `root` or a dedicated unprivileged user.
    #[arg(long, env = "TERMENBOX_RUN_USER", default_value = "termenbox")]
    run_user: String,

    /// UDP port of the WebRTC-direct transport. Without it, a first start binds the first free port from
    /// 4443 to 4542 and saves it; every later start uses that port or fails, never another (decision 0118).
    #[arg(long, env = "TERMENBOX_UDP_PORT")]
    udp_port: Option<u16>,

    /// Which tmux server holds the workspaces: `termenbox` (its own server and config, decisions 0120 and 0121, the
    /// default) or `default` (the user's own tmux server and config; the tmux termenbox runs must then be the same
    /// version as that server, or it cannot attach).
    #[arg(long, env = "TERMENBOX_TMUX_SOCKET")]
    tmux_socket: Option<String>,

    /// Address clients connect to over WebRTC; detected when not set.
    #[arg(long, env = "TERMENBOX_PUBLIC_IP")]
    public_ip: Option<std::net::IpAddr>,

    /// Turn the WebRTC-direct transport off (WebSocket only).
    #[arg(long, env = "TERMENBOX_WEBRTC", default_value = "on")]
    webrtc: String,

    /// Reach this server over the LAN only (decision 0123): no directory publisher, no public-IP lookup, no router
    /// port mapping. Pair a phone with `--print-pairing`.
    #[arg(
        long,
        env = "TERMENBOX_LAN_ONLY",
        default_value = "false",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    lan_only: bool,

    /// The LAN addresses to put in the candidate bundle, comma-separated, instead of the ones found on the
    /// interfaces (at most 4 are kept).
    #[arg(long, env = "TERMENBOX_LAN_ADDRS")]
    lan_addrs: Option<String>,

    /// Report this node's attention to a conductor (decision 0170): a conductor pairing link, enrols once and is
    /// remembered; `off` stops the link and keeps the pin. Without it, and with nothing remembered, the node dials nothing.
    #[arg(long, env = "TERMENBOX_CONDUCTOR")]
    conductor: Option<String>,

    /// Act as a conductor (decision 0170): accept enrolled nodes' links and merge their attention. Off by default;
    /// an ordinary server never answers a node.
    #[arg(long, env = "TERMENBOX_ACCEPT_NODES", default_value = "false")]
    accept_nodes: bool,

    /// Print the link and QR that enrol a node into the conductor running here (decision 0170), then exit. The
    /// one-time secret comes from the running server (started with --accept-nodes) and is shown only here.
    #[arg(long, default_value = "false")]
    conductor_enrol: bool,

    /// Print the pairing link and QR for this server (a signed candidate bundle; works with no directory) and exit.
    #[arg(long, default_value = "false")]
    print_pairing: bool,

    /// Serve /rtc-info with the unsigned connection details. For development and tests only.
    #[arg(long, env = "TERMENBOX_DEV_INFO", default_value = "off")]
    dev_info: String,

    /// One-time secret generated by the client and carried in the install command; claims the server.
    #[arg(long, env = "TERMENBOX_CLAIM_SECRET")]
    claim_secret: Option<String>,

    /// Directory to announce to (defaults to the client's origin). Empty turns announcing off.
    #[arg(long, env = "TERMENBOX_DIRECTORY_URL")]
    directory_url: Option<String>,

    /// One-shot, run by the installer on an existing install: start a new claim with this secret,
    /// replacing any owner. Whoever can run this has root on the host, which already allows the same.
    #[arg(long)]
    claim: Option<String>,

    /// With --register: draw the QR even when stdout is not a terminal (used through `docker exec`).
    #[arg(long, default_value = "false")]
    qr: bool,

    /// How many days of agent history the server keeps (decision 0105).
    #[arg(
        long,
        env = "TERMENBOX_HISTORY_DAYS",
        default_value_t = history::DEFAULT_DAYS,
        value_parser = clap::value_parser!(u32).range(1..=3650)
    )]
    history_days: u32,

    /// The most disk the agent history may use, in MB (decision 0105).
    #[arg(
        long,
        env = "TERMENBOX_HISTORY_MAX_MB",
        default_value_t = history::DEFAULT_MAX_MB,
        value_parser = clap::value_parser!(u64).range(1..=1000)
    )]
    history_max_mb: u64,

    /// Directory of the change journal (root-only); see decision 0067.
    #[arg(
        long,
        env = "TERMENBOX_JOURNAL_DIR",
        default_value = "/var/lib/termenbox-root"
    )]
    journal_dir: PathBuf,

    /// One-shot, for the installer: record a change of this kind (file_created, file_replaced, user_created,
    /// package_installed, unit_installed, firewall_rule_added) before applying it. Fields: --journal-field K=V.
    #[arg(long)]
    journal_record: Option<String>,

    #[arg(long = "journal-field", value_parser = parse_field)]
    journal_fields: Vec<(String, String)>,

    /// One-shot: list the changes that have not been reverted.
    #[arg(long, default_value = "false")]
    journal_list: bool,

    /// One-shot: undo the journaled changes, newest first. Modified files are reported, not removed.
    #[arg(long, default_value = "false")]
    revert: bool,

    /// With --revert: only this kind of change.
    #[arg(long)]
    only: Option<String>,

    /// With --revert: show what would be undone and change nothing.
    #[arg(long, default_value = "false")]
    dry_run: bool,

    /// With --revert: also delete the home directory of a user we created (the owner's explicit choice).
    #[arg(long, default_value = "false")]
    delete_home: bool,

    /// One-shot: verify a signed release manifest (path) against the embedded release keys and print its fields
    /// as key=value lines. Needs --release-sig; add --release-arch to print that architecture's download.
    #[arg(long)]
    verify_release: Option<PathBuf>,

    #[arg(long)]
    release_sig: Option<PathBuf>,

    #[arg(long)]
    release_arch: Option<String>,

    /// One-shot, run as root by the systemd units the installer creates: `update` checks for a new signed release and
    /// applies it (rolling back if the service does not come up); `tick` acts on a passed grace deadline (no network
    /// otherwise); `requests` handles what the server asked for. By
    /// hand: `status` (read-only), `check` (never installs), `apply`, `postpone` and `skip`.
    #[arg(long)]
    root_helper: Option<String>,

    /// With `--root-helper status`: print the update state as JSON.
    #[arg(long, default_value = "false")]
    json: bool,

    /// With --root-helper: this is the no-root install (decision 0113). The helper runs as the user who owns the
    /// service, never as root, and restarts it with `systemctl --user`.
    #[arg(long, default_value = "false")]
    user_helper: bool,

    /// Where the unprivileged server leaves requests for the root helper.
    #[arg(
        long,
        env = "TERMENBOX_REQUESTS_DIR",
        default_value = "/var/lib/termenbox/requests"
    )]
    requests_dir: PathBuf,

    /// Print the registration link for this (unclaimed) server and exit.
    #[arg(long, default_value = "false")]
    register: bool,

    /// What an agent tool runs on each of its events (`claude`): copies its JSON to the local event socket.
    /// Never fails and never waits long. Used by the entries `--connect-claude` writes.
    #[arg(long)]
    hook: Option<String>,

    /// One-shot, opt-in (decision 0079): add Termenbox's hooks to Claude Code's settings. Shows what it would
    /// do; add --yes to apply. `--disconnect-claude` removes exactly that again.
    #[arg(long, default_value = "false")]
    connect_claude: bool,

    #[arg(long, default_value = "false")]
    disconnect_claude: bool,

    /// One-shot, opt-in (decisions 0079 and 0106): write Termenbox's plugin into OpenCode's config directory.
    /// Shows what it would do; add --yes to apply. `--disconnect-opencode` removes exactly that again.
    #[arg(long, default_value = "false")]
    connect_opencode: bool,

    #[arg(long, default_value = "false")]
    disconnect_opencode: bool,

    /// The OpenCode config directory to change (the plugin goes to `<dir>/plugins/termenbox.js`). Default:
    /// $OPENCODE_CONFIG_DIR, else $XDG_CONFIG_HOME/opencode, else ~/.config/opencode.
    #[arg(long)]
    opencode_dir: Option<PathBuf>,

    /// One-shot, opt-in (decision 0141): set up the assisted connection in your own Cloudflare account (a signaling
    /// Worker and a TURN key). The account API token is read from stdin, used once and never stored.
    #[arg(long, default_value = "false")]
    setup_cloudflare: bool,

    /// One-shot: delete what --setup-cloudflare made (the Worker, its Durable Object and the TURN key). Reads the
    /// account API token from stdin like --setup-cloudflare.
    #[arg(long, default_value = "false")]
    teardown: bool,

    /// With --setup-cloudflare or --teardown: the account, by name or id, when the token sees several.
    #[arg(long)]
    cloudflare_account: Option<String>,

    /// With --setup-cloudflare: the workers.dev subdomain to create when the account has none.
    #[arg(long)]
    workers_subdomain: Option<String>,

    /// Creates the Web Push key (VAPID) in `connectors.env` in the config directory, mode 0600, and prints the
    /// public key. Restart the server afterwards. Does nothing when a key is already there.
    #[arg(long)]
    setup_push: bool,

    /// With --connect-claude or --connect-opencode: apply instead of only showing the change.
    #[arg(long, default_value = "false")]
    yes: bool,

    /// The Claude Code settings file to change. Default: $CLAUDE_CONFIG_DIR/settings.json, else ~/.claude/settings.json
    #[arg(long)]
    claude_settings: Option<PathBuf>,
}

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<AuthState>,
    pub workspaces: Arc<WorkspaceManager>,
    pub server_name: String,
    /// `container` or `host:<user>`, shown to the owner.
    pub mode: String,
    pub rtc_info: Option<rtc::RtcInfo>,
    /// The signed candidate bundle pushed to every client after it authenticates; set once the transport is up.
    pub paths: Arc<std::sync::OnceLock<Arc<candidates::PathHub>>>,
    /// Set when this server accepts nodes (`--accept-nodes`, decision 0170); `None` makes it an ordinary server.
    pub conductor: Option<Arc<conductor::Conductor>>,
}

/// The crate's version, unless a release build sets `TERMENBOX_RELEASE_VERSION` to the full SemVer
/// (`0.1.36-nightly.20261022`), so a prerelease needs no change to `Cargo.toml`.
const RELEASE_VERSION: &str = match option_env!("TERMENBOX_RELEASE_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// `0.1.0+abc1234`: the version and the commit it was built from. A release binary has the commit baked in at build
/// time (`TERMENBOX_BUILD`); the Docker image sets it in the environment instead. Neither: `dev`, which compares newer
/// than a prerelease of the same core, so a release build must never lack it.
pub fn version() -> String {
    let build = option_env!("TERMENBOX_BUILD")
        .map(str::to_string)
        .or_else(|| std::env::var("TERMENBOX_BUILD").ok())
        .unwrap_or_else(|| "dev".into());
    format_version(RELEASE_VERSION, &build)
}

fn format_version(release: &str, build: &str) -> String {
    format!("{release}+{}", build.chars().take(7).collect::<String>())
}

fn parse_field(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .ok_or_else(|| "expected KEY=VALUE".to_string())
}

/// The journal commands run as the installer or an operator, never as the server.
fn run_journal_command(args: &Args) -> anyhow::Result<bool> {
    if let Some(kind) = &args.journal_record {
        let journal = journal::Journal::open(&args.journal_dir)?;
        let id = journal.record(journal::Change::from_fields(kind, &args.journal_fields)?)?;
        println!("recorded change {id} ({kind})");
        return Ok(true);
    }
    if args.journal_list {
        for (id, change) in journal::Journal::open(&args.journal_dir)?.pending()? {
            println!("{id} {}", serde_json::to_string(&change)?);
        }
        return Ok(true);
    }
    if args.revert {
        let journal = journal::Journal::open(&args.journal_dir)?;
        let opts = journal::RevertOptions {
            only: args.only.clone(),
            dry_run: args.dry_run,
            delete_home: args.delete_home,
        };
        let report = journal.revert(&mut journal::RealOps, &opts)?;
        let verb = if args.dry_run {
            "would revert"
        } else {
            "reverted"
        };
        println!("{verb}: {:?}", report.reverted);
        for (id, why) in &report.skipped {
            println!("skipped {id}: {why}");
        }
        for (id, why) in &report.failed {
            println!("failed {id}: {why}");
        }
        if !report.failed.is_empty() {
            std::process::exit(1);
        }
        return Ok(true);
    }
    Ok(false)
}

/// `--verify-release`: for the installer's second check and the updater; exit status 1 means do not trust it.
fn run_release_command(args: &Args) -> anyhow::Result<bool> {
    let Some(manifest_path) = &args.verify_release else {
        return Ok(false);
    };
    let sig_path = args
        .release_sig
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("--verify-release needs --release-sig"))?;
    let keys = release::ReleaseKeys::from_env()?;
    anyhow::ensure!(
        !keys.is_empty(),
        "no release keys configured (TERMENBOX_RELEASE_KEYS)"
    );
    let (manifest, kid) = release::Manifest::verify(
        &std::fs::read(manifest_path)?,
        &std::fs::read_to_string(sig_path)?,
        &keys,
        release::URL_PREFIX,
        release::Channel::of_version(&version()),
    )?;
    println!("kid={kid}");
    println!("version={}", manifest.version);
    println!("freeze={}", manifest.freeze);
    println!("newer={}", manifest.is_newer_than(&version())?);
    if let Some(arch) = &args.release_arch {
        let b = manifest.binary(arch)?;
        println!("url={}", b.url);
        println!("sha256={}", b.sha256);
    }
    Ok(true)
}

fn default_config_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib"))
        .join(".config/termenbox")
}

/// The UDP port the server bound last time (decision 0118); absent or unreadable means "pick one".
fn saved_udp_port(dir: &std::path::Path) -> Option<u16> {
    std::fs::read_to_string(dir.join("udp_port"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Remembers the bound port so a restart keeps it and the firewall rule stays valid. The file is only a
/// hint (the bound socket is the real guard), so it is written to a temporary file and renamed rather
/// than locked: a concurrent reader then never sees a half-written number.
fn save_udp_port(dir: &std::path::Path, port: u16) -> std::io::Result<()> {
    let tmp = dir.join(format!("udp_port.{}.tmp", std::process::id()));
    std::fs::write(&tmp, port.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, dir.join("udp_port")) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// The host's short name (`vps-1` for `vps-1.example.com`), cleaned for display; `termenbox-server` when unknown.
fn default_server_name() -> String {
    let raw = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| std::fs::read_to_string("/etc/hostname"))
        .unwrap_or_default();
    clean_server_name(&raw)
}

fn clean_server_name(raw: &str) -> String {
    let short = raw.trim().split('.').next().unwrap_or("");
    let name: String = short
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    if name.is_empty() || name == "localhost" {
        "termenbox-server".into()
    } else {
        name
    }
}

/// `TERMENBOX_ACP`: `on` or `off`, nothing else.
fn parse_acp(v: &str) -> anyhow::Result<bool> {
    match v {
        "on" => Ok(true),
        "off" => Ok(false),
        other => Err(anyhow::anyhow!(
            "unknown TERMENBOX_ACP `{other}` (on or off)"
        )),
    }
}

/// `TERMENBOX_BACKEND`: one manager (`tmux`, `acp`, `command`), a comma list of them (taken as written), or `auto`
/// (tmux, plus acp when `acp_on`). Registering acp is free: no child starts until a workspace is created.
fn resolve_backends(spec: &str, acp_on: bool) -> anyhow::Result<Vec<String>> {
    let mut kinds: Vec<String> = if spec == "auto" {
        let mut v = vec!["tmux".to_string()];
        if acp_on {
            v.push("acp".to_string());
        }
        v
    } else {
        spec.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    kinds.dedup();
    anyhow::ensure!(!kinds.is_empty(), "TERMENBOX_BACKEND is empty");
    anyhow::ensure!(
        !kinds.iter().any(|k| k == "herdr"),
        "TERMENBOX_BACKEND=herdr is no longer supported: herdr was removed (decision 0097); use tmux"
    );
    Ok(kinds)
}

fn build_backend(
    args: &Args,
    kind: &str,
    mode: &RunMode,
    env: events::EventEnv,
    prefix: &str,
) -> anyhow::Result<Arc<dyn WorkspaceBackend>> {
    match kind {
        "tmux" => Ok(Arc::new(
            TmuxBackend::new(mode.runner())
                .with_events(env)
                .with_event_prefix(prefix),
        )),
        // ACP agents the server starts itself (decision 0111): the built-ins and the owner's allowlist file.
        "acp" => {
            let config = acp::AcpConfig::load(&args.config_dir.join("acp-agents.toml"))?;
            let backend = Arc::new(
                acp::AcpBackend::new(config)
                    .with_events(env.hub.clone())
                    .with_state_file(args.config_dir.join("acp-workspaces.json"))
                    .with_legacy_state_file(args.config_dir.join("acp-sessions.json"))
                    .with_safe_mode(crash::safe_mode_ids())
                    .with_event_prefix(prefix),
            );
            backend.spawn_restore();
            backend.spawn_heartbeat();
            Ok(backend)
        }
        "command" => {
            let need = |v: &Option<String>, name: &str| {
                v.clone()
                    .ok_or_else(|| anyhow::anyhow!("TERMENBOX_BACKEND=command needs {name}"))
            };
            Ok(Arc::new(CommandBackend::with_runner(
                &args.backend_name,
                &need(&args.backend_list, "TERMENBOX_BACKEND_LIST")?,
                &need(&args.backend_create, "TERMENBOX_BACKEND_CREATE")?,
                &need(&args.backend_attach, "TERMENBOX_BACKEND_ATTACH")?,
                mode.runner(),
            )?))
        }
        other => Err(anyhow::anyhow!(
            "unknown TERMENBOX_BACKEND `{other}` (tmux, acp, command or auto)"
        )),
    }
}

fn print_registration(args: &Args, auth: &AuthState) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    // The same block as --print-pairing. Before the first start there is no port to sign, so fall back to the bare links.
    if let Ok(block) = pairing_block(args, auth) {
        if let Some(owner) = auth.owner() {
            println!("This server is registered to @{}.", owner.login);
        }
        print!(
            "{}",
            block.render(args.qr || std::io::stdout().is_terminal())
        );
        return Ok(());
    }
    if let Some(owner) = auth.owner() {
        println!("This server is registered to @{}.", owner.login);
        // A browser that forgot the server (removed it, or a new device) can add it again by id: no secret needed.
        let id = directory::ServerKey::load_or_create(&args.config_dir)?.server_id();
        let link = auth::server_link(&args.client_url, &id);
        if args.qr || std::io::stdout().is_terminal() {
            if let Some(qr) = auth::claim_qr(&link) {
                println!("{qr}");
            }
        }
        println!(
            "To add it to a browser signed in as @{}, scan the code or open:\n\n  {link}\n",
            owner.login
        );
        return Ok(());
    }
    if auth.is_claimed() {
        anyhow::bail!(
            "owner.json exists but cannot be read; fix or remove it, then run --register again"
        );
    }
    let claim = auth
        .claim_token()?
        .ok_or_else(|| anyhow::anyhow!("no claim token"))?;
    let link = auth::claim_link(&args.client_url, &claim);
    if args.qr || std::io::stdout().is_terminal() {
        if let Some(qr) = auth::claim_qr(&link) {
            println!("{qr}");
        }
    }
    println!("Scan the code with Termenbox (or your phone's camera), or open:\n\n  {link}\n");
    println!("The first user to connect becomes the owner. The code works for 15 minutes, once.");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Before logging starts, so the output is tmux's alone. `termenbox-server tmux <args>`: the same tmux, socket and config the server uses (decision 0120).
    if std::env::args().nth(1).as_deref() == Some("tmux") {
        return run_tmux_passthrough();
    }

    logging::init();

    let args = Args::parse();
    if let Some(adapter) = &args.hook {
        events::run_hook(adapter);
        return Ok(());
    }
    if args.setup_cloudflare || args.teardown {
        return run_cloudflare(&args).await;
    }
    if args.setup_push {
        match push::setup(&args.config_dir) {
            Ok(msg) => println!("{msg}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.connect_claude || args.disconnect_claude {
        // A disconnect follows the file the connect changed (the journal says which), not the environment of
        // whoever runs it: the uninstaller does not have the service's CLAUDE_CONFIG_DIR.
        let settings = args
            .claude_settings
            .clone()
            .or_else(|| {
                args.disconnect_claude
                    .then(|| claude_hooks::journal_settings(&args.config_dir))
                    .flatten()
            })
            .unwrap_or_else(claude_hooks::default_settings_path);
        let outcome = if args.connect_claude {
            let binary = std::env::current_exe()?.display().to_string();
            claude_hooks::connect(&settings, &binary, &args.config_dir, args.yes)
        } else {
            claude_hooks::disconnect(&settings, &args.config_dir)
        };
        match outcome {
            Ok(o) => println!("{}", o.0),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.connect_opencode || args.disconnect_opencode {
        // As for Claude Code, a removal follows the journal: the uninstaller may not have the environment the
        // connect was run with.
        let plugin = args
            .opencode_dir
            .clone()
            .map(|d| d.join("plugins").join("termenbox.js"))
            .or_else(|| {
                args.disconnect_opencode
                    .then(|| opencode_hooks::journal_plugin(&args.config_dir))
                    .flatten()
            })
            .unwrap_or_else(opencode_hooks::default_plugin_path);
        let outcome = if args.connect_opencode {
            let binary = std::env::current_exe()?.display().to_string();
            opencode_hooks::connect(&plugin, &binary, &args.config_dir, args.yes)
        } else {
            opencode_hooks::disconnect(&plugin, &args.config_dir)
        };
        match outcome {
            Ok(o) => println!("{}", o.0),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if run_journal_command(&args)? {
        return Ok(());
    }
    if let Some(action) = &args.root_helper {
        let setup = helper::Setup {
            root_dir: args.journal_dir.clone(),
            requests_dir: args.requests_dir.clone(),
            user: args.user_helper,
        };
        match helper::run(action, &setup, args.json).await {
            Ok(status) => std::process::exit(status),
            Err(e) => {
                eprintln!("root helper: {e}");
                std::process::exit(1);
            }
        }
    }
    if let Err(e) = run_release_command(&args) {
        eprintln!("release check failed: {e}");
        std::process::exit(1);
    }
    if args.verify_release.is_some() {
        return Ok(());
    }
    // An uninstalled server that Docker restarted stays idle instead of coming back as a new unclaimed
    // server; the installer's --claim removes the marker, which ends the idle wait so Docker restarts it.
    if args.claim.is_none() && !args.register && uninstall::is_uninstalled(&args.config_dir) {
        println!("This server was uninstalled. Remove its container, or run the installer again to set it up.");
        while uninstall::is_uninstalled(&args.config_dir) {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        return Ok(());
    }

    let keys = cert::CertKeys::from_env()?;
    anyhow::ensure!(
        !keys.is_empty(),
        "no certificate keys configured (TERMENBOX_CERT_KEYS)"
    );
    std::fs::create_dir_all(&args.config_dir)?;
    let server_id = directory::ServerKey::load_or_create(&args.config_dir)?.server_id();
    let auth = Arc::new(AuthState::new(args.config_dir.clone(), keys, server_id)?);

    if let Some(secret) = &args.claim {
        auth.reclaim(secret)?;
        println!("Claim started; the server will announce it within seconds.");
        return Ok(());
    }

    if args.register {
        return print_registration(&args, &auth);
    }

    if args.print_pairing {
        return print_pairing(&args, &auth);
    }
    if args.conductor_enrol {
        return print_conductor_enrol(&args).await;
    }

    if let Some(secret) = &args.claim_secret {
        auth.provide_claim(secret)?;
    }

    // Only the server run path keeps a log file; a hook or a one-shot command does not.
    logging::enable_file(&args.config_dir);
    // After the log file exists, so the first line of a bad run is already in it: the hook writes the crash file and the
    // marker says whether the last run ended without a clean exit (decision 0142).
    crash::install_panic_hook(&args.config_dir);
    let previous_run = crash::start(&args.config_dir);
    previous_run.log();
    let _clean_exit = crash::CleanExit;

    match auth.owner() {
        Some(owner) => info!("Server owner: @{}", owner.login),
        None if auth.is_claimed() => {
            tracing::warn!("owner.json exists but cannot be read; treating the server as claimed");
        }
        None => {
            auth.claim_token()?;
            info!("Server is unclaimed; run `termenbox-server --register` to get the registration link");
        }
    }

    let mode = RunMode::parse(&args.run, &args.run_user)?;
    mode.self_check()
        .await
        .map_err(|e| anyhow::anyhow!("TERMENBOX_RUN={}: {e}", mode.label()))?;
    choose_tmux_server(&args, &mode).await?;
    if mode.is_host() {
        info!(target: "audit", event = "host_mode_enabled", user = %mode.label());
    }
    // History is optional: a file that cannot be opened or rewritten costs the Activity view, not the server.
    let history = match history::History::open(
        &args.config_dir,
        history::Limits::new(args.history_days, args.history_max_mb),
        events::now_ms(),
    ) {
        Ok(h) => Some(Arc::new(h)),
        Err(e) => {
            tracing::warn!("history disabled: {e:#}");
            None
        }
    };
    let mut hub = events::EventHub::load_or_create(&args.config_dir)?;
    hub = hub.with_tab_status(Arc::new(tabstatus::TabStatus::default()));
    if let Some(history) = &history {
        hub = hub.with_history(history.clone());
    }
    // The attention meter is optional too: without it the Home line is empty, nothing else changes.
    match attention_meter::AttentionMeter::open(&args.config_dir, events::now_ms()) {
        Ok(meter) => {
            let meter = Arc::new(meter);
            hub = hub.with_meter(meter.clone());
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    let meter = meter.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Err(e) = meter.flush() {
                            tracing::warn!("attention meter not saved: {e:#}");
                        }
                    })
                    .await;
                }
            });
        }
        Err(e) => tracing::warn!("attention meter disabled: {e:#}"),
    }
    let socket = events::socket_path(&args.config_dir);
    // Which workspace an event belongs to is checked against the panes; the runner arrives with the backends below.
    let owners = Arc::new(owner::Resolver::default());
    tokio::spawn({
        let (hub, socket, owners) = (hub.clone(), socket.clone(), owners.clone());
        async move {
            if let Err(e) = events::serve_socket(hub, socket, owners).await {
                tracing::warn!("agent event socket: {e}");
            }
        }
    });
    let push = push::Push::load(&args.config_dir);
    push::spawn_watcher(push.clone(), hub.clone());
    let conductor_pin = match conductor_link::configure(&args.config_dir, args.conductor.as_deref())
    {
        Ok(pin) => pin,
        Err(e) => {
            tracing::warn!("conductor link not started: {e:#}");
            None
        }
    };
    let conductor = args.accept_nodes.then(|| {
        let c = Arc::new(conductor::Conductor::new(
            Some(args.config_dir.clone()),
            events::now_ms(),
        ));
        tokio::spawn({
            let (c, path) = (c.clone(), conductor::enrol_socket_path(&args.config_dir));
            async move {
                if let Err(e) = conductor::serve_enrol_socket(c, path).await {
                    tracing::warn!("conductor enrolment socket: {e}");
                }
            }
        });
        tokio::spawn({
            let c = c.clone();
            async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    c.tick(events::now_ms());
                }
            }
        });
        c
    });
    if let Some(dir) = args.requests_dir.parent() {
        push::spawn_update_watcher(push.clone(), dir.to_path_buf(), args.server_name.clone());
    }
    // A `working` signal that goes quiet decays to `unknown` without any event to trigger it.
    tokio::spawn({
        let hub = hub.clone();
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                hub.refresh(events::now_ms());
            }
        }
    });
    // Records past their age go once an hour; the size limit is kept as records are written.
    if let Some(history) = history {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                let history = history.clone();
                let _ =
                    tokio::task::spawn_blocking(move || history.maintain(events::now_ms())).await;
            }
        });
    }
    let kinds = resolve_backends(&args.backend, parse_acp(&args.acp)?)?;
    // With several managers every workspace id is `manager:name`; with one it stays the plain name.
    let prefix_of = |k: &str| {
        if kinds.len() > 1 {
            format!("{k}:")
        } else {
            String::new()
        }
    };
    let window_hub = control::WindowHub::default();
    let mut tmux_version = String::new();
    let mut control_on = false;
    let mut tmux_prefix = String::new();
    for k in &kinds {
        match k.as_str() {
            "tmux" => {
                // A tmux server that outlived the last upgrade still has the old config.
                mode.runner().reload_tmux_config().await;
                agentwatch::spawn_tmux_scan(hub.clone(), mode.runner(), prefix_of(k));
                owners.attach(mode.runner(), prefix_of(k));
                // Control mode needs tmux 3.2; an older one keeps the scan and says so (plan H.3).
                let out = mode
                    .runner()
                    .run("tmux", &["-V".to_string()])
                    .await
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default();
                tmux_version = control::version_label(&out);
                tmux_prefix = runner_prefix(&mode).await;
                if control::control_supported(&out) {
                    control_on = true;
                    control::spawn(window_hub.clone(), mode.runner(), prefix_of(k));
                } else {
                    info!("tmux {tmux_version}: live window updates need tmux 3.2 or newer; using the periodic scan");
                }
            }
            _ => {}
        }
    }
    let env = events::EventEnv {
        hub: hub.clone(),
        socket,
    };
    let backend = if kinds.len() > 1 {
        let mut parts: Vec<(String, Arc<dyn WorkspaceBackend>)> = Vec::new();
        for k in &kinds {
            parts.push((
                k.clone(),
                build_backend(&args, k, &mode, env.clone(), &prefix_of(k))?,
            ));
        }
        Arc::new(backend::CompositeBackend::new(parts)) as Arc<dyn WorkspaceBackend>
    } else {
        build_backend(&args, &kinds[0], &mode, env, "")?
    };
    info!("Workspace backend: {}", backend.name());
    let registry = Arc::new(registry::Registry::load(
        args.config_dir.join("workspaces.json"),
    ));
    // Server health (U14): sampled every 10 s into memory; the data directory is the disk it reports.
    let health = Arc::new(HealthCollector::new());
    health.spawn(args.config_dir.clone());
    let binary = std::env::current_exe()?.display().to_string();
    // A plugin the owner connected earlier is brought up to this version's; OpenCode reads it on its next start.
    if let Some(done) = opencode_hooks::refresh(&opencode_hooks::Target {
        auto: args.opencode_dir.is_none(),
        plugin: args
            .opencode_dir
            .clone()
            .map(|d| d.join("plugins").join("termenbox.js"))
            .unwrap_or_else(opencode_hooks::default_plugin_path),
        binary: binary.clone(),
        state_dir: args.config_dir.clone(),
    }) {
        info!(target: "audit", event = "opencode_plugin_refreshed", detail = %done.0);
    }
    let mut state = AppState {
        auth,
        workspaces: Arc::new(
            if control_on {
                WorkspaceManager::new(backend, mode.clone(), registry)
                    .with_windows(window_hub.clone(), tmux_version.clone())
            } else {
                WorkspaceManager::new(backend, mode.clone(), registry)
                    .with_tmux_version(tmux_version.clone())
            }
            .with_prefix(tmux_prefix.clone())
            .with_push(push.clone())
            .with_events(hub.clone())
            .with_conductor(conductor.clone())
            .with_health(health)
            .with_previous_exit(previous_run.message())
            .with_agent_hooks(agent_hooks::Targets {
                claude: claude_hooks::Target {
                    auto: args.claude_settings.is_none(),
                    settings: args
                        .claude_settings
                        .clone()
                        .unwrap_or_else(claude_hooks::default_settings_path),
                    binary: binary.clone(),
                    state_dir: args.config_dir.clone(),
                },
                opencode: opencode_hooks::Target {
                    auto: args.opencode_dir.is_none(),
                    plugin: args
                        .opencode_dir
                        .clone()
                        .map(|d| d.join("plugins").join("termenbox.js"))
                        .unwrap_or_else(opencode_hooks::default_plugin_path),
                    binary,
                    state_dir: args.config_dir.clone(),
                },
            })
            .with_requests(args.requests_dir.clone())
            .with_uninstall(
                args.server_name.clone(),
                Arc::new(|| {
                    // Let the reply reach the client, then stop; Docker then finds the idle marker.
                    std::thread::spawn(|| {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        crash::mark_clean();
                        std::process::exit(0);
                    });
                }),
            ),
        ),
        server_name: args.server_name.clone(),
        mode: mode.label(),
        rtc_info: None,
        paths: Arc::default(),
        conductor,
    };
    // Started once the workspace manager exists: it applies the answers a conductor routes to this node.
    if let Some(pin) = conductor_pin {
        let node = conductor_link::Node {
            key: directory::ServerKey::load_or_create(&args.config_dir)?,
            name: args.server_name.clone(),
            version: version(),
            hub: hub.clone(),
            applier: Some(state.workspaces.clone()),
        };
        let backoff = signal::Backoff::new(
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(60),
        );
        tokio::spawn(conductor_link::run(
            args.config_dir.clone(),
            pin,
            node,
            backoff,
        ));
    }

    let mut reach: Option<reach::Handle> = None;
    if args.webrtc != "off" {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        let lan_only = args.lan_only;
        // LAN-only never looks outside. Otherwise a host that cannot find its public address (no internet) still
        // serves its LAN and overlay candidates instead of refusing to start.
        let public_ip = match args.public_ip {
            Some(ip) => Some(ip),
            None if lan_only => None,
            None => {
                let found = netutil::discover_public_ip(&http).await;
                if found.is_none() {
                    tracing::warn!(
                        "cannot find this host's public IP (no internet?): serving LAN and overlay candidates only; \
                         set TERMENBOX_PUBLIC_IP to announce one"
                    );
                }
                found
            }
        };
        let ifaces = candidates::interfaces();
        let lan = candidates::lan_addrs(&ifaces, args.lan_addrs.as_deref());
        // The address ICE-lite declares; clients dial the candidates of the bundle, so with no public address a
        // LAN one (or loopback) stands in.
        let rtc_ip = public_ip.unwrap_or_else(|| {
            lan.first()
                .map(|a| (*a).into())
                .unwrap_or(std::net::Ipv4Addr::LOCALHOST.into())
        });
        let cert = rtc::load_or_create_cert(&args.config_dir)?;
        // The server owns the socket, so it picks the port in one step: no pick-then-bind race
        // (decision 0118). An explicit port wins; otherwise the saved one (never swapped for another); a first start scans the range.
        let bind_ip: std::net::IpAddr = std::net::Ipv4Addr::UNSPECIFIED.into();
        let (socket, port) = rtc::bind_transport(
            bind_ip,
            args.udp_port,
            rtc::UDP_PORT_RANGE,
            saved_udp_port(&args.config_dir),
        )
        .await?;
        // Save the bound port so every later start uses exactly it (the firewall rule is for that port). The socket
        // is already listening, so a config dir we cannot write must not stop the server: warn and go on.
        if let Err(e) = save_udp_port(&args.config_dir, port) {
            tracing::warn!(
                "cannot remember UDP port {port} in {}: {e}",
                args.config_dir.display()
            );
        }
        // STUN shares the transport socket (DECISION-0141): the mapping is only worth anything if the request left it.
        let stun_servers = stun::servers(lan_only, std::env::var("TERMENBOX_STUN").ok().as_deref());
        let socket = Arc::new(socket);
        let (stun_inbox, stun_rx) = stun::channel();
        let (info, assisted) = rtc::start_assisted(
            socket.clone(),
            rtc_ip,
            cert.clone(),
            rtc::Limits::default(),
            state.clone(),
            (!stun_servers.is_empty()).then_some(stun_inbox),
        )
        .await?;
        info!(
            "WebRTC-direct on udp {}:{} ({})",
            info.ip, info.port, info.fingerprint
        );
        // A global IPv6 address gets a socket of its own on the same port, so a `wan6` candidate is real.
        let mut v6 = None;
        // TERMENBOX_NO_IPV6: for a host whose IPv6 does not route, and for tests that must not have a direct way in.
        if !lan_only && std::env::var_os("TERMENBOX_NO_IPV6").is_none() {
            if let Some(addr) = candidates::global_ipv6(&ifaces) {
                match tokio::net::UdpSocket::bind((addr, port)).await {
                    Ok(socket) => {
                        rtc::start_with_socket(
                            socket,
                            addr.into(),
                            cert,
                            rtc::Limits::default(),
                            state.clone(),
                        )
                        .await?;
                        info!("WebRTC-direct also on udp [{addr}]:{port}");
                        v6 = Some(addr);
                    }
                    Err(e) => tracing::warn!("cannot listen on IPv6 {addr}: {e}"),
                }
            }
        }
        // Where this host sits and whether the router will open the port (R64); never in LAN-only mode.
        let handle = if lan_only {
            println!("Reachability\n  LAN-only: no directory, no public-IP lookup, no router port mapping.\n  Next: pair a phone with `termenbox-server --print-pairing`");
            reach::Handle::idle().0
        } else {
            reach::spawn(
                reach::Config {
                    port,
                    gateway: reach::detect_gateway(),
                    // TERMENBOX_ASSUME_NAT: a host whose public address is a 1:1 NAT's, or a test host that must not count as public.
                    host_public4: std::env::var_os("TERMENBOX_ASSUME_NAT").is_none()
                        && netutil::local_route_ip().is_some_and(netutil::is_public_unicast),
                    v6,
                    overlay: !candidates::overlay_addrs(&ifaces, None).is_empty(),
                    seen: None,
                },
                http.clone(),
                true,
            )
        };
        let origin = args
            .directory_url
            .clone()
            .unwrap_or_else(|| args.client_url.clone());
        let publishes = directory::publishes(!lan_only && public_ip.is_some(), &origin);
        let stun_rx_out = if stun_servers.is_empty() {
            tokio::sync::watch::channel(None).1
        } else {
            stun::spawn(socket, stun_rx, stun_servers, handle.rx.clone())
        };
        // The assisted path (DECISION-0141) exists only once the owner has run the Cloudflare setup (step 2.7), and
        // never in LAN-only mode.
        if !lan_only {
            match signal::Config::load(&args.config_dir) {
                Ok(Some(cfg)) => {
                    let usage = Arc::new(signal::Usage::open(
                        &args.config_dir,
                        signal::Usage::cap_from_env(
                            std::env::var("TERMENBOX_RELAY_CAP_GB").ok().as_deref(),
                        ),
                    ));
                    println!("{}", usage.status_line());
                    let handler = Arc::new(signal::Signal::new(
                        state.auth.clone(),
                        assisted,
                        signal::Endpoint {
                            host: std::net::SocketAddr::new(info.ip, info.port),
                            mapped: Some(stun_rx_out.clone()),
                        },
                        Some(signal::Minter::new(
                            &signal::credential_api(
                                std::env::var("TERMENBOX_RELAY_API").ok().as_deref(),
                            ),
                            &cfg.turn_key_id,
                            &cfg.turn_key_token,
                        )),
                        usage,
                    ));
                    tokio::spawn(signal::run(
                        signal::dial_url(
                            &cfg.signal,
                            std::env::var("TERMENBOX_SIGNAL_DIAL").ok().as_deref(),
                        ),
                        handler,
                        signal::Backoff::new(
                            std::time::Duration::from_secs(1),
                            std::time::Duration::from_secs(60),
                        ),
                    ));
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("assisted connection is off: {e:#}"),
            }
        }
        let hub = candidates::PathHub::new_with_stun(
            args.config_dir.clone(),
            Arc::new(directory::ServerKey::load_or_create(&args.config_dir)?),
            info.fingerprint.clone(),
            args.server_name.clone(),
            port,
            args.lan_addrs.clone(),
            match public_ip {
                Some(std::net::IpAddr::V4(ip)) => Some(ip),
                _ => None,
            },
            v6,
            publishes,
            handle.rx.clone(),
            stun_rx_out,
        );
        if !lan_only {
            hub.enable_assisted();
        }
        if let Err(e) = hub.refresh() {
            tracing::warn!("cannot sign a candidate bundle: {e:#}");
        }
        tokio::spawn(hub.clone().run());
        let _ = state.paths.set(hub);
        reach = Some(handle);
        let key = Arc::new(directory::ServerKey::load_or_create(&args.config_dir)?);
        info!("Server id {}", key.server_id());
        push.set_server_id(key.server_id());
        directory::spawn_publisher(
            publishes,
            origin,
            key,
            info.clone(),
            state.auth.clone(),
            http,
        );
        state.rtc_info = Some(info);
    }

    let app = router(state, args.dev_info == "on");

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    // On SIGTERM or Ctrl-C the router mapping is taken away again, so the port does not stay open for the rest of
    // its lease.
    tokio::select! {
        r = axum::serve(listener, app) => r?,
        _ = shutdown_signal() => info!("Shutting down"),
    }
    if let Some(reach) = &reach {
        reach.release().await;
    }

    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => tokio::select! {
            _ = term.recv() => {},
            _ = tokio::signal::ctrl_c() => {},
        },
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

/// The pairing block (decision 0126): the signed candidate bundle as a link, QR and (later) a short code. An unclaimed
/// server adds its claim token, so the same link also claims it (decision 0117's preview still comes first); an owned
/// server prints a re-add link with the bundle and no claim. Needs the server to have started once, for its port.
fn pairing_block(args: &Args, auth: &AuthState) -> anyhow::Result<candidates::PairingBlock> {
    let wire = pairing_wire(args)?;
    let claim = if auth.owner().is_some() {
        None
    } else {
        auth.claim_token()?
    };
    Ok(candidates::PairingBlock::new(
        &args.client_url,
        &wire,
        claim.as_deref(),
        None,
    ))
}

/// The signed candidate bundle of this server, as the wire string.
fn pairing_wire(args: &Args) -> anyhow::Result<String> {
    let port = saved_udp_port(&args.config_dir).ok_or_else(|| {
        anyhow::anyhow!("this server has not started yet; start it once, then run --print-pairing")
    })?;
    let cert = rtc::load_or_create_cert(&args.config_dir)?;
    let key = directory::ServerKey::load_or_create(&args.config_dir)?;
    candidates::pairing_bundle(
        &args.config_dir,
        &key,
        &rtc::fingerprint(&cert),
        &args.server_name,
        port,
        args.lan_addrs.as_deref(),
        args.lan_only,
    )
}

/// `--print-pairing`: for a phone that has no directory path to this server (LAN-only, offline, or behind a NAT with no
/// relay), and what `--register` prints too.
/// `--setup-cloudflare` and `--teardown` (decision 0141).
async fn run_cloudflare(args: &Args) -> anyhow::Result<()> {
    anyhow::ensure!(
        !(args.setup_cloudflare && args.teardown),
        "give --setup-cloudflare or --teardown, not both"
    );
    std::fs::create_dir_all(&args.config_dir)?;
    let server_id = directory::ServerKey::load_or_create(&args.config_dir)?.server_id();
    let api = std::env::var("TERMENBOX_CLOUDFLARE_API").unwrap_or_else(|_| cloudflare::API.into());
    let params = cloudflare::Params {
        api: &api,
        dir: &args.config_dir,
        server_id: &server_id,
        account: args.cloudflare_account.as_deref(),
        subdomain: args.workers_subdomain.as_deref(),
        cap_gb: signal::Usage::cap_from_env(
            std::env::var("TERMENBOX_RELAY_CAP_GB").ok().as_deref(),
        ),
    };
    let token = cloudflare::read_token()?;
    let result = if args.setup_cloudflare {
        cloudflare::setup(&token, &params).await.map(|r| r.render())
    } else {
        cloudflare::teardown(&token, &params)
            .await
            .map(|lines| lines.join("\n") + "\n")
    };
    match result {
        Ok(text) => print!("{text}"),
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// `--conductor-enrol`: the link a node runs `--conductor <link>` with. Its signed part is this server's bundle; the
/// secret rides outside it and works once, for 15 minutes (`conductor::ENROL_TTL_MS`).
async fn print_conductor_enrol(args: &Args) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    let wire = pairing_wire(args)?;
    let secret = conductor::request_enrol(&conductor::enrol_socket_path(&args.config_dir)).await?;
    let link = candidates::enrol_link(&args.client_url, &wire, &secret);
    if args.qr || std::io::stdout().is_terminal() {
        if let Some(qr) = auth::claim_qr(&link) {
            println!("{qr}");
        }
    }
    println!(
        "On the node, run:\n\n  termenbox-server --conductor '{link}'\n\nIt works once, for 15 minutes. The node then waits for your tap in the app before its attention joins the stream."
    );
    Ok(())
}

fn print_pairing(args: &Args, auth: &AuthState) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    let block = pairing_block(args, auth)?;
    print!(
        "{}",
        block.render(args.qr || std::io::stdout().is_terminal())
    );
    Ok(())
}

/// No credentials cross origins here: the WebSocket authenticates in its first message.
fn router(state: AppState, dev_info: bool) -> Router {
    let mut app = Router::new()
        .route("/health", get(|| health::HEARTBEATS.respond()))
        .route("/ws", get(ws_handler));
    if dev_info {
        app = app.route("/rtc-info", get(rtc_info));
    }
    app.layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn rtc_info(
    State(state): State<AppState>,
) -> axum::response::Result<axum::Json<rtc::RtcInfo>> {
    state
        .rtc_info
        .map(axum::Json)
        .ok_or_else(|| axum::http::StatusCode::NOT_FOUND.into())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, State(state), headers))
}

/// Decides which tmux server the workspaces live on and, for termenbox's own, writes its config (decision 0120).
/// Runs before any tmux command of the server, because runners made earlier would use the default socket.
async fn choose_tmux_server(args: &Args, mode: &RunMode) -> anyhow::Result<()> {
    use tmuxconf::Socket;
    let explicit = match args.tmux_socket.as_deref().filter(|s| !s.is_empty()) {
        Some(v) => Some(Socket::parse(v).ok_or_else(|| {
            anyhow::anyhow!("TERMENBOX_TMUX_SOCKET={v} is not `termenbox` or `default`")
        })?),
        None => None,
    };
    let saved = tmuxconf::saved(&args.config_dir);
    let choice = tmuxconf::decide(explicit);
    if saved != Some(choice) {
        if let Err(e) = tmuxconf::save(&args.config_dir, choice) {
            tracing::warn!("cannot remember the tmux server choice: {e}");
        }
    }
    if saved == Some(Socket::Default) && choice == Socket::Private {
        tracing::warn!(
            "workspaces still running on the default tmux server stay there and are no longer listed"
        );
    }
    info!(target: "audit", event = "tmux_server", socket = choice.as_str());
    if choice == Socket::Default {
        return Ok(());
    }
    if matches!(mode, RunMode::Host { .. }) {
        // The host's terminfo is not visible from here, so take the entry every host has.
        let conf = tmuxconf::render("xterm-256color", None, None);
        mode.runner().install_host_tmux_conf(&conf).await?;
        backend::use_private_tmux(backend::Isolation::HostUser);
    } else {
        let conf = tmuxconf::write_config(&args.config_dir, terminfo::host_pane_term())?;
        backend::use_private_tmux(backend::Isolation::File(
            conf.to_string_lossy().into_owned(),
        ));
    }
    Ok(())
}

/// `termenbox-server tmux <args>`: runs tmux the way the server does, so `termenbox-server tmux attach` reaches the
/// workspaces from a shell. Replaces this process.
fn run_tmux_passthrough() -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let dir = std::env::var_os("TERMENBOX_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(default_config_dir);
    let explicit = std::env::var("TERMENBOX_TMUX_SOCKET")
        .ok()
        .and_then(|v| tmuxconf::Socket::parse(&v));
    // A server that has not started yet has not chosen: the private one, as a new install would.
    if explicit
        .or_else(|| tmuxconf::saved(&dir))
        .unwrap_or(tmuxconf::Socket::Private)
        == tmuxconf::Socket::Private
    {
        backend::use_private_tmux(backend::Isolation::File(
            dir.join("tmux.conf").to_string_lossy().into_owned(),
        ));
    }
    let args: Vec<String> = std::env::args().skip(2).collect();
    let (program, argv) = backend::Runner::local().wrap(false, "tmux", &args);
    let err = std::process::Command::new(&program).args(&argv).exec();
    anyhow::bail!("cannot run {program}: {err}")
}

/// tmux's prefix key as it writes it (`C-b`); the default when it cannot be read.
async fn runner_prefix(mode: &RunMode) -> String {
    let args = ["show-options", "-gv", "prefix"].map(String::from);
    match mode.runner().run("tmux", &args).await {
        Ok(o) if o.status.success() => {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if p.is_empty() || p == "None" || p.len() > 16 {
                "C-b".into()
            } else {
                p
            }
        }
        _ => "C-b".into(),
    }
}

#[cfg(test)]
mod version_tests {
    use super::{format_version, version, RELEASE_VERSION};

    #[test]
    fn the_build_is_cut_to_seven_characters_after_the_full_semver() {
        assert_eq!(
            format_version("0.1.57-beta.11", "a11fc10deadbeef"),
            "0.1.57-beta.11+a11fc10"
        );
    }

    #[test]
    fn the_running_version_starts_from_the_release_version() {
        assert!(version().starts_with(&format!("{RELEASE_VERSION}+")));
    }
}

#[cfg(test)]
mod name_tests {
    use super::clean_server_name;

    #[test]
    fn the_default_name_is_the_short_hostname() {
        assert_eq!(clean_server_name("vps-1.example.com\n"), "vps-1");
        assert_eq!(clean_server_name("  "), "termenbox-server");
        assert_eq!(clean_server_name("localhost"), "termenbox-server");
        assert_eq!(clean_server_name("a b\u{1b}[31mc"), "ab31mc");
        assert_eq!(clean_server_name(&"x".repeat(80)).len(), 40);
    }
}

#[cfg(test)]
mod udp_port_tests {
    use super::{save_udp_port, saved_udp_port};

    #[test]
    fn the_bound_port_is_remembered_and_a_bad_file_is_ignored() {
        let dir = std::env::temp_dir().join(format!("termenbox-udp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(saved_udp_port(&dir), None, "nothing saved yet");
        save_udp_port(&dir, 4447).unwrap();
        assert_eq!(saved_udp_port(&dir), Some(4447), "the restart sees it");

        std::fs::write(dir.join("udp_port"), "not a port").unwrap();
        assert_eq!(
            saved_udp_port(&dir),
            None,
            "a corrupt file falls back to picking"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod backend_spec_tests {
    use super::{parse_acp, resolve_backends};

    #[test]
    fn auto_adds_acp_unless_it_is_off_and_a_list_is_taken_as_written() {
        assert_eq!(resolve_backends("auto", true).unwrap(), ["tmux", "acp"]);
        assert_eq!(resolve_backends("auto", false).unwrap(), ["tmux"]);
        assert_eq!(resolve_backends("tmux", true).unwrap(), ["tmux"]);
        assert_eq!(
            resolve_backends("command, acp", false).unwrap(),
            ["command", "acp"]
        );
        assert!(resolve_backends("herdr", true).is_err());
    }

    #[test]
    fn terminal_acp_is_on_or_off_and_anything_else_names_the_variable() {
        assert!(parse_acp("on").unwrap());
        assert!(!parse_acp("off").unwrap());
        let e = parse_acp("yes").unwrap_err().to_string();
        assert!(e.contains("TERMENBOX_ACP"), "{e}");
    }
}
