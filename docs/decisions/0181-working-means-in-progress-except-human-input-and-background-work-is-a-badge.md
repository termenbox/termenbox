---
id: 0181-working-means-in-progress-except-human-input-and-background-work-is-a-badge
title: Working means the agent has anything in progress except waiting on the human, and background work after a turn is a badge
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev
amends: [0095]
tags: [agents, server, client]
---

## Decision

- **`working` is evidence of progress, not "a turn is open".** A session is `working` while the agent is doing anything: inference, a tool (a long build or test suite included), a subagent, a compaction, a retry back-off. It is not working while it waits for the human; that is `needs_you`.
- **Sub-reason.** The status names why: `inferring`, `tool(<name>)`, `subagent`, `compacting`, `retrying`. Proof is a `PreToolUse` without its `PostToolUse`, a live child process, a transcript tool call with no result, or fresh output.
- **A finished turn is not working.** When the turn ends (`Stop`), the session becomes `finished` with a `bg: N running` badge if background work lives (a backgrounded Bash, a background subagent, a scheduled wakeup). It never stays working for a dev server nobody will stop. If a background completion wakes the agent, that opens a new turn and the session is `working` again.
- **Finished alerts once.** The finish chimes like any finished turn. Later background completions arrive as silent `info` items ([[0186-info-is-a-silent-attention-tier-and-results-clear-only-when-a-human-views-them]]), so one piece of work does not chime twice.
- **When progress proof lapses** (no hook, transcript change, pane change or child process for a window), the session shows `working` with a `quiet` sub-reason until the process is gone, which closes the turn as interrupted. The window length is **TBD**: it is measured in plan step A.3 of `docs/plans/agent-status-model.md`, and this decision is amended with the number.
- **Open question.** Whether and when Claude wakes on a background completion is **TBD**, measured in plan step A.2.

## Why

The 30-minute pin in the reducer shows `working` for half an hour after a missed `Stop` or an Esc, and the 60-second decay in `derive_activity` drops a long silent tool to idle or unknown (fact 0071). Defining working by progress, with an honest `quiet` state instead of a guess, fixes both. Treating background work as a badge keeps the status truthful about the foreground turn, which is what the person is waiting on.
