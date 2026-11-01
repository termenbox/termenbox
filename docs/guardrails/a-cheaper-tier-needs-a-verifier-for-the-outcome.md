---
id: a-cheaper-tier-needs-a-verifier-for-the-outcome
title: A task is routed to a cheaper model tier than the owner's default only when a machine verifier for its outcome exists and the tier's recorded history passes it
status: active
date: 2026-11-01
governed-by: 0179
grounded-in: [0179, 0173, 0176]
derivation-note: Cost savings from a weaker model are only safe when a wrong result is caught by something other than the human or the same model.
tags: [agents, control-plane, automation, security]
---

# Guardrail: A task is routed to a cheaper model tier than the owner's default only when a machine verifier for its outcome exists and the tier's recorded history passes it

## Rule
**MUST** keep a task on the owner's default tier when its outcome spec has no machine verifier (tests, a diff bound, a reviewer check by a different model). **MUST** escalate one tier when verification fails, and record the failure against the tier and class. **MUST NOT** let a model's own confidence stand in for a verifier. **MUST** keep routing in shadow mode (recommend, never act) until the owner has confirmed it for the class.

## Enforcement
- Server test: a task with no verifier is never routed down; a failed verification escalates once and is counted.
