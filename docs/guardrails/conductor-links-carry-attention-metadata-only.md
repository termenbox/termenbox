---
id: conductor-links-carry-attention-metadata-only
title: Node-to-conductor links carry attention metadata only, bounded and scrubbed; terminal bytes cross them only on an explicit open
status: active
date: 2026-11-01
governed-by: [0166]
grounded-in: [0166, 0095, resource-constraints, security-secrets-handling]
derivation-note: A fleet's event volume and the secrets in terminal output both make a metadata-only link the safe and affordable default.
tags: [security, network, fleet, control-plane, performance]
---

# Guardrail: Node-to-conductor links carry attention metadata only, bounded and scrubbed; terminal bytes cross them only on an explicit open

## Rule
**MUST** send only obligation, state, rule, usage and incident records on the standing link, with agent text capped, control characters stripped and scrubbed (`notifications-carry-no-agent-text` for push). **MUST** send terminal bytes only for a terminal the operator has opened, and stop when it closes. **MUST** bound the conductor's per-node queue and history by bytes and age (`resource-constraints`), and drop with a visible count, never grow.

## Enforcement
- Test: a node with 50 idle agents sends under a fixed bytes-per-minute budget; no terminal bytes without an open.
