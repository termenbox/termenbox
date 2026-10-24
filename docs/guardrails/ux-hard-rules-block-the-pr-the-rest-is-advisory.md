---
id: ux-hard-rules-block-the-pr-the-rest-is-advisory
title: The UX job fails a pull request on objective rule breaks only; trends, diffs and heuristic findings are advisory
status: active
governed-by: 0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set
grounded-in: [0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set, ux-acceptance-targets, a-phone-check-names-its-platform]
derivation-note: A check that fails on a measurement must be stable and objective, or contributors learn to ignore it; the acceptance targets are the objective part, the rest informs a human.
tags: [client, mobile, ux, ci]
---

# Guardrail: Hard UX rules block, the rest is advisory

## Rule
The UX job **MUST** fail on each of these in any scenario, engine or profile:
1. A tappable control under 44 x 44 pt, except the dense terminal key row, whose height stays at least 44 pt (DR1 and its exception).
2. Horizontal page overflow (DR15).
3. Text contrast under 4.5:1, or 3:1 for 22 px and larger (DR8).
4. A tap-count budget broken: push to sent answer at most 2 taps, any session from Home in 1 tap, every agent's state on Home with 0 taps (`ux-acceptance-targets`).
5. The emulated soft keyboard covers the focused input or the key row (`soft-keyboard-never-hides-input`), or an element moves under the finger (`ui-never-moves-under-the-finger`).
6. A console error or uncaught exception.
7. First usable paint over 1.5 s on the throttled 4G profile, as the median of at least three runs.

The job **MUST NOT** fail on metric trends, screenshot differences, focus-order or discoverability findings, or an agent's heuristic findings; it reports them in the artifact and the pull request comment. A rule is promoted from advisory to blocking only after three runs on `develop` agree. Every result **MUST** be labelled emulated.
