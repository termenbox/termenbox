---
id: the-deputy-is-optional-capped-and-owner-supplied
title: The deputy is optional, owner-supplied and capped, and the system works with it off
status: active
date: 2026-11-01
governed-by: [0169, 0166]
grounded-in: [0169, no-paid-services, security-secrets-handling]
derivation-note: No paid service may be required; a model call costs the owner money and may carry terms limits (fact 0066).
tags: [deployment, deputy, security, control-plane]
---

# Guardrail: The deputy is optional, owner-supplied and capped, and the system works with it off

## Rule
**MUST NOT** ship a model, a key or a hosted deputy service, or require one. **MUST** keep contracts, rules, clusters and escalation fully working with the deputy off. **MUST** enforce a per-owner monthly token cap and stop calling at the cap, with a visible notice. **MUST** keep the `claude` print-mode backend off by default, behind a notice that cites fact 0066, and **MUST NOT** call the Agent SDK for the deputy. **MUST** keep the endpoint key in the server's secret store, never in a log or a wire message.

## Enforcement
- Test: the deputy disabled, every layer except 3 still answers; a call at the cap is refused and shown.
