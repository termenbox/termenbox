---
id: irreversible-classes-never-leave-propose-without-a-checkpoint
title: A decision class that is irreversible never rises above propose unless a checkpoint makes the action undoable
status: active
date: 2026-11-01
governed-by: [0173, 0176]
grounded-in: [0173, 0176, 0180]
derivation-note: Measured confidence says how often the deputy was right, not what a wrong answer costs; for actions that cannot be undone the cost decides.
tags: [security, deputy, agents, control-plane]
---

# Guardrail: A decision class that is irreversible never rises above propose unless a checkpoint makes the action undoable

## Rule
**MUST** keep these classes at **propose**: delete outside the workspace, push or deploy, spend, credential access, network egress, and any class the owner marks irreversible. **MAY** raise one only for an action covered by a live checkpoint (git worktree or snapshot) that the server has verified restorable, and only to answer-with-undo. **MUST** keep the same floor for contract and rule answers (`auto-answered-permissions-are-scoped-visible-and-audited`). Promotion counts never override this floor.

## Enforcement
- Server test: a class with the maximum record stays at propose without a checkpoint, and drops to propose when the checkpoint is gone.
