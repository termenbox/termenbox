# Configuration

Back to the [README](../../README.md). To run your own client, directory and OAuth app, see [BOOTSTRAP.md](../BOOTSTRAP.md); servers then need `TERMENBOX_CLIENT_URL=https://your-client`, which the install command of your own client carries.

Server (environment variables of the container; the installer sets the first two):

| Variable | Description |
|---|---|
| `TERMENBOX_UDP_PORT` | UDP port of the WebRTC transport (default: the first free port in 4443-4542, remembered across restarts) |
| `TERMENBOX_CLIENT_URL` | Client and directory origin (default https://app.termenbox.dev) |
| `TERMENBOX_PUBLIC_IP` | Address clients connect to; detected when unset |
| `TERMENBOX_LAN_ONLY` | `1`: reach the server over the LAN or an overlay only (no directory, no public-IP lookup, no router mapping); pair with `--print-pairing` |
| `TERMENBOX_LAN_ADDRS` | LAN addresses for the signed list, comma-separated, instead of the ones found on the interfaces (at most 4 are used) |
| `TERMENBOX_OVERLAY_ADDRS` | Overlay addresses (Tailscale, WireGuard) for the signed list, instead of the ones found |
| `TERMENBOX_WEBRTC` | `off` for WebSocket only |
| `TERMENBOX_SERVER_NAME` | Display name; default: the host's short hostname (the installer passes it into the container) |
| `TERMENBOX_BACKEND` | `auto` (default: `tmux`, plus `acp` unless `TERMENBOX_ACP=off`), `tmux`, `command`, `acp`, or a comma list such as `tmux,acp`, taken as written |
| `TERMENBOX_ACP` | `on` (default) offers the built-in agent-view (ACP) agents `claude` and `opencode` once their adapter is installed; `off` removes the `acp` backend from `auto` |
| `TERMENBOX_CONFIG_DIR` | Owner, keys and certificate (default `~/.config/termenbox`) |
| `TERMENBOX_HISTORY_DAYS` | Days of agent history kept for the Activity view (default 90) |
| `TERMENBOX_HISTORY_MAX_MB` | Most disk the history may use, oldest dropped first (default 20) |

Client build: `VITE_GITHUB_CLIENT_ID`; `TERMENBOX_CLIENT_ORIGIN` (`https://your-client`) is written into the installer served at `/i` as its default client address, so a self-hosted client shows the same bare install command. The OAuth code exchange runs in a Pages Function that needs `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and `CERT_SIGNING_KEY` (base64 PKCS#8 Ed25519) as secrets on the Pages project. Servers trust its public key from `packages/server/cert-keys.txt`; self-hosters set `TERMENBOX_CERT_KEYS=kid=<base64url key>` on their servers.
