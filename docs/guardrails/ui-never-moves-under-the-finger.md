---
id: ui-never-moves-under-the-finger
title: The interface never moves a control or switches the session under the user's finger
status: active
governed-by: 0082
grounded-in: [0082]
derivation-note: Given a list that reorders and signals that arrive on their own (0082), a tap must still land on the thing the user read.
tags: [client, mobile]
---

# Guardrail: The interface never moves under the finger

## Rule
**MUST NOT** switch the active tab or session on its own, whatever arrives. **MUST NOT** reorder, insert or remove a card or tab while a touch is down, a card is expanded, or for 2 s after a scroll; apply the change afterwards without an animation that moves a control. **MUST** attach a one-tap answer to the prompt it was read from. A list that fills while it is read (search results) meets the first rule by being append-only: it never sorts or regroups on arrival (decision 0082).
