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

- **Reserved range.** The server binds its WebRTC UDP port itself, taking the first free port from 4443 to 4542 (or the remembered one), and saves the choice. The installer no longer picks the port, so a missing `ss` and the pick-then-bind race stop mattering. The machine's admin opens `4443-4542/udp` once; later user installs need no root for the firewall. An explicit `TERMENBOX_UDP_PORT` still wins.
- **Say it first.** Before downloading or changing anything, the installer's summary states what will need root and whether it is already done: a firewall (`ufw` or `firewalld`) is active (detected without root through `systemctl is-active`, since the rules need root to read), and whether lingering is on (`loginctl show-user`), trying `enable-linger` early because it is reversible and journaled. It prints the exact commands for the admin, and still never runs `sudo` (0113).
- **Test from outside in the app.** The last step of Add Server makes the real WebRTC attempt from the browser, which is the only outside vantage point that costs nothing (no probe host, `no-paid-services`). A failure shows the port and the firewall command for the admin, and says when the user's own network blocks UDP.
- **No TCP fallback for this.** A non-root user cannot open a TCP port either, and the hosted client is HTTPS, so a plain `ws://` to an address is refused as mixed content. The tunnel fallback stays as 0057 describes.

## Why

The installer's firewall step is the only recurring root need of a user install, and it recurs because each instance picks a random port. A fixed range turns it into one rule per machine and lets the server, which owns the socket, decide the port.
