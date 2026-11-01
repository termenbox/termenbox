---
id: write-a-mission-contract-template
title: Write a mission contract template for a kind of work
status: active
date: 2026-11-01
tags: [control-plane, agents, security, automation, procedural]
---

# Procedure: Write a mission contract template for a kind of work

Use when a kind of work repeats (a template or a recurring job) and its asks should stop reaching a person (decision 0172).

1. **List what the work touches**: workspaces, path globs, commands, tool kinds, network hosts. Start from the dry run of the last 50 asks of this kind (0180), not from guesses.
2. **Write the scope narrowly**: one workspace, relative paths, parsed-argument command patterns (no compound forms), a host list instead of "network".
3. **Set limits**: spend, wall time, number of files changed, and the done criteria a machine can check (tests pass, diff within bounds).
4. **Choose the safe default** on timeout: park, deny or a named safe branch (0175), and the urgency class.
5. **Check the irreversible floor.** Anything irreversible is excluded from the contract or requires a checkpoint (`irreversible-classes-never-leave-propose-without-a-checkpoint`).
6. **Preview** the generated rule set and what it would have answered; the owner confirms once.
7. **Run it dogfood-first** (`dogfood-a-control-plane-slice`) and review the breaches after a week; tighten or widen with the evidence.
