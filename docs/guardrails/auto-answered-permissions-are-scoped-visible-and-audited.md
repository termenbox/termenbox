---
id: auto-answered-permissions-are-scoped-visible-and-audited
title: A permission answered by a rule is scoped, matched on parsed arguments, visible in the log, audited, and never destructive by default
status: active
date: 2026-11-01
governed-by: [0165, 0172, 0173]
grounded-in: [0165, 0143, security-secrets-handling, audit-logging-no-secrets]
derivation-note: A rule answers on the owner's behalf with no human present, so its reach, its matching, its trace and its off switch must be fixed in advance.
tags: [security, agents, acp, server]
---

# Guardrail: A permission answered by a rule is scoped, matched on parsed arguments, visible in the log, audited, and never destructive by default

## Rule
**MUST** create a rule only from an owner's explicit action after a confirmation that shows its scope, limited to one workspace and one tool kind, with its creator and time. **MUST** match a shell command on parsed arguments, and send any compound or indirect form (`&&`, `||`, `;`, pipe, redirect, command substitution, `eval`, an unparsable string) to the user. **MUST** show every auto-answered request as a marked line in the stream and write an audit entry that holds the kind, the rule id and the outcome, never command output or a secret. **MUST** keep a one-tap switch that turns all rules off and a way to revoke each. **MUST NOT** auto-allow `delete`, a path outside the workspace directory or a network fetch unless a rule names that case exactly. **MUST NOT** let a rule answer for a workspace other than its own, or widen what the agent's own sandbox allows. **MUST NOT** create a rule from a suggestion without the owner's confirmation of its preview. **MUST** apply every limit above to an answer given by a mission contract (0172) or by the deputy (0173), mark it with its source (`contract:<id>` or `deputy:<class>`) and audit it the same way. **MUST NOT** let a fault in the rule store stop the server or answer a request by default: on any doubt the request goes to the user (decision 0143).
