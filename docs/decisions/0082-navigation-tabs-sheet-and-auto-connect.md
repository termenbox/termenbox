---
id: 0082-navigation-tabs-sheet-and-auto-connect
title: Navigation is a tab strip and a session sheet over servers that connect by themselves
status: active
date: 2026-10-13
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, mobile, agents]
---

## Decision

Supersedes 0070 (never built: the app still holds one connection and shows one tab). Kept from 0070: one `Connection` per server, soft limit of 6 that warns and never drops, background heartbeats only while the page is visible, `AppShell` owns the viewport, killing a session is `close_session` and always confirmed, wide screens keep a docked sidebar.

- **Servers connect by themselves.** Every saved server connects at app start; there is no connect step. A server can be Paused (Servers, in the menu). An unreachable or reconnecting server shows on its group header and its tabs, greyed, with the reason and Retry; it never takes over the screen. The old server list is only the empty state for adding the first server.
- **Top bar on a phone:** ☰ menu, a horizontally scrolling tab strip, a Sessions button with the count of sessions that need you. The menu holds Servers, Settings (cache TTL, theme), Report a problem, Install/Update, the account with the GitHub avatar, Log out.
- **Tabs** are one per session across all servers, mirroring the servers exactly, no close button, with a server badge when more than one server is saved. Each carries a status icon and an accessible state word. Tabs of sessions that need you move to the front while the strip is untouched. Nothing ever switches the active tab by itself.
- **Sessions sheet:** a bottom sheet on phones (peek, half, full) replaces the left drawer; cards are those of plan v8.
- **Views.** Presets: Needs me (needs input and errors, longest waiting first), Working, Cache expiring (idle agents, soonest to lose their cache first), Recent (last agent response), By server. Free controls: group by server, agent, status or none; sort; filter. It opens on Needs me when anything needs you, otherwise By server; the choice is remembered per device (browser storage; it joins synced settings if 0068 is accepted).
- **Proactive signals:** a non-modal needs-you banner (name, what it asked, Open, and the answer buttons when hook-confirmed), a count in the tab title and the installed app's badge with a dot on each tab, the cache-expiring nudge of 0083, and per-server degraded notices.
- **Session actions:** long-press a tab or card for Rename, Kill (confirmed), Copy name; New session is a + in the sheet (templates stay in plan v2 A.3).
- **Streaming lists are append-only.** A list that fills while the user reads it (the search sheet) does not sort or regroup as results arrive: a group sits where its first hit arrived and a hit goes to the end of its group, so a row never moves under a finger. This replaces a hold on the whole list, which would stop results appearing while the user scrolls. Sorting happens only when the user starts a new query or changes the grouping.
- **Data:** the server reports each session's agent (claude, codex, herdr, shell) from the process scan and a `last_response_at` (last turn-complete or working-to-idle change), so grouping and sorting work without hooks.

## Why

The owner works across several machines and agents and needs to see, in one glance, what waits for them, without a connect step or a screen change to find out. 0070 decided most of this and nothing built it, so this decision keeps it and adds the views and proactive behaviour that make the list the working surface. See [[0061-agent-state-and-push]], [[0083-cache-expiry-estimate-and-statusline-forwarder]], [[0084-tab-switch-shows-a-server-snapshot-first]], [[connections-warn-never-drop]], [[ui-never-moves-under-the-finger]].
