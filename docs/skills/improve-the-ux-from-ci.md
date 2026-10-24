---
id: improve-the-ux-from-ci
title: Analyse the mobile and desktop UX from the CI report, change it, and prove the change helped
status: active
date: 2026-10-24
tags: [client, mobile, ux, ci, procedural]
---

# Procedure: Improve the UX from the CI report

For an agent (Claude Code or another) or the owner. Inputs: the `ux-report` artifact of a UX job run (`gh run download <run> -n ux-report`), `docs/plans/assets/ux/RUBRIC.md`, `INVENTORY.md` and `DESIGN-PRINCIPLES.md` in the same folder. The job only measures (decision 0135); you analyse and change.

1. **Read the report**: `report.json` (per scenario, engine and profile: taps, time, target sizes, overflow, contrast, focus order, first paint), the screenshots, and the base-vs-head diff. Note every hard-rule failure first (`ux-hard-rules-block-the-pr-the-rest-is-advisory`).
2. **Score against the rubric**: for each path in `INVENTORY.md` record taps, menu depth, discoverability, shortcut coverage and conflicts (desktop), key-row and keyboard behaviour, and focus order. Open the screenshots; a number can hide a layout that reads badly.
3. **Pick findings**, at most three per pass, each tied to a rubric line and a number it should move (for example "Home to the sessions sheet: 3 taps, target 1"). Drop a finding you cannot tie to a measurement or a design rule; list it as advisory instead.
4. **Check the decisions** a change touches (0070, 0071, 0082, 0100-0110) and `docs/plans/assets/ux/DIRECTION.md`. If a finding contradicts one, stop and ask the owner; do not rewrite a decision to fit a change.
5. **Change on its own branch** off `develop` (`working-a-change`), one finding per pull request. Edit the scenario first when the path itself changes, so the assessment keeps walking it.
6. **Prove it**: push and let the UX job run base and head. Keep the change only if the targeted metric improves and no hard rule regresses on any engine or profile. Paste the before and after numbers and the head screenshots in the pull request.
7. **Say what CI cannot show.** Label every result emulated (`a-phone-check-names-its-platform`). A change to the soft keyboard, safe areas, gestures or the installed PWA also needs `mobile-ui-device-check` on the owner's iPhone before it counts as verified; a visual or wording change needs the owner's approval before merge.
8. **File what you did not do** as roadmap steps (`docs/plans/roadmap/`), each with the rubric line and number that motivated it.
