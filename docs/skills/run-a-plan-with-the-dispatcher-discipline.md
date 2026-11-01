---
id: run-a-plan-with-the-dispatcher-discipline
title: Run a plan with the dispatcher discipline: pre-flight, outcome checks, decomposition, routing, combining and a run log
status: active
date: 2026-11-01
tags: [control-plane, process, agents, procedural]
---

# Procedure: Run a plan with the dispatcher discipline

Use to implement `docs/plans/attention-control-plane.md` (or any plan in the step format) by hand with the rules that decision 0179 gives the product, until the dispatcher itself (R99, R100) exists. The plan format is already a dispatcher input: `Check` is the outcome spec, `Needs` the decomposition, `Class`, `Effort` and `Tier` the routing, `‖` the combining hint. Decisions 0020 to 0024 hold the model-routing rules; `max-concurrent-agents-rule` and `parallel-tasks-use-worktrees` bound parallel work.

1. **Pre-flight a slice before its first step.** Read every step of the slice and list the owner decisions and reviews it will raise (named review steps, wording to confirm, thresholds, anything marked "owner"). Ask them once, together, and write each answer into the step's Context. Do not start a step that will stop on a question you could have asked now.
2. **Check every outcome spec.** A step's `Check` must fail before the work and pass after. If a step has no such command, add a small step that produces the verifier first (a test, a script, a file check). A step whose only verifier is the owner (a dogfood note, a video review) is routed on the owner's default tier and is never shortened.
3. **Decompose to one pull request.** A step that predicts more than one PR (S of 13, or two modules plus UI plus wire) is split into steps that each have their own `Check` and `Needs`. Record the prediction.
4. **Route by class and evidence.** Start `explore` and `review` on the lowest tier that has passed that kind of `Check` in the run log, `implement` and `debug` one tier up; escalate one tier when the `Check` fails, at most twice, then hand over to the owner (decision 0022). Never route below the default for a step without a command `Check`.
5. **Combine.** Steps that touch the same module and context run in one session and one PR when the plan allows it (for example 1.1 with 1.4, which share `attention_meter.rs`). Steps marked `‖` with no shared files run in separate pool worktrees, at most 3 agents at once.
6. **Keep the run log.** After each step append a row to `docs/plans/assets/control-plane/run-log.md`: step id, tier used, tokens or cost if the host reports it, owner questions asked, `Check` result, predicted versus actual size. This is the data R97 and R99 start from; the dogfood note closes the step, the log row feeds the next prediction.
7. **Hand over by the contract.** Each delegated step gets a brief with goal, scope and `Done-when`, and returns the result then the evidence (decision 0031). Reviews are read-only (`reviewer-read-only`).
