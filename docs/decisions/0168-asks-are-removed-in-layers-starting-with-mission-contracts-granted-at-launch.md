---
id: 0168-asks-are-removed-in-layers-starting-with-mission-contracts-granted-at-launch
title: Asks are removed in five layers (mission contract, rule, deputy, cluster card, human); a mission contract grants scope, budget and time at launch
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, agents, security, automation, server]
related: [0104, 0143, 0161, 0165, 0169, 0170, 0172]
---

## Decision

- **Layer order for any obligation** (permission, question, plan approval, elicitation): (1) the **mission contract** the work was launched under, (2) owner **rules** (0161), (3) the **deputy** (0169), (4) a **cluster card** (0170), (5) the human. Each layer either answers, or passes down with what it learned. The first layer that can answer, within its authority, does.
- **A mission contract** is granted when work starts, or once for a template or recurring job. It names the workspace(s), allowed paths, commands and tool kinds, network allowance, spend and time limits, the done criteria, and the safe default on timeout (0171). The server enforces it as a generated rule set plus the agent's own modes and sandbox where the agent allows them; it never widens an agent's sandbox.
- **Inside the contract is certain by construction**; an ask that fits it never reaches a person. A breach is an incident (0171), not a silent denial.
- **Contracts are suggested, not invented**: from past approvals of the same kind of work, shown with what they would have covered (the dry run of 0161), and reviewed once per class.
- The matching, scope and audit limits of `auto-answered-permissions-are-scoped-visible-and-audited` apply to contract answers.
- **Delivery order in the plan**: rules (R80) first, then contracts (R89), then the deputy (R91). Contracts and rules are deterministic and need no model.

## Why

Making answers cheaper does not scale; not generating the ask does. Approving a scope once per mission removes the largest class of asks, and makes "confidence" structural for them.

## Not chosen

- Rules and deputy only (the first run of every new task type is noisy and a run has no bounded blast radius), and contracts later (slower to the target).
