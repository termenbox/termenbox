---
id: errors-fit-one-screen
title: An error state fits one phone screen with one cause and one action
status: active
governed-by: 0127-add-server-is-a-four-step-wizard-with-a-standalone-scan
grounded-in: [ux-acceptance-targets, connections-warn-never-drop]
derivation-note: Given 0127 (the wizard names the cause and one fix) and a closed-UDP-port error seen spanning several screens on an iPhone, an error must not need scrolling.
tags: [client, mobile, ux, guardrail]
---

# Guardrail: Errors Fit One Screen

## Rule

1. Any error state on any Add Server step **MUST** fit the viewport of a 375x667 phone without scrolling: one cause, one primary action.
2. Detail (firewall commands, per-address results, raw messages) **MUST** sit behind a fold that is closed by default.
3. A working fallback (for example WebSocket when UDP fails) **MUST** be shown as a notice, not as an error.

## Tests Required

| Test | Description |
|---|---|
| `error_states_fit_375x667` | A browser e2e renders each failure state (expired link, server not found, UDP closed with and without fallback, permission denied) at 375x667 and asserts the document does not scroll. |
| `details_are_folded` | Each of those states has its detail element closed on first render. |
