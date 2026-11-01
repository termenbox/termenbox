---
id: control-plane-steps-are-dogfooded-before-they-are-done
title: A control-plane step is not done until the owner has run it on real work and a dogfood note records the attention-leverage reading
status: active
date: 2026-11-01
governed-by: 0174
grounded-in: [0174, 0165]
derivation-note: The product is judged by the owner's own attention, so a slice that only passed its tests has not yet shown it helps.
tags: [process, dogfood, control-plane]
---

# Guardrail: A control-plane step is not done until the owner has run it on real work and a dogfood note records the attention-leverage reading

## Rule
**MUST** mark a step of `docs/plans/attention-control-plane.md` `[x]` only when its Check passes and `docs/plans/assets/control-plane/dogfood.md` has a dated note for it from the owner (days used, the meter reading, what hurt). **MUST NOT** deploy to or modify the owner's own `termenbox.service`; the owner installs. A slice with risk to the owner's work **MUST** ship behind a switch that is off by default.

## Enforcement
- Plan review: a step marked done without a dogfood note is reopened.
