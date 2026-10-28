---
id: a-fault-in-one-session-never-stops-the-server
title: A fault in one session never stops the server, and a death always leaves a record
status: active
date: 2026-10-28
governed-by: 0142,0143
grounded-in: [0142, 0143]
derivation-note: The owner wants maximum viability (0143); the 2026-10-28 incident left no evidence (fact 0061, 0142).
tags: [server, agents, observability]
---

# Guardrail: A fault in one session never stops the server, and a death always leaves a record

## Rule
**MUST** run each ACP session's tasks under a supervisor so a panic, closed pipe or agent exit fails that workspace only. **MUST NOT** hold a lock that a panicking session can poison for the others. **MUST** write the run marker at start and leave a log line or crash file for every panic. **MUST NOT** restore a workspace that was live at the last two unclean exits as running.

## Enforcement
- Test: a panic in one session's task leaves the others answering.
- Test: an unclean exit is reported on the next start; two in a row restore the live workspace stopped.
