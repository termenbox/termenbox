# Termenbox

A web terminal for managing remote AI coding agents (Codex, Claude Code) from any phone or desktop browser. Sessions live in tmux on your own server and survive dropped connections. No native apps, no subscriptions, no relay in the middle.

## How it works

```
 Browser (client)  ── WebRTC data channel, direct, end-to-end encrypted ──►  Your VPS
 app.termenbox.dev          one UDP port                               termenbox-server + tmux
        │                                                                          │
        └── GitHub login ── signed directory (address + key, never traffic) ◄──────┘
```

- **Direct connection.** The browser talks to your server over a WebRTC data channel on one UDP port. No tunnel, domain, certificate or relay is needed. The channel is encrypted (DTLS) and the browser pins the server's fingerprint.
- **Persistence = tmux.** The server attaches a tmux client per connection. If your phone drops off the network, the session keeps running and reconnecting repaints the screen. Any session manager with a CLI can be used instead (see Session backends).
- **Owner only.** The first GitHub user to connect becomes the server's owner; everyone else is refused.
- **Free to run.** The client and a small signed directory run on Cloudflare's free tier. Your server needs a VPS with 512 MB of RAM and a public IP.

## Quick start

You need a Linux VPS with a public IP (512 MB RAM, 1 vCPU is enough) and root access.

1. Open **https://app.termenbox.dev** and log in with GitHub.
2. Choose **Add Server**. Copy the command it shows and run it on your VPS.
3. Wait a minute. The server appears in the list. Tap it.

That is the whole setup. The command picks a free UDP port, starts the server and hands it a one-time secret. It is safe to run again; running it with a new secret re-claims the server for a new owner. Add Server also asks **where sessions should run** (see below).

**Prefer to scan?** On the VPS run `curl -fsSL https://app.termenbox.dev/install.sh | sh -s -- - https://app.termenbox.dev`. It shows a QR code; in Add Server choose **Scan a QR** and use the camera, or take a screenshot of the code and pick it from the gallery (delete the screenshot afterwards).

### Firewalls

The server needs **one inbound UDP port** (the installer picks a free one, usually 4443, and opens it in `ufw` or `firewalld` if they are active). If you use a provider firewall or security group (AWS, GCP, Oracle, Hetzner Cloud Firewall), open that port there. The app tells you which port when it cannot reach your server: *"Server unreachable on UDP port N"*. To see the port: `cat /opt/termenbox/udp_port` (isolated container) or `sudo cat /var/lib/termenbox-root/udp_port` (on the machine).

Some mobile and corporate networks block UDP. Those clients cannot connect until the tunnel fallback is available (see the roadmap).

### Where your agents run

Add Server offers two places (decisions 0058, 0075 and 0078):

**In an isolated container** (the default). Sessions run inside a Docker container, in `/opt/termenbox/home` (mounted as the container's home, so it survives restarts and upgrades). Docker is installed if missing. The container cannot see the rest of your VPS. The image has tmux, git, curl and Node. Install your agents in a Termenbox session, for example:

```bash
npm install -g @anthropic-ai/claude-code @openai/codex
```

**On the machine itself** (no Docker). The server is a single static binary run as a systemd service, and sessions run directly on your VPS, where your repositories and agent tools already are. You choose who the sessions run as: a dedicated unprivileged user `termenbox` (created for you, recommended), the user who runs the install command (sessions open on your behalf, in your home, with your repositories, keys and agent logins; nothing is created), or `root`. This gives your browser a shell on the server itself, so the app shows a warning and asks you to confirm. Requirements: systemd, `curl`, `ssh-keygen` (package `openssh-client`), x86_64 or aarch64. A pinned static `tmux` (3.2 or newer, decision 0119) is installed next to the server, so none has to be installed first; `TERMENBOX_TMUX` points the server at a tmux of your own instead.

**As your own user, with no root** (decision 0113). The same machine-itself install, but as a user service of the account that runs the command: no `sudo`, no new user. The binary lives in `~/.local/bin`, state in `~/.config/termenbox`, and the update check is a timer of your own. Starting at boot needs lingering, which systemd lets you switch on for yourself (`loginctl enable-linger`); if your system refuses, the installer says so and prints the one command root would have to run. A firewall rule is printed the same way, never run. The pinned static `tmux` goes to `~/.local/bin` beside the server, so none has to be installed. The dedicated user and root choices still use a system service and ask for `sudo`.

The installer also sets up a daily update check that runs as root, separately from the server (which can stay an unprivileged user). Before anything is installed, and before every update, the release is checked: its manifest must carry a valid signature by the project's release key, and the binary must match the signed SHA-256 (see `SECURITY.md`). It lists every change it will make, records each one in a journal in `/var/lib/termenbox-root` before making it, and can undo them.

### Updating and removing

Remove the server from the machine with one command. Without `--yes` it only shows what it would do:

```bash
curl -fsSL https://app.termenbox.dev/uninstall.sh | sudo sh            # preview, changes nothing
curl -fsSL https://app.termenbox.dev/uninstall.sh | sudo sh -s -- --yes
```

An install made for your own user needs no `sudo` to remove: run the same command as that user, without `sudo`.

It handles both install kinds. For the container it removes the container and `/opt/termenbox`. On the machine itself it stops the service and the updater, deletes the server's state and undoes every journaled change (binary, units, firewall rule, and the `termenbox` user and tmux only if the installer added them). It never touches your repositories, or the home of the user your sessions run as. Add `--keep-state` to keep the server's keys and owner, `--delete-home` to also delete the home of a `termenbox` user the installer created. Running tmux sessions are left alone. Then use **Remove server** in the app to drop it from your list; you can also uninstall from there while connected.

Updates: the container updates when you run the install command again. On the machine itself a timer (root-owned, or your own for the no-root install) checks for a new signed release every day; if the new server does not come up healthy the old one is put back. Check now: `sudo termenbox-server --root-helper update`.

## Sessions, tabs and windows

- **Tabs.** Each running session is a tab at the top, showing the folder and the agent of the window it speaks for; sessions from every saved server share one strip. Several sessions in the same folder are told apart by name. Tap an inactive tab to open it (its last screen shows at once, then the live one), tap the active tab for its windows, long-press for actions.
- **Windows.** The dropdown of the active tab lists the tmux windows of the session: switch to one, close it (a running program or the last window asks first), or open a new one with **+ New window** in the folder of the current window.
- **Detach or close.** Detach hides the tab and keeps the session running; Close session asks first and ends it.
- **Needs you.** When an agent waits for an answer, a banner names the session and offers its choices without changing the tab you are on.
- **Servers.** The Servers screen lists the saved servers; Add Server shows the install command for a container or for host mode and waits for the server to appear.

These behaviours are tested in a real browser (`packages/client/e2e/`), including two real server processes with real tmux. The browser runs emulate a phone; the soft keyboard, safe areas and gestures on a real iPhone are checked by hand with `docs/skills/mobile-ui-device-check.md`.

## Is it safe to use?

What a stranger on the internet can and cannot do, and what you accept by installing:

- **Only you can open a session.** The first GitHub account to connect owns the server; everyone else is refused. Your browser holds a device key that cannot be exported, and the server checks a certificate for it on every connection.
- **Your terminal traffic never touches our servers.** It goes from your browser to your machine, encrypted. The free directory only maps your server's public key to its address.
- **Your GitHub token is not kept.** It is used once to sign the certificate and dropped.
- **Host install = a shell on your machine, for your browser.** Whoever controls your GitHub account and an enrolled browser can run commands as the user the sessions run as. That is the product; choose the narrowest option you can live with: the isolated container, then a dedicated `termenbox` user, then your own user, then root. Turn on two-factor authentication on GitHub.
- **Nothing is installed silently.** The installer lists every change first, records each in a journal before making it, and can undo all of them. Binaries are checked against a signature by the release key before they run, on install and on every update; a signed freeze can stop updates.
- **Nothing else runs as root.** The server runs as an unprivileged user; root work (updates, removal) is done by a separate small helper that accepts only a fixed list of requests.
- **What we cannot protect against:** a compromised machine, a stolen unlocked phone with an enrolled browser (remove the device in the app), or a malicious agent you run in a session.

Full model and how to report a problem: [SECURITY.md](SECURITY.md).

## Hosting your own client

By default everything uses the hosted client at app.termenbox.dev. To run your own (your domain, your OAuth app, your directory) follow [docs/BOOTSTRAP.md](docs/BOOTSTRAP.md). Servers then need `TERMENBOX_CLIENT_URL=https://your-client` (the install command carries it).

## Session backends

The default (`TERMENBOX_BACKEND=auto`) uses tmux. To use another tool with a CLI, give three commands (no shell; `{name}` and `{id}` are replaced by a validated session name):

```
TERMENBOX_BACKEND=command
TERMENBOX_BACKEND_NAME=zellij
TERMENBOX_BACKEND_LIST=zellij list-sessions --short
TERMENBOX_BACKEND_CREATE=zellij attach --create-background {name}
TERMENBOX_BACKEND_ATTACH=zellij attach {id}
```

The attach command runs in a PTY. It must repaint the session when it starts and only detach when it is killed; that is how a dropped connection resumes. The zellij lines are an illustration and untested. Otherwise implement the `SessionBackend` trait (`packages/server/src/backend.rs`).

### Agent sessions over ACP (T0)

An agent that speaks the Agent Client Protocol can run as a session the server starts itself, with a view the app draws (Markdown replies, tool calls, diffs, the plan, permission questions you answer with one tap) instead of a terminal. The server starts only the commands you list, in `acp-agents.toml` in the config directory (a missing file means none):

```
max_sessions = 2            # agents at once; each is a Node or Rust process of a few hundred MB
[agents.claude]
command = "claude-agent-acp"
env_remove = ["CLAUDECODE"]
```

Run `TERMENBOX_BACKEND=tmux,acp` to offer both: New session then asks for the manager ("Agent session (ACP)") and the directory. Keep `tmux` in the list: the **Shell** key in an agent session opens a plain tmux session in the same directory (an ACP agent has no terminal of its own). See `docs/skills/probe-an-agent-over-acp.md` for adding an agent.

## Notifications

When a Claude Code session asks for you, finishes or fails, the server raises an item the app shows in the banner, the tab title, the app badge and the Inbox (Settings → Appearance, notifications and more → Inbox). To also get a notification on a phone that is not looking at the app, set up Web Push once:

```
termenbox-server --setup-push     # writes the key to connectors.env (mode 0600) next to the server's other state
```

Restart the server, then Settings → Appearance, notifications and more → Turn on notifications (on iPhone the app must be added to the Home Screen first, iOS 16.4+). A notification is sent about 5 seconds after the item appears, unless a device is showing that session. It names the session and what happened and never carries anything the agent wrote. A phone follows one server at a time.

## Agent events

The server turns what agents do into a small typed stream (idle, working, needs input, done) that clients can follow. With no setup it comes from a process scan and screen heuristics (low confidence). For exact tool and "needs input" events from Claude Code, opt in:

```
termenbox-server --connect-claude          # shows the hook lines it would add
termenbox-server --connect-claude --yes    # applies them (backup kept)
termenbox-server --disconnect-claude       # removes exactly those again
```

Only sessions created by Termenbox carry the variables hooks need. Agent text in events is cleaned (control characters, secrets, addresses) and capped before it is stored or sent. Details: `docs/plans/archive/termenbox-v6-agent-events.md`.

## Configuration

Server (environment variables of the container; the installer sets the first two):

| Variable | Description |
|---|---|
| `TERMENBOX_UDP_PORT` | UDP port of the WebRTC transport (default: the first free port in 4443-4542, remembered across restarts) |
| `TERMENBOX_CLIENT_URL` | Client and directory origin (default https://app.termenbox.dev) |
| `TERMENBOX_PUBLIC_IP` | Address clients connect to; detected when unset |
| `TERMENBOX_WEBRTC` | `off` for WebSocket only |
| `TERMENBOX_SERVER_NAME` | Display name; default: the host's short hostname (the installer passes it into the container) |
| `TERMENBOX_BACKEND` | `tmux` (default), `command`, `acp`, or a comma list such as `tmux,acp` |
| `TERMENBOX_CONFIG_DIR` | Owner, keys and certificate (default `~/.config/termenbox`) |
| `TERMENBOX_HISTORY_DAYS` | Days of agent history kept for the Activity view (default 90) |
| `TERMENBOX_HISTORY_MAX_MB` | Most disk the history may use, oldest dropped first (default 20) |

Client build: `VITE_GITHUB_CLIENT_ID`. The OAuth code exchange runs in a Pages Function that needs `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and `CERT_SIGNING_KEY` (base64 PKCS#8 Ed25519) as secrets on the Pages project. Servers trust its public key from `packages/server/cert-keys.txt`; self-hosters set `TERMENBOX_CERT_KEYS=kid=<base64url key>` on their servers.

## Development

```bash
cargo test --workspace                 # needs tmux for the end-to-end tests
cd packages/client && npx pnpm@9 install && ./node_modules/.bin/vitest run
# browser end-to-end (needs a built server, `pnpm build`, `pnpm e2e:build` and Chrome)
node e2e/run.mjs && node e2e/touch.mjs && node e2e/cards.mjs && node e2e/windows.mjs && node e2e/two-servers.mjs
```

No compiler on the machine? `scripts/dev.sh` runs any command in a pinned build container (Rust, tmux, and with
`--full` Node, pnpm and Chromium). The repo is mounted; cargo's registry and `target/`, and pnpm's store, live in a
named Docker volume, so repeat builds are incremental (`docker compose -f docker/docker-compose.dev.yml down -v`
clears them):

```bash
scripts/dev.sh cargo test --workspace
scripts/dev.sh --full sh -c 'pnpm install --frozen-lockfile && pnpm --filter termenbox-client e2e:build && cd packages/client && node e2e/run.mjs'
```

All builds and image publishing run in CI (`.github/workflows/`). Layout:

```
packages/core     shared protocol types (Rust)
packages/server   termenbox-server: auth, sessions, WebRTC + WebSocket transports, directory announcer
packages/client   Svelte 5 + xterm.js client, Pages Functions (OAuth exchange, signed directory)
packages/agent    parked (agent detection moves into the server, issue #19)
docker/           image (built by CI), dev build container, sample compose file
docs/             facts, decisions, guardrails, plans
```

The plan and its status live in GitHub issues; see `docs/plans/termenbox-roadmap.md`.

## Roadmap

Mobile UX, the installable PWA, agent notifications, end-to-end tests and resource gates are done. What comes next is ordered in `docs/plans/termenbox-roadmap.md`.

## License

MIT
