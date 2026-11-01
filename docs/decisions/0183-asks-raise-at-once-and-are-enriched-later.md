---
id: 0183-asks-raise-at-once-and-are-enriched-later
title: An ask is raised at the first evidence with a typed kind, its text and options are filled in when they arrive, and the UI never shows an empty ask
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev
amends: [0095]
tags: [agents, notifications, client, server]
---

## Decision

- **Raise at once.** The first evidence of a wait (`PermissionRequest`, a transcript pending tool call, a blocked screen) opens a `needs_you` item with a typed kind: `permission`, `question`, `plan`, `trust` or `elicitation`. The status, the sound and the push do not wait for the text.
- **Enrich later, by tab.** The question text and options come from, in order: the structured tool input (`AskUserQuestion`, `ExitPlanMode`, a permission's tool and arguments), the transcript, the `Notification` message (up to six seconds after the request, fact 0048), and last the screen parser. Content attaches to the open ask of the same tab and key, never to "the first open ask in the workspace".
- **Never empty.** Until content arrives the card shows a typed fallback ("Claude asks you a question", "Claude wants to run a tool") and an "open terminal" action. A late `Notification` fills the card in place without a second alert.
- **Options reach the answer path only from structure.** Choices offered as one-tap answers come from structured input or a high-confidence parse, within the limits of the banner today (1 to 3 choices). Screen guesses give text only.
- **Push stays content-free** (guardrail `notifications-carry-no-agent-text`); the text appears only inside an authenticated client.

## Why

Asks arrive without questions today because the text rides on the late `Notification`, is matched to any open ask in the workspace, and never reads `tool_input` for `AskUserQuestion` or `ExitPlanMode`; `TabPrompt` is never filled for Claude (fact 0071). Holding the ask until its text is known would delay status and sound, which is the opposite bug. Raising early and enriching in place gives a correct status immediately and a complete card as soon as the facts exist.
