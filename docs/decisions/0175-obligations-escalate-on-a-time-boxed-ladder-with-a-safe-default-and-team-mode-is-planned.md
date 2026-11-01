---
id: 0175-obligations-escalate-on-a-time-boxed-ladder-with-a-safe-default-and-team-mode-is-planned
title: Obligations escalate on a time-boxed ladder with a safe default for one owner; team mode (quorum, zones of responsibility) is planned, not built
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, agents, security, mobile, notifications]
related: [0053, 0062, 0095, 0169, 0171, 0172, 0173]
---

## Decision

- **Every obligation carries an urgency class and a time-box** with a **safe default**: park the agent, deny, or take the safest branch named by the contract. The class and the default come from the contract (0172), else from the class's history.
- **Escalation ladder for the owner**: a card in the stream; a push when no device shows it (0095, `notifications-carry-no-agent-text`); a louder push and any second device; then the safe default is applied and an **incident** entry is written. A fleet is never blocked on one sleeping person.
- **Incidents are first-class**: a breach of a contract, a stuck agent past its threshold (0168), a deputy demotion, a time-out default and a node that went silent. They appear in the same stream, are deduplicated by cause and cluster like asks, and each shows what the system did on its own.
- **Single owner now (0053).** Actor id and owner id are separate in every record, audit entry and wire message (`operator-actions-carry-an-actor-id-apart-from-the-owner`).
- **Team mode is planned** (R98): **zones of responsibility** (an actor handles named workspaces, classes or nodes, with off-hours cover), and **quorum** (a class marked high-risk needs N approvers, with distinct actors). It adds roles, per-actor device keys and per-actor audit, and must not change the single-owner path.

## Why

Escalation that waits for one human does not fit a fleet that runs while they sleep. A safe default bounds the cost of silence; separating actor from owner now avoids a rewrite later.

## Not chosen

- Owner plus delegates now (widens the security surface before the single-owner path is proven).
