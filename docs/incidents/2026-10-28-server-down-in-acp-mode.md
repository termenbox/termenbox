# Incident 2026-10-28: the server went down while working in ACP mode

Status: cause unknown. Evidence missing: no log of any kind survived (fact 0061).

## Timeline
- The owner was working in an ACP workspace. Something failed and the termenbox server process died. Time, host, agent and unit are not recorded; the owner did not note them.
- Afterwards there were no logs to read. The server logged plain text to stderr only and had no panic hook or run marker.

## Cause
Unknown. Candidates: a panic outside a tokio task or a poisoned lock; an OOM kill (several adapters on a small box, plan `acp-session-modes` choice F); an agent-child fault. None can be confirmed.

## What changed
- Decision 0142: own JSONL logs, panic hook, run marker.
- Decision 0143: per-session supervision, restart policy, safe mode, meaningful `/health`.
- Decision 0144: user-triggered diagnostics bundle, incident prompt, issue template, postmortems.
- Plan `docs/plans/archive/server-viability-and-logging.md`.

## Open
When the next unclean exit happens, the run marker and logs should name the cause; add it here.
