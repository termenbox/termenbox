---
id: ux-scenarios-assert-a-state-before-they-capture-it
title: A UX scenario asserts the state it claims to show before it records or screenshots it
status: active
governed-by: 0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set
grounded-in: [0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set, 0056, 0148-the-new-workspace-dialog-shows-recents-and-puts-the-rest-in-layers-inside-it]
derivation-note: Capture scripts that skip a step when the target is not visible pass while showing nothing (the New workspace sheet shots at e89424a never showed a picker); a capture is evidence only if its state was asserted first.
tags: [client, ux, ci, testing, video]
---

# Guardrail: Assert the state, then capture it

## Rule
A scenario step that records or screenshots a state **MUST** first wait for and assert that state (the layer is visible, has the expected title and rows, the chosen value is shown). A failed assertion **MUST** fail the run.
A scenario **MUST NOT** wrap a capture in `if (await x.isVisible())` or any branch that skips it.
A video **MUST** be recorded from the same steps that pass as a test, with human pacing (typing and tap delays, a visible cursor or touch spot).
