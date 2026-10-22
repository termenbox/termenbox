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
3. Add the server in the wizard by scanning the QR inline in Connect, by pasting the link, and (once the mailbox is built, R65 steps 2.5 and 3.1) by typing the code. From a second device, add it with the global Scan QR.
4. At each step, confirm the checklist names the stage and each address's result, and that the scan button and camera are visible without scrolling.
5. Force each failure (expired link, wrong Wi-Fi, denied Local Network permission, stopped server, closed UDP port, wrong code 21 times). Confirm every error fits one screen, with one action and details folded, and that a working WebSocket fallback shows as a notice.
6. Record the number of taps and seconds from "run command" to a working terminal for each cell.
7. Report screenshots of each failure state in the PR.

## Connectivity spike (R62 step 1, R65 step 1.1)

Run once on a real iPhone (Safari and the installed PWA) and a real Android phone, against a server on the same Wi-Fi started from the one-liner. Write each result into FACT-0052, labelled by platform.

1. From the hosted HTTPS client, open a WebRTC connection and a WebSocket to the server's private address. Note whether the Local Network prompt appears, what the page sees when it is allowed, denied, or never asked.
2. Tap and scan the `https://<client>/#/pair/…` link: does it open Safari or the PWA, and does the fragment survive? Compare with a `termenbox://` link.
3. Cut the router's internet (unplug the WAN) and add the server from the printed QR.
4. Time the pairing-code stretch (Argon2id or scrypt in JS) on the slowest phone and set the parameter to about 100 to 300 ms.
5. On iOS 16.4, check `crypto.subtle.verify` with Ed25519 on a bundle; if it throws, `lib/bundle.ts` needs the noble fallback.
6. On a Windows machine with WSL2 (optional): does the WebSocket fallback reach the server from another device after a `netsh interface portproxy` rule, and which versions support `networkingMode=mirrored` (FACT-0053).
