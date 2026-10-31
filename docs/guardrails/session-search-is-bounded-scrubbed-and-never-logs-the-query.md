---
id: session-search-is-bounded-scrubbed-and-never-logs-the-query
title: Session search is bounded, scrubbed, confined to known folders, and never logs the query
status: active
date: 2026-10-31
governed-by: 0156,0157
grounded-in: [0156, 0157, resource-constraints, security-secrets-handling, audit-logging-no-secrets]
derivation-note: Search reads whole conversations that hold secrets, on a 512 MB / 1 vCPU box, so it must be capped, scrubbed and silent about what was asked.
tags: [security, privacy, performance, server]
---

# Guardrail: Session search is bounded, scrubbed, confined to known folders, and never logs the query

## Rule
**MUST** cap every search by files, bytes read and wall time, run it off the control path on a blocking thread, make it cancellable, and say when a cap cut it short. **MUST** scrub every snippet and window like the event log is scrubbed (decision 0095) and skip raw tool input and output. **MUST** read only session files whose directory belongs to a workspace or tab the server knows. **MUST NOT** put the query, a snippet or a path into a log line, audit entry, crash file or diagnostics bundle. **MUST NOT** start, resume or write to an agent as part of a search; resuming is a separate explicit user action. **MUST NOT** use `mmap` on a session file.
