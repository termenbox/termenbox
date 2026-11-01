---
id: 0169-termenbox-is-an-attention-control-plane-and-its-metric-is-attention-leverage
title: Termenbox is an attention control plane for one person running 100 to 1,000 agents; its metric is attention leverage, in seconds of human per agent-hour
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [product, control-plane, agents, fleet, mobile]
amends: [0051]
related: [0095, 0100, 0122, 0180, 0168, 0170, 0172, 0173, 0174, 0176]
---

## Decision

- **The product is the human's attention layer over a fleet of agents**, reachable from any device, that lets one person keep a large number of agents productive. The web terminal and its Mosh-level persistence stay as the substrate and the drill-down; they are no longer the headline.
- **The metric is attention leverage**: seconds of human attention spent per agent-hour of productive work. Targets, for one person at 8 h a day (fact 0070): 10^2 agents = about 12 s, 10^3 = about 1.2 s, 10^4 = about 0.12 s per agent-hour. Concurrent agents and output multiples are derived from it, not goals of their own.
- **The server measures it** from card dwell time, answer latency and the number of obligations that reached a human versus those removed by each layer (0172). The measurement is local, metadata only, and needs no telemetry (R87).
- **Every feature is judged by what it does to this number.** A feature that adds a human step per agent is wrong at scale even if it is pleasant at five agents.
- **Stays true**: web-first, self-hosted, one owner per server (0053), no paid service in the required stack, the web client complete (`native-wrapper-keeps-web-parity`).

## Why

Home already answers "what needs me" (0100), but the design unit is still one agent in one tab, so human effort grows linearly with agents. Naming one scarce resource, attention, and one number gives every later decision (contracts, deputy, clusters, escalation) the same yardstick, and it is the only framing under which 10^3 and 10^4 make sense: below about a second per agent-hour a person can decide on classes and error budgets, not on instances.

## Not chosen

- "Concurrent agents per human" as the headline (gameable by idle agents), and "output multiple" (depends on agent quality, which termenbox does not control).

## Consequences

- `CLAUDE.md` Project Overview is reworded. Roadmap steps R87 to R98 and `docs/plans/attention-control-plane.md` carry the work. Decision 0177 pauses the Agent view programme for it.
