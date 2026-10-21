---
id: prioritise-the-roadmap-by-wsjf
title: Order the roadmap by WSJF and dependencies
status: active
date: 2026-10-21
tags: [roadmap, procedural]
---

# Order the roadmap by WSJF and dependencies

Applies to `docs/plans/termenbox-roadmap.md` (index) and its step files in `docs/plans/roadmap/`. Run it when a step is added, finished or re-scored. There are no priority tiers (decision 0125).

## Score

Score each pending step with V, T and R from 1 to 10 and S from the size list. WSJF = (V + T + R) / S. Write the numbers in the `Score:` header and one line per number under `## Score rationale`, naming the evidence (a fact, a metric, a code path, a dependant). A number with no reason is a guess; say so.

| Score | Anchors |
|---|---|
| **V** value | 9-10 the product's core promise or the free gate fails without it; 7-8 most users meet it weekly, or it removes a top friction; 5-6 a clear gain for a large segment, or it unlocks several steps; 3-4 a minority or hygiene; 1-2 marginal |
| **T** time criticality | 9-10 a deadline, a live bug or a security exposure; 7-8 it blocks scheduled work or the gate; 5-6 value decays slowly or a dependant waits; 3-4 no cost to waiting yet, matters at scale; 1-2 can wait indefinitely |
| **R** risk reduction or opportunity | 9-10 removes a likely outage, security, data-loss or legal exposure, or enables a major capability; 7-8 removes a known risk or proves feasibility before an expensive build; 5-6 enables several later steps or guards a core path; 3-4 small; 1-2 none |
| **S** size | 1 under half a day; 2 about a day; 3 one pull request of two to three days; 5 about a week, one or two pull requests; 8 about two weeks, several pull requests; 13 three weeks or more, or a decision plus several components. Count owner-only work (a device run) in the size of the step that needs it |

Check a score against measured numbers where they exist: fact 0022 for the Cloudflare budget, fact 0039 for what has been run on a real device, the step's own `Do` list for size.

## Order

1. A step takes the best bundle score of any free step it unlocks (the chain's summed V + T + R over its summed sizes), so a low-scoring prerequisite of a high-value step does not wait.
2. Order by lane (free before paid, decision 0093), then that chain score, then the step's own WSJF, never placing a step before its predecessors (`Preds:`).
3. A step the owner pinned stays where they put it (`Pin: after Xnn`), with the date and reason in the step.
4. A step whose Status is `blocked` stays in the table but is not taken as next.
5. Edit the step file's `Score:` and `Status:` lines and run `python3 docs/plans/roadmap/check.py && python3 docs/plans/roadmap/rank.py`, which rewrites the Order table and applies steps 1-4. Mark finished steps `done (PR #n)` and `git mv` them to `roadmap/archive/`.
6. Re-score when a step finishes, its size turns out wrong, or the facts under a number change (a quota measured, a device run recorded). When most steps are done, move the plan to `docs/plans/archive/` and start a new one.

WSJF divides by size, so spikes and small fixes rank high. That is intended; if a large gating step should go first, raise its T or pin it, and write the reason down.
