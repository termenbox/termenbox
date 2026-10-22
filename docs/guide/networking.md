# How it connects, and home servers

Back to the [README](../../README.md).

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

## Home servers, LAN and pairing links

A server is one identity (its key and certificate fingerprint) that a client reaches over several candidate paths and races: your LAN address, a public IPv4 or IPv6 address, an overlay address (Tailscale, WireGuard) and, later, a tunnel (decision 0123). The server signs the list and sends it to each client after login; the app tries the paths, keeps the first that connects, shows which one it is using (*Direct, LAN*, *Tunnel*) and, after a Wi-Fi to cellular change or a missed heartbeat, races again and re-attaches by itself. When nothing connects it lists every path it tried, why each failed and one fix.

At start the server checks where it sits and prints a short block: whether it has a public address, IPv6 or an overlay, whether the router's WAN address is private or shared (carrier-grade NAT, where no port forward can help), and whether the router opened the UDP port for it by PCP, NAT-PMP or UPnP (renewed every half lease, removed on exit). It says **mapped, not verified from outside**: nothing outside confirms the port is reachable, so a router that accepted the mapping can still sit behind a firewall you do not control.

The installer prints the same pairing block on every run, so a server on your own network is added by scanning it (decisions 0126, 0127). To use a server on its own network, with no directory or internet:

```bash
TERMENBOX_LAN_ONLY=1 termenbox-server       # no directory announcements, no public-IP lookup, no router mapping
termenbox-server --print-pairing            # a link and a QR code for your phone
```

Open the link or scan the code on a phone on the same network. The app checks the signature, shows the server's name and fingerprint, and connects only when you confirm; compare the fingerprint with the one on the server. An unclaimed server's link also claims it. The link carries the signed list in its fragment, which no host sees. LAN and overlay addresses never go to the public directory. A server behind carrier-grade NAT with no relay can only be reached from its own network or over an overlay: the app says so when it fails.

## Servers inside a virtual machine

A server inside a NAT'd VM (WSL2 in its default NAT mode, VirtualBox NAT, Docker Desktop) advertises an address that only the VM can use, so other devices on your network cannot reach it. On WSL2 set `networkingMode=mirrored` in `%UserProfile%\.wslconfig`, run `wsl --shutdown` in PowerShell, and allow the UDP port through Windows Firewall; the installer prints these steps when it sees WSL. Otherwise bridge the VM, forward the port, or use an overlay such as Tailscale. Details and what has not been verified: `docs/facts/0053-a-server-in-a-nat-vm-needs-port-forwarding-to-be-reached-from-other-devices.md`.
