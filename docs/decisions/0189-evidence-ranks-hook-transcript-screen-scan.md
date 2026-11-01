---
id: 0189-evidence-ranks-hook-transcript-screen-scan
title: Status evidence ranks hook, then transcript, then screen, then scan, and the transcript is a reconciler and the source of ask text
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev
amends: [0095, 0157-agent-session-files-are-read-through-a-session-log-port-with-one-adapter-per-agent]
tags: [agents, server, protocol]
---

## Decision

- **Order of authority:** hook, then transcript (the agent's own session file, read through the session-log port), then screen markers, then the process scan. A higher source wins a conflict; a lower one may only fill a gap or close what a higher one left open.
- **The transcript is promoted from search-only to a status source.** The session-log adapter for Claude exposes turn start and end, tool calls with and without results, pending asks, and error stops as typed observations. It closes what hooks miss (Esc on a dialog or a running turn sends no hook, fact 0048) and supplies the text of `AskUserQuestion` and `ExitPlanMode` when the hook does not.
- **Every source emits the same typed events** (turn start/end, tool start/end, ask open/content/close, subagent and background start/end, compact, session start/end, process gone) to the machine of [[0187-agent-status-is-one-per-session-machine-and-the-workspace-is-a-roll-up]]. A source never sets a status directly.
- **Reconcile covers every agent pane,** not one chosen pane per workspace. A close from the screen or transcript needs two disagreeing observations in a row and an old enough last event; the two constants live in one place.
- **A scan or screen hint never raises Attention.** Only hook, transcript and ACP events can.
- **The lag between the UI and the log is measured, not assumed** (plan step A.1). The result is a fact, and the grace values are set from it.
- **Agents without hooks or an adapter** (Codex today) show what their weaker sources prove and nothing more; an unproven state is `unknown`, drawn as nothing.

## Why

Hooks are fast but incomplete, the screen is complete but brittle, and the transcript is durable but lags. Ranking them, and letting each only do what it is good at, stops the screen from overwriting a hook's finished state (it does today) and stops a missed hook from leaving a wrong status for half an hour. The transcript format is private to Claude Code, so the adapter sits behind the port of decision 0157 and a tier-3 smoke run guards against drift.
