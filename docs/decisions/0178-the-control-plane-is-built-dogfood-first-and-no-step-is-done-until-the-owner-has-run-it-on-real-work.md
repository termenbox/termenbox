---
id: 0178-the-control-plane-is-built-dogfood-first-and-no-step-is-done-until-the-owner-has-run-it-on-real-work
title: The control plane is built dogfood-first: a thin end-to-end slice runs on the owner's own fleet early, and no step is done until it has run on real work with a measured reading
status: active
date: 2026-11-01
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, dogfood, roadmap, process]
related: [0167, 0169, 0177]
---

## Decision

- **Thin vertical slice first.** The first milestone (M1) is the smallest path that the owner uses every day on their own machines: the attention-leverage meter on the owner's server, then a conductor with two or three of the owner's own nodes and the existing Needs-you list across them. Breadth (contracts, deputy, levers) comes after M1 has been used.
- **A step is done only when** its checks pass **and** the owner has run it on real work for the days the step names, with the attention-leverage reading (R87) recorded in the step's dogfood note. A step that cannot be dogfooded says why and is split.
- **The owner installs each slice.** Agents and CI never deploy to or touch the owner's own `termenbox.service` (it is hands off); a slice ships as a release or a branch build the owner chooses to install. Where a slice has a risk to the owner's work, it ships behind a switch that is off by default and one tap to disable.
- **Dogfood findings feed the plan.** A pain found in use is recorded as a dogfood note, and becomes a step or changes one before the next slice is wired.
- **The meter is the evidence.** Before/after readings of seconds of human per agent-hour, from the owner's own fleet, are what show a slice helped.
- The template-first rule (0167) still governs the UI; dogfood is the last gate, after the review, not a replacement for it.

## Why

A control plane for attention can only be judged by someone whose attention it is spending. Real use finds the wrong cluster key, the card that is too long and the escalation that wakes the owner for nothing faster than any fixture.
