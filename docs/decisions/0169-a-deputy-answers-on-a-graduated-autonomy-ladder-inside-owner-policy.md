---
id: 0169-a-deputy-answers-on-a-graduated-autonomy-ladder-inside-owner-policy
title: An optional deputy model answers obligations on a graduated autonomy ladder per decision class, inside owner policy, with measured promotion and demotion
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, deputy, agents, security, automation]
related: [0161, 0165, 0168, 0170, 0171, 0172]
---

## Decision

- **A deputy is a model that triages the obligations that survive contracts and rules.** It reads the obligation, the diff or command, the contract and similar past decisions, then answers, asks the agent a clarifying question, or passes the case on with a recommended answer and a confidence.
- **A decision class** is a stable key (agent, tool kind, workspace, normalised command or change shape). Each class sits on a rung: **propose** (the human sees the recommendation and taps), **answer with undo** (the deputy answers, the case stays visible and revocable for a window), **silent** (answered, logged, shown only in the digest).
- **Promotion and demotion are measured, not chosen by the model.** Propose to answer-with-undo: at least 30 owner-confirmed decisions of the class and 0 overrides in the last 30. Answer-with-undo to silent: at least 200 decisions, at most 0.5% overrides and none in the last 50. Any override demotes one rung and opens a new window. The owner may tighten or loosen thresholds per class.
- **The irreversible floor.** Classes that delete outside the workspace, push or deploy, spend, touch credentials or reach the network never rise above propose, unless a checkpoint makes the action undoable (0172, `irreversible-classes-never-leave-propose-without-a-checkpoint`).
- **Backends are owner-supplied** (`the-deputy-is-optional-capped-and-owner-supplied`): an Anthropic or OpenAI-compatible completion endpoint (hosted or self-hosted), or the unmodified `claude` binary in print mode under the owner's own login on the owner's machine. The Agent SDK is never used for the deputy. The `claude` backend is opt-in and off by default, with a one-line notice that high-volume automated use of a subscription is a grey area (fact 0066) and is rate-limited.
- **A cheap-first cascade**: deterministic class features and similarity to past decisions, then a small model for judgement, then a stronger model for cases bound for a human. A hard token cap applies per owner and month. With the deputy off the system still works (contracts, rules, human).
- **Agent text is data** (`deputy-treats-agent-text-as-data-and-picks-from-enumerated-answers`).

## Why

After contracts and rules the tail is novel judgement calls; at 1,000 agents they are hundreds a day. A ladder gives the leverage of an autonomous deputy and keeps trust: it earns each rung per class from the owner's own answers.

## Not chosen

- Propose-only (the human stays in every loop; ceiling about 10^3), rules only (plateaus), one global autonomy switch (no way to be trusted for reads and not for pushes).
