---
id: 0172-the-route-to-10-4-is-reversible-by-default-learning-from-answers-hierarchy-with-sampled-audit-and-an-attention-budget
title: The route to 10^4 is four levers, all planned: reversible by default, learning from answers, hierarchy with sampled audit, and an attention budget
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, agents, automation, fleet, product]
related: [0165, 0168, 0169, 0170, 0171]
---

## Decision

At 10^4 a person has about 0.12 s per agent-hour (fact 0067), so the human supervises error budgets and policies, not agents. Four levers, each cutting a different term, are planned after the 10^3 layers (0168 to 0171):

1. **Reversible by default (R92).** Each agent works behind a checkpoint (git worktree or snapshot). A blocking "may I?" becomes act, report, undo if rejected, so asks stop blocking and review moves off the critical path. A checkpoint is also what lets an irreversible class rise above propose.
2. **Learn from answers (R93).** A human answer and its reason become a standing instruction or a contract clause, pushed to the agents' context. The recurrence rate of an ask is measured and shown.
3. **Hierarchy with sampled audit (R95).** Lead agents run sub-fleets of about 20 and send only exceptions and digests upward, so the human supervises 30 to 50 leads. Outcomes are checked by a sample sized for a confidence bound, not one by one; defects found tighten the class.
4. **Attention budget (R94).** The owner sets a budget, for example 30 minutes a day and at most 4 interrupts an hour. The system steers contracts and autonomy to stay inside it; a breach is an incident (0171).

Machine verification (tests, CI, a reviewer agent) is the evidence behind confidence in 1 and 3; confidence never rests on a model's self-assessment alone.

## Why

Layers 0168 to 0171 reach about 10^3 by removing, batching and delegating asks. Beyond that the remaining cost is blocking and per-agent review, which these four remove.

## Not chosen

- Recording them as direction only. The owner chose to plan all four.
