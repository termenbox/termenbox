---
id: add-a-decision-class-to-the-deputy
title: Add a decision class to the deputy and move it up or down the ladder
status: active
date: 2026-11-01
tags: [control-plane, deputy, agents, server, procedural]
---

# Procedure: Add a decision class to the deputy and move it up or down the ladder

Use when a new kind of obligation needs its own class, or a class needs promoting or demoting (decision 0173; guardrails `irreversible-classes-never-leave-propose-without-a-checkpoint`, `deputy-treats-agent-text-as-data-and-picks-from-enumerated-answers`).

1. **Name the class key** from stable features only: agent, tool kind, workspace, a normalised command or change shape. No agent text, no paths with user names. Add a test showing two equivalent obligations share a key and two different ones do not.
2. **Mark reversibility.** If the action is delete-outside-workspace, push or deploy, spend, credentials or network egress, set the class irreversible: it stays at propose without a verified checkpoint.
3. **Start at propose.** A new class always starts there; never seed a higher rung.
4. **Record outcomes.** Each owner answer to a proposal is a confirmation or an override of the recommended option; store counts per class in the deputy's state.
5. **Promote by the thresholds of 0173**, evaluated by the server, never by the model. Check the irreversible floor first.
6. **Demote on any override** by one rung, open a new window, and show the demotion as an incident (0175).
7. **Verify** with the server test that replays a recorded history and asserts the rung after each step; run it before shipping a threshold change.
