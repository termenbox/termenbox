---
id: ui-features-ship-templates-tests-and-media-before-wiring
title: An Agent view slice is wired to the real stream only after its templates are tested and styled and its video and manual exist
status: active
date: 2026-11-01
governed-by: 0163
grounded-in: [0163, 0135]
derivation-note: Wiring before the look, the tests and the documentation are settled makes every review a rework of built code.
tags: [client, ux, process]
---

# Guardrail: An Agent view slice is wired to the real stream only after its templates are tested and styled and its video and manual exist

## Rule
**MUST** build each slice as templates on fixtures first, cover them with tests, style them, and render the slice's video and generated manual before its wiring step starts. **MUST** run the same scenario against live data when wiring, and attach the new video. **MUST** keep the committed guide equal to what the scenarios generate (the drift check). **MUST NOT** hand-write a guide section the scenarios could generate, or label a rendered walkthrough as a recording of a real device or agent.
