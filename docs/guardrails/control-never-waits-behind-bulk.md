---
id: control-never-waits-behind-bulk
title: Control messages never wait behind bulk traffic
status: active
governed-by: 0139
grounded-in: [0139, 0096]
derivation-note: Cancel and permission answers must work on a slow link while output floods, on WebRTC and on the single-stream WebSocket fallback.
tags: [protocol, transport, performance]
---

# Guardrail: Control messages never wait behind bulk traffic

## Rule
**MUST** send control messages (cancel, interrupt, kill, permission answers, prompt submit, focus, window operations) on their own channel or priority queue. **MUST** bound the size of event and byte frames so a control message waits at most one frame, on every transport. **MUST** acknowledge each control message with a result.

## Enforcement
- Server test: with the event and byte queues full, a control message is written before the next bulk frame on the WebSocket path.
- E2E: a cancel sent during a large tool-output flood arrives within a stated bound (set when the test is written, from a measured baseline).
