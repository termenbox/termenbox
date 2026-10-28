---
id: investigate-a-server-incident
title: Investigate a server incident from its evidence and record it
status: active
date: 2026-10-28
tags: [server, observability, procedural]
---

# Procedure: Investigate a server incident from its evidence and record it

Use when a server died, hung or restarted unexpectedly, yours or a user's (decisions 0142, 0143, 0144).

1. **Get the evidence first.** Ask the app for the diagnostics bundle (it is scrubbed and previewed; never ask for raw logs or terminal content). On your own host read `<config-dir>/logs/termenbox.jsonl`, the newest `crash-*.json`, and `journalctl -u <unit>` or `docker logs` as a cross-check. Do not restart anything before copying them.
2. **Classify the death** from the record: a crash file means a panic (read the location and thread); `previous run ended uncleanly` with no crash file means OOM kill or SIGKILL (check `dmesg`/`journalctl -k` for the OOM killer and the last health sample's RSS); a hang shows a log gap with the process alive and a failing `/health`.
3. **Find the trigger.** Which workspace and agent was live, whether a turn was open, the last ACP event kinds (never message text). Reproduce with `scripts/acp-probe.mjs` against a throwaway working directory, never the owner's sessions.
4. **Fix and pin.** Add a test that fails without the fix; check the supervisor and safe-mode guardrail (`a-fault-in-one-session-never-stops-the-server`) still hold.
5. **Record.** Write `docs/incidents/<date>-<slug>.md` (timeline, cause or "unknown, evidence missing: X", changes, links). New truths become facts, new commitments decisions (use `capture`). Close the GitHub issue labelled `incident` with the postmortem link.
