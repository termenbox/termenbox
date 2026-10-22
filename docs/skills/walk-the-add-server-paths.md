---
id: walk-the-add-server-paths
title: Walk every Add Server path on real devices before a release
status: active
date: 2026-10-22
tags: [client, mobile, ios, lan, procedural]
---

# Procedure: Walk the Add Server paths

Use before a release that touches the installer, `AddServerModal`, pairing or the candidate race. CI cannot use a camera or the iOS Local Network permission. Label results by platform per `a-phone-check-names-its-platform`.

1. Pick the matrix: devices (real iPhone Safari and installed PWA, real Android Chrome, desktop Chrome) against topologies (a VPS with a public port, a LAN host with internet, a LAN host with the internet cut, a host with its UDP port closed, a host on another Wi-Fi than the phone).
2. For each cell, run the one-liner on the host. Confirm the installer ends with a QR, link and code; run it again and confirm they are printed again.
3. Add the server in the wizard by pasting the command, by scanning the QR inline in Connect, by pasting the link, and by typing the code. From a second device, add it with the global Scan QR.
4. At each step, confirm the checklist names the stage and each address's result, and that the scan button and camera are visible without scrolling.
5. Force each failure (expired link, wrong Wi-Fi, denied Local Network permission, stopped server, closed UDP port, wrong code 21 times). Confirm every error fits one screen, with one action and details folded, and that a working WebSocket fallback shows as a notice.
6. Record the number of taps and seconds from "run command" to a working terminal for each cell.
7. Report screenshots of each failure state in the PR.
