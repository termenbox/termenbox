---
id: protocol-core-stays-acp-v1
title: The protocol core stays ACP v1; extensions are namespaced, negotiated and ignorable
status: active
governed-by: 0136
grounded-in: [0136, 0111]
derivation-note: The owner commits to an ACP v1 core with an extension registry (0136), and agents and clients only interoperate if removing the extensions leaves valid ACP.
tags: [protocol, agents, architecture]
---

# Guardrail: The protocol core stays ACP v1

## Rule
**MUST** keep every core message and content shape identical to ACP v1. **MUST** carry anything termenbox adds (windows, focus, control, `seq`/`epoch`, tier) only as a namespaced extension, negotiated at `initialize`. **MUST** skip, not fail on, an extension a peer does not know. **MUST NOT** change a core shape without a protocol version bump (decision 0136).

## Enforcement
- CI replays the recorded fixtures in `packages/server/testdata/acp/` through the server, strips every extension, and validates the result against the schema crate.
- CI asserts that the client renders every standard `session/update` kind and falls back to the generic renderer for an unknown extension or content type.
