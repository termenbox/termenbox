---
id: operator-actions-carry-an-actor-id-apart-from-the-owner
title: Every operator action, rule, contract and audit entry records an actor id separate from the owner id
status: active
date: 2026-11-01
governed-by: [0167, 0171]
grounded-in: [0167, 0171, 0053, audit-logging-no-secrets]
derivation-note: Team mode (quorum, zones) is planned; adding an actor to records later would be a migration of every record and message.
tags: [security, audit, control-plane, protocol]
---

# Guardrail: Every operator action, rule, contract and audit entry records an actor id separate from the owner id

## Rule
**MUST** put `actor` (who acted: a person or `deputy:<class>` or `rule:<id>`) and `owner` in every answer, rule, contract, lease and audit record and in the wire messages that carry them. With one owner the two values may be equal; code **MUST NOT** treat them as the same field.

## Enforcement
- Protocol tests in `packages/core` fail on an answer, rule or contract message without `actor`.
