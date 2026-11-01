---
id: 0174-the-human-works-in-one-stream-of-cluster-cards-in-a-sliding-window
title: The human works in one stream of cluster cards in a sliding window; a card is a self-contained brief with a recommendation, a risk line and a time-box
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, client, ux, mobile, agents]
amends: [0100]
related: [0104, 0165, 0169, 0171, 0172, 0173, 0175]
---

## Decision

- **One stream.** Home's Needs-you queue (0100) becomes a single ordered stream across all nodes. Order is by value of attention: urgency class and time-box first, then blocked agents and cost of waiting, then age.
- **The unit is a cluster** of equivalent open obligations ("37 agents want to run X"); a singleton is a cluster of one. One tap answers the whole cluster. After the answer the card offers "make this a rule" or "raise the deputy for this class" (0165, 0173).
- **A card is a brief**, readable alone in a few seconds: what is asked, in one line; the facts needed to decide (contract, diff stat, path scope, command, cost so far, what the agent was doing); the deputy's recommended answer with confidence and why; the risk line of 0165; what happens if nobody answers and when. Evidence (the diff, the transcript, three similar past decisions) is one tap away, not on the card.
- **Clustering is shown, not trusted.** A cluster card states the fact its members share and lists outliers; one tap splits a member out. A wrong cluster is demoted like a wrong answer.
- **A sliding window.** The screen holds the current card, the next two and a count of the rest; the window slides as cards are answered and prefetches the evidence of the next card. The stream obeys `ui-never-moves-under-the-finger`.
- **Voice and one-hand reach.** Answers are available by tap, swipe and dictated reply.
- UI follows the template-first rule (0167) and the design rules in `docs/plans/assets/ux/DESIGN-PRINCIPLES.md`.

## Why

Instance cards make human work grow with the number of asks. Cluster-first makes decisions per human grow slowly with agents, which is what 10^3 and 10^4 need; the shared fact and outlier list keep a mis-cluster visible.

## Not chosen

- Instance-first with a batch shortcut (linear in asks), and a model-chosen mix of cluster and instance (more to calibrate; a later refinement).
