---
id: 0127-add-server-is-a-four-step-wizard-with-a-standalone-scan
title: Add Server is a four-step wizard with no fork up front, and Scan QR is a separate path outside it
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [onboarding, client, mobile, ux]
date: 2026-10-22
---

## Decision

- **Steps:** 1 Install (one command, one Copy button; the run-mode choice is folded under "Options"), 2 Connect, 3 Confirm, 4 Done. One step is on screen at a time.
- **Connect** is a live checklist: code read, looking for the server (each address shown as trying, failed or won, with a reason), found and reading what it is. The same step offers "Server shows a QR? Scan it" with the camera opening in place at the top, a paste field for the link, and a field for the short code.
- **Confirm** is the 0117 preview and the name field. **Done** shows the active path ("Direct, LAN") and a plain-language fix when degraded.
- There is no "code or anonymous" choice. The user runs the same command whatever the topology; the QR is a way to finish Connect. **There is one add command**, `curl -fsSL <client>/i | sh`, with one optional mode word under Options (`container`, `host-user`, `host`; the default is this machine as the current user). It carries no secret and no client address: the host mints the claim, and a client build writes its own address into the `/i` it serves. The embedded-secret command and the `sh -s -- -` QR form are gone, not a shortcut or a second path; 0126 earlier called the embedded-secret command a shortcut, and this decision does not keep it.
- **Scan QR** is a button on the server list and the header, outside the wizard. It opens the camera full-screen and goes straight to Confirm, for adding a server from a second device.
- **Errors:** a failure on any step or device fits one phone screen without scrolling: one cause, one primary action, details folded. A working fallback (WebSocket instead of UDP) is a notice, not an error (guardrail `errors-fit-one-screen`).
- Failures name the likely cause: different Wi-Fi or a guest network that isolates devices, iOS Local Network permission denied, server stopped, link expired, UDP port closed.

## Why

The dialog put name, run-mode form, instructions, command, camera and scan buttons in one scroll, so on a phone the scan button was below the fold and the camera opened further down. After a scan the only feedback was "this can take a minute". A user with a closed UDP port on an iPhone saw an error spanning several screens.

Not chosen: a first step that asks "with code or anonymous" (the user cannot judge the difference before they have run anything); a step 0 asking where the server is (it would only change copy).

## Consequences

- Replaces the tabs in `AddServerModal.svelte`; `PortHelp` becomes a folded detail.
- `scanCommand` and the secret in `installCommand` are removed; Connect has nothing to wait for until a link, scan or code arrives, and says so ("Nothing yet? Scan or paste the link the installer printed").
- Plan: R65.
