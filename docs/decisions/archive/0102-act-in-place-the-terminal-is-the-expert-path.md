---
id: 0102-act-in-place-the-terminal-is-the-expert-path
title: Users act in place; the full-screen terminal is the expert path and the fallback
status: superseded
superseded-by: 0109-state-first-and-the-terminal-is-first-class
date: 2026-10-16
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, mobile, agents, roadmap]
---

## Decision

- Choosing an option, typing a reply, dictating (R53) and sending a follow-up prompt all work from a decision card or the session view, without opening the terminal.
- The full terminal stays one obvious tap away on every session, for experts and for the cases where the structured view cannot help. Reaching it is never the only way to complete a common task; when a user has to drop to the terminal to answer, that is treated as a product gap to close.
- Free-text replies go through a composer that supports the system keyboard's dictation; no audio leaves the device to our servers (`no-paid-services`, `security-secrets-handling`).

## Why

The terminal is powerful and a poor fit for a phone; a decision-first product (0100) fails if every answer needs a drill-down. Keeping the terminal one tap away preserves what experts value. Content for the in-place view is in [[0103-session-view-uses-capability-tiers-and-agent-profiles]].
