---
id: 0061-agent-state-and-push
title: Agent state comes from process scans and tool adapters; notifications are installed-PWA Web Push
status: superseded
superseded-by: 0095-agent-status-is-activity-plus-attention
date: 2026-10-12
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, notifications, pwa, roadmap]
---

## Decision

- The server scans each pane's process tree for claude, codex and herdr and reports one of idle, working, needs input, done, error. Per-tool adapters add precision (herdr's status API, Claude/Codex hooks when configured); a documented prompt heuristic is best effort.
- Notifications use Web Push (VAPID, no paid service) from the installed PWA. The payload carries no terminal text, only "session X needs you". Fallback: in-app inbox, tab badge, toasts.
- Detection runs in the server (0054) and is audit-logged without agent output.

## Why

Hooks alone are silent for unconfigured tools and output-idle heuristics misfire, so the scan is the floor and adapters raise accuracy. iOS delivers push only to installed PWAs (16.4+), which is why the PWA ships first.

> Note (2026-10-15): Superseded in part by 0095 (state model, notifications) once the owner accepts it; 0079 and 0083 stand.
