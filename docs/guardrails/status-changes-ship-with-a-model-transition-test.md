---
id: status-changes-ship-with-a-model-transition-test
title: A change to the status machine, an evidence adapter or a status rendering ships with a transition test and an edge-case scenario
status: active
governed-by: 0187-agent-status-is-one-per-session-machine-and-the-workspace-is-a-roll-up
grounded-in: [0187-agent-status-is-one-per-session-machine-and-the-workspace-is-a-roll-up, 0189-evidence-ranks-hook-transcript-screen-scan, 0074]
derivation-note: Given that status bugs recur because each fix patched one path without a test of the whole machine (fact 0074), every change to it has to prove the transition table still holds.
tags: [agents, server, client, testing]
---

# Guardrail: Status changes ship with a transition test

## Rule
A change to the status machine, to an adapter that feeds it (hook, transcript, screen, scan) or to how a surface renders status **MUST** add or update a row in the table-driven transition test, and **MUST** name the edge-case scenario of the plan's catalogue that covers it. A new hook, transcript field or screen marker **MUST** come with a recorded fixture from a real run, not a hand-written one.

## Check
The transition test lists every transition of decisions 0187 to 0193; a CI step fails when a status enum value, sub-reason, failure cause or event kind has no row. A changed adapter fails its fixture test when its fixture directory has no file for a handled event. Reviewers use the procedure `add-a-status-transition`.
