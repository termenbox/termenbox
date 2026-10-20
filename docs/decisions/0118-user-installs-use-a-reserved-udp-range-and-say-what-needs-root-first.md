---
id: 0118-user-installs-use-a-reserved-udp-range-and-say-what-needs-root-first
title: User installs bind a port from a reserved UDP range and say what needs root before changing anything
status: active
date: 2026-10-17
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [deployment, network, security, client]
---

## Decision

Amends 0113 and supports 0115.

- **Reserved range.** The server binds its WebRTC UDP port itself, taking the first free port from 4443 to 4542 on its first start, and saves the choice (amended below: the saved port is never swapped). The installer no longer picks the port, so a missing `ss` and the pick-then-bind race stop mattering. The machine's admin opens `4443-4542/udp` once; later user installs need no root for the firewall. An explicit `TERMENBOX_UDP_PORT` still wins.
- **Say it first.** Before downloading or changing anything, the installer's summary states what will need root and whether it is already done: a firewall (`ufw` or `firewalld`) is active (detected without root through `systemctl is-active`, since the rules need root to read), and whether lingering is on (`loginctl show-user`), trying `enable-linger` early because it is reversible and journaled. It prints the exact commands for the admin, and still never runs `sudo` (0113).
- **Test from outside in the app.** The last step of Add Server makes the real WebRTC attempt from the browser, which is the only outside vantage point that costs nothing (no probe host, `no-paid-services`). A failure shows the port and the firewall command for the admin, and says when the user's own network blocks UDP.
- **No TCP fallback for this.** A non-root user cannot open a TCP port either, and the hosted client is HTTPS, so a plain `ws://` to an address is refused as mixed content. The tunnel fallback stays as 0057 describes.

## Why

The installer's firewall step is the only recurring root need of a user install, and it recurs because each instance picks a random port. A fixed range turns it into one rule per machine and lets the server, which owns the socket, decide the port.

## Amendment (proposed 2026-10-20, awaiting acceptance)

Found when a second Linux user's install showed an empty Add Server form: a pinned port outside the range was never flagged, a re-run of the installer printed a claim link for a service that was crash-looping, and the app's error named no command.

- **The port is the owner's, and it stays.** Only a first start scans the range. Every later start binds the saved (or explicitly set) port or exits with a message that names the port and `TERMENBOX_UDP_PORT`; it never moves to another port, because the firewall rule and the directory record are for that one.
- **Manual port.** `TERMENBOX_UDP_PORT=<port>` in front of `sh` on the install line (unattended), or a prompt on the terminal when the range has no free port; the installer stops with that instruction when there is no terminal.
- **The installer checks the service on every path**, including "already installed", before it prints a claim link or QR, and prints the server's own error when the service is down.
- **The installer says the port and how to open it**: the host firewall (ufw, firewalld) for that port, and a best-effort note for the hosting provider's network firewall, detected from the link-local metadata address (AWS, Google Cloud, Azure, DigitalOcean, Hetzner, Oracle, Vultr; nothing for other hosts). A port pinned outside the range is reported as such, not changed.
- **The app shows the same help** with the server's port when a preview, a reachability check or a server card finds it unreachable.
- **Push is not used for this**: a server that cannot bind its port cannot send a push.
