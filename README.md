<h1 align="center">Termenbox</h1>

<p align="center">
  <b>Run Claude Code and Codex on your own server, from your phone.</b><br>
  A self-hosted web terminal for AI coding agents. Workspaces live in tmux and survive dropped connections. No app required, no subscription.
</p>

<p align="center">
  <a href="https://github.com/termenbox/termenbox/actions/workflows/ci.yaml"><img alt="CI" src="https://github.com/termenbox/termenbox/actions/workflows/ci.yaml/badge.svg"></a>
  <a href="https://github.com/termenbox/termenbox/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/termenbox/termenbox"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/github/license/termenbox/termenbox"></a>
</p>

## Install

On the Linux machine that will run your agents (a VPS or a home server; x86_64 or aarch64, with systemd):

```bash
curl -fsSL https://app.termenbox.dev/i | sh
```

Then open **[app.termenbox.dev](https://app.termenbox.dev)** on your phone or laptop, log in with GitHub, tap **Add Server** and scan the QR the command printed (or paste its link). That is the whole setup. The command carries no secret and is safe to run again.

> [!TIP]
> No camera? Paste the link the command printed into Add Server instead.

## Pick your path

| You have | Run on the machine | Then |
|---|---|---|
| **A VPS with a public IP** | `curl -fsSL https://app.termenbox.dev/i \| sh` | Scan the QR in Add Server. If it cannot connect, open the one UDP port the installer names. |
| **A home server or laptop on your Wi-Fi** | the same command | Scan the QR from a phone on the same Wi-Fi. No public IP, domain or router setting needed. |
| **Workspaces isolated in Docker** | `curl -fsSL https://app.termenbox.dev/i \| sh -s -- container` | Scan the QR. Docker is installed if missing. |
| **A dedicated user or root service** | `… \| sh -s -- host-user` or `… \| sh -s -- host` | Scan the QR; asks for `sudo`. |
| **Your own client and domain** | the command your client shows | Follow [BOOTSTRAP.md](docs/BOOTSTRAP.md). |
| **To remove it** | `curl -fsSL https://app.termenbox.dev/uninstall.sh \| sudo sh` | Shows what it would do first; add `-s -- --yes` to apply. |

> [!IMPORTANT]
> The default install runs workspaces directly on the machine as **your own user**, so the app gets a shell with your repositories, keys and agent logins. Choose the container path if that is more access than you want.

> [!NOTE]
> **Windows:** run it inside WSL2 with `networkingMode=mirrored`, otherwise other devices cannot reach it ([details](docs/guide/networking.md#servers-inside-a-virtual-machine)).

## Why Termenbox

- **Built for the phone.** Tabs per workspace, a key row for Esc, Ctrl and arrows, and a banner when an agent is waiting for you.
- **Survives bad networks.** The workspace runs in tmux on your server. Close the tab, lose signal or switch from Wi-Fi to cellular and it is still there; reconnecting re-attaches and repaints.
- **Direct and private.** The browser talks to your server over an encrypted WebRTC data channel on one UDP port. There is no relay in the middle, and our free directory only maps your server's key to its address.
- **Works with your agents.** Claude Code, Codex, OpenCode, or any program in a terminal. Notifications and agent status come from hooks you switch on yourself.
- **Free and yours.** MIT licensed. The hosted client and directory run on Cloudflare's free tier, and the server needs 512 MB of RAM and one vCPU.

```
 Browser (app.termenbox.dev)  ── WebRTC, direct, encrypted ──►  Your machine
                                      one UDP port             termenbox-server + tmux
```

## Questions

<details><summary><b>Can I use Claude Code or Codex from my phone?</b></summary>

Yes. Install Termenbox on the machine where the agent runs, then use the app as the terminal: start the agent in a workspace, answer its questions from the banner, and come back later to the same workspace. Supported browsers: iOS Safari 16.4+, Chrome on Android 111+, current Chrome, Firefox, Edge and Safari on desktop.
</details>

<details><summary><b>Is this like Mosh or tmux over SSH?</b></summary>

The persistence is tmux's: the server keeps the workspace and each connection attaches to it. What Termenbox adds is a browser client made for touch, a connection that re-races its paths after a network change, and notifications. It is not an SSH client and does not need one.
</details>

<details><summary><b>Does my terminal traffic go through your servers?</b></summary>

No. It goes from your browser to your machine, encrypted, and the browser pins the server's fingerprint. Only a signed record (the server's public key and address) goes through the directory, and LAN addresses never do. Details: [security](docs/guide/safety.md) and [SECURITY.md](SECURITY.md).
</details>

<details><summary><b>Does it need a public IP or port forwarding?</b></summary>

On the same network as your phone, no: scan the QR and it connects over the LAN. On iPhone this depends on Safari's Local Network permission, which we have not finished testing; the link and QR work wherever the phone can reach the machine. Away from home you need one reachable UDP port, or an overlay such as Tailscale. A server behind carrier-grade NAT can only be reached from its own network or over an overlay until a relay exists. The app lists every path it tried and what to fix. See [networking](docs/guide/networking.md).
</details>

<details><summary><b>What does it cost?</b></summary>

Nothing beyond the machine you run it on. There is no subscription, and using Termenbox never requires installing an app — the browser is the product.
</details>

<details><summary><b>Who can open a workspace?</b></summary>

Only the GitHub account that claims the server first, from a browser you have enrolled. Everyone else is refused.
</details>

## Documentation

| | |
|---|---|
| [Install, firewall, run modes, update and remove](docs/guide/install.md) | [Workspaces, agents and notifications](docs/guide/workspaces.md) |
| [How it connects, home servers, WSL](docs/guide/networking.md) | [Configuration](docs/guide/configuration.md) |
| [Security model](docs/guide/safety.md) and [reporting a problem](SECURITY.md) | [Hosting your own client](docs/BOOTSTRAP.md) |
| [Development and tests](docs/guide/development.md) | [Roadmap](docs/plans/termenbox-roadmap.md) |

## License

MIT
