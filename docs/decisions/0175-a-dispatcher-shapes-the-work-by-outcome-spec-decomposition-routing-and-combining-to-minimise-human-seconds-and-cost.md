---
id: 0175-a-dispatcher-shapes-the-work-by-outcome-spec-decomposition-routing-and-combining-to-minimise-human-seconds-and-cost
title: A dispatcher shapes the work: outcome specs, decomposition, decision pre-flight, verified-quality routing and task combining, to minimise human seconds, cost and unverified risk together
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, agents, automation, performance, fleet]
related: [0165, 0168, 0169, 0170, 0172]
---

## Decision

- **A dispatcher sits above the five ask-removal layers (0168).** It decides how a mission is cut, where each task runs and on which model tier, because token cost, decision-point count and quality are coupled to those choices.
- **Outcome spec.** Every mission carries a machine-checkable definition of done (tests, a diff bound, a reviewer check), extending the contract's done criteria. A task without a verifier is never routed to a cheaper tier than the owner's default.
- **Decomposition.** A mission is split into tasks, each with a predicted cost vector: tokens, quality risk, expected decision points, reversibility. The split is a proposal; the owner sees it for new kinds of work and it is learned per class after.
- **Decision pre-flight.** Decisions the tasks will predictably raise are asked once, before launch, as one card, and the answers become contract clauses (0168). This is the plan-time counterpart of runtime clustering (0170).
- **Routing.** A task starts on the cheapest agent, model tier and node whose recorded history passes its verifier, and escalates a tier when verification fails. Escalation count and promotion follow the measured-ladder logic of 0169, evaluated by the server.
- **Combining.** Tasks that share context, a contract or a decision class run on one node and session, for cache reuse and one set of asks.
- **Objective.** Minimise `w1 * human_seconds + w2 * cost + w3 * (1 - verified_quality)` inside the contract. The owner sets the weights; the meter (R85, R97) supplies the terms.
- **Measure before steering.** Telemetry first (R97), then recommendations in shadow mode, and only then action, each dogfooded (0174).

## Why

Making asks cheaper (0168 to 0172) does not choose the cheapest way to get the work done. The same mission cut differently raises the same question 30 times or once, and runs on a model that is ten times dearer than needed. Treating human seconds, cost and quality as one objective is what makes 10^3 and 10^4 affordable as well as possible.

## Not chosen

- Leaving routing to each agent or to the owner's habit (no joint optimisation), and building the router before collecting data (a guess tuned on nothing).
