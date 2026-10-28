---
id: 0070-session-navigation-and-connections
title: Sessions of all servers are navigated in one place over simultaneous connections
status: superseded
superseded-by: 0082-navigation-tabs-sheet-and-auto-connect
date: 2026-10-12
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, mobile, protocol]
---

## Decision

- The client keeps one `Connection` per server, all connected at once; the sidebar groups sessions by server and every tab carries a server badge.
- The top bar has a sidebar toggle and one tab per session (active one highlighted), mirroring the servers' session lists one-to-one. Tabs have no close button.
- Killing a session is the close button on a sidebar row or the long-press / right-click menu entry, and always asks for confirmation. The menu has only that entry for now.
- The sidebar has three states on wide screens (hidden, icon rail, full 280 px, remembered). On a phone it is hidden or an overlay of about 70% width that closes on a tap outside; there is no phone rail.
- The viewport is owned by one `AppShell` (dynamic viewport height, `visualViewport` resize and scroll, safe-area insets) with a `?debug` overlay that prints the numbers; the owner verifies on an iPhone.
- Soft limit of 6 connected servers: past it the client shows a notice with the count and a Manage connections action at app start and when a server is added or connected. It never disconnects a server by itself; each server has a connect/disconnect toggle. Background connections ping only while the page is visible.
- The server gains `close_session` (kill) and the protocol carries it; `SessionBackend` gains `kill`.

## Why

Managing several machines means jumping between their agents; one connection at a time makes that a server switch plus a wait. All-connected gives instant switching at the cost of N channels and heartbeats on a phone, so the owner chose a visible soft limit over silent drops. Killing ends a running agent, so it is deliberate and confirmed; tabs mirror the server so the two views cannot disagree. See [[0069-ui-foundation-tailwind-shadcn]], [[soft-keyboard-never-hides-input]], [[kill-session-needs-confirmation]], [[connections-warn-never-drop]].
