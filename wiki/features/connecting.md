# How it connects, and home servers

Back to the [README](../../README.md). New to this? [Chapter 2, Connect your phone](../course/02-connect-your-phone.md) walks through it in a few minutes.

```
 Browser (client)  ── WebRTC data channel, direct, end-to-end encrypted ──►  Your VPS
 app.termenbox.dev          one UDP port                               termenbox-server + tmux
        │                                                                          │
        └── GitHub login ── signed directory (address + key, never traffic) ◄──────┘
```

- **Direct connection.** The browser talks to your server over a WebRTC data channel on one UDP port. No tunnel, domain, certificate or relay is needed. The channel is encrypted (DTLS) and the browser pins the server's fingerprint.
- **Persistence = tmux.** The server attaches a tmux client per connection. If your phone drops off the network, the workspace keeps running and reconnecting repaints the screen. Any workspace manager with a CLI can be used instead (see Workspace backends).
- **Owner only.** The first GitHub user to connect becomes the server's owner; everyone else is refused.
- **Free to run.** The client and a small signed directory run on Cloudflare's free tier. Your server needs a VPS with 512 MB of RAM and a public IP.

## Home servers, LAN and pairing links

A server is one identity (its key and certificate fingerprint) that a client reaches over several candidate paths and races: your LAN address, a public IPv4 or IPv6 address, an overlay address (Tailscale, WireGuard) and, later, a tunnel. The server signs the list and sends it to each client after login; the app tries the paths, keeps the first that connects, shows which one it is using (*Direct, LAN*, *Tunnel*) and, after a Wi-Fi to cellular change or a missed heartbeat, races again and re-attaches by itself. When nothing connects it lists every path it tried, why each failed and one fix.

At start the server checks where it sits and prints a short block: whether it has a public address, IPv6 or an overlay, whether the router's WAN address is private or shared (carrier-grade NAT, where no port forward can help), and whether the router opened the UDP port for it by PCP, NAT-PMP or UPnP (renewed every half lease, removed on exit). It says **mapped, not verified from outside**: nothing outside confirms the port is reachable, so a router that accepted the mapping can still sit behind a firewall you do not control.

The installer prints the same pairing block on every run, so a server on your own network is added by scanning it. To use a server on its own network, with no directory or internet:

```bash
TERMENBOX_LAN_ONLY=1 termenbox-server       # no directory announcements, no public-IP lookup, no router mapping
termenbox-server --print-pairing            # a link and a QR code for your phone
```

Open the link or scan the code on a phone on the same network. The app checks the signature, shows the server's name and fingerprint, and connects only when you confirm; compare the fingerprint with the one on the server. An unclaimed server's link also claims it. The link carries the signed list in its fragment, which no host sees. LAN and overlay addresses never go to the public directory. A server behind carrier-grade NAT with no relay can only be reached from its own network or over an overlay: the app says so when it fails.

## Tunnels: reaching a server from anywhere

A tunnel is a property of its server, in your own Cloudflare account. Each server has at most one, and the app never guesses which server you mean.

- **Settings, Tunnels** lists every connected server with a tick box and its state: *No tunnel*, *Temporary address*, *Own tunnel*, *Stopped*, *Offline* or *Too old*. Offline and too-old servers cannot be ticked and say why. A server screen shows the same state as a chip, with a *Set up* or *Manage* link that opens this list with that server ticked.
- **One token, many servers.** Create a Cloudflare API token with the permissions the list shows, tick the servers, paste the token once and choose *Check names*. The app proposes a hostname for each server under one domain and checks it for clashes before anything is created; edit a name if you want. *Apply* sets up the servers one after another. A failure on one server shows its reason and a *Retry*, and does not stop the others.
- **The token is not stored.** It goes to each server over that server's own authenticated connection, is used for that one job and is wiped there. The browser keeps it in memory for the run only: never in storage, an address, a log or an error report. It is gone when the run ends, on *Close* and when the tab closes.
- A **Temporary address** is a Cloudflare quick tunnel that a server starts when nothing else reaches it. Its hostname changes every time the server restarts, so the chip says so. The same list moves it to your own tunnel; an own tunnel can be rotated or given a new hostname there. Linking Cloudflare is a required step for a server reached only this way: its address stops working when it restarts, and every device loses the server until Cloudflare is linked. The app shows a *Link Cloudflare* card for every such server. *Not now* hides it for this app session only; it comes back on the next start and whenever the address changes, and the chip keeps the warning. It never blocks a session. Until the no-domain address record exists, an owner without a Cloudflare domain sees the warning and needs a new link after each restart.
- **Disconnect** is an action on an own or stopped tunnel: with the token it deletes the DNS record, the tunnel and the local credential. A server that is disconnected without a token drops only its local credential, records what it left at Cloudflare, and its row says *Left at Cloudflare: ...* until a later run ticks *Delete what was left* and deletes it with a token.
- **Share links carry the route.** A link shared from the app carries the sender's signed list of paths, as a pairing link does, so a second device learns the tunnel too. It checks the signature, and the first live connection refreshes the list. It also carries a one-time enrolment token that the server needs before it opens a session for the new device, and never a claim token. An unverified route falls back to the directory.

## The assisted way

A server behind a router you cannot change, or inside WSL2's own NAT, cannot take an inbound UDP connection. Termenbox can still connect an *assisted* way, through a free Cloudflare account that you own. Nothing runs on the project's account, and nothing is shared with it.

On the server, once:

```bash
termenbox-server --setup-cloudflare      # reads a Cloudflare API token from stdin, uses it once, never stores it
```

The token needs **Workers Scripts: Edit**, **Account Settings: Read** and **Cloudflare Calls** (create it at dash.cloudflare.com, *My Profile, API Tokens*). The command deploys a small signaling Worker (`termenbox-relay-<hash>`) and a TURN key in your account, keeps only the TURN key's id and its credential-minting token in `relay.json` (mode 0600, next to the server's other state), and prints the Worker's address, the free allowance and the cap.

- **What the connection does.** The phone and the server first try to punch a hole through both NATs with free public STUN (no account needed). If that does not connect within a few seconds, the session goes through your Cloudflare TURN relay. The session header says which: *Punched* or *Relay*.
- **What it costs.** Nothing on the free plan. Cloudflare's Realtime TURN includes 1,000 GB a month, shared with its other Realtime products, and charges $0.05 per GB beyond it. The server counts every relay credential it hands out as 128 MiB, and stops handing them out at a monthly cap of 500 GB (`TERMENBOX_RELAY_CAP_GB` changes it), so a runaway cannot bill you.
- **What it exposes.** Only a client with a device certificate for you gets credentials, and each lasts 15 minutes. The Worker is a switchboard that holds no secret and checks nothing; the server verifies every message.
- **Taking it away.** `termenbox-server --teardown` (token on stdin again) deletes the Worker, its Durable Object and the TURN key, and removes `relay.json`.

Not a LAN-only server: with `TERMENBOX_LAN_ONLY` the server never contacts STUN or the Worker. On WSL2 you can instead set `networkingMode=mirrored` (the installer prints the steps); `--setup-cloudflare` is the alternative when you cannot or would rather not.

## Servers inside a virtual machine

A server inside a NAT'd VM (WSL2 in its default NAT mode, VirtualBox NAT, Docker Desktop) advertises an address that only the VM can use, so other devices on your network cannot reach it. On WSL2 set `networkingMode=mirrored` in `%UserProfile%\.wslconfig`, run `wsl --shutdown` in PowerShell, and allow the UDP port through Windows Firewall; the installer prints these steps when it sees WSL. Otherwise bridge the VM, forward the port, or use an overlay such as Tailscale.
