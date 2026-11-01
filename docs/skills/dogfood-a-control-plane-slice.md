---
id: dogfood-a-control-plane-slice
title: Dogfood a control-plane slice on the owner's own fleet and record the reading
status: active
date: 2026-11-01
tags: [control-plane, dogfood, process, procedural]
---

# Procedure: Dogfood a control-plane slice on the owner's own fleet and record the reading

Use as the last gate of every step of `docs/plans/attention-control-plane.md` (decision 0178; guardrail `control-plane-steps-are-dogfooded-before-they-are-done`).

1. **Ship behind a switch** that is off by default when the slice can disturb the owner's work. Say in the PR how to switch it off in one step.
2. **Hand over, do not deploy.** Agents and CI never touch the owner's `termenbox.service`. Give the owner the release or branch build and the install command.
3. **Take a before reading** of attention leverage (R87) on the owner's server for the days the step names, with the slice off.
4. **Run on real work** for those days with the slice on.
5. **Write the dogfood note** in `docs/plans/assets/control-plane/dogfood.md`: date, step, days used, before and after readings, the three things that hurt, and whether to keep, change or revert.
6. **Feed back.** A pain becomes a step or edits one before the next slice is wired. Only then mark the step `[x]`.
