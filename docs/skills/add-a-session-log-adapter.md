---
id: add-a-session-log-adapter
title: Add a session-log adapter for an agent
status: active
date: 2026-10-31
tags: [agents, server, procedural]
---

# Procedure: Add a session-log adapter for an agent

Use when an agent keeps its conversations in files and should be searchable (decisions 0156, 0157). The port lives in the server's `session_log` module; an adapter implements `search`, `window` and `page` for one agent.

1. Find the agent's session directory and file format from its docs and from a throwaway run with a scratch `HOME`, never the owner's sessions. Note how a rewind, fork, compaction and a tool call are written. Record the findings in a fact (extend 0065).
2. Record fixtures under `packages/server/testdata/session_log/<agent>/`: a short conversation, one with a rewind or fork, one torn at the last line, one truncated, one with an unknown version, and one with a secret in a tool output.
3. Implement the adapter: map records to the unified entries, send an unmappable record as a `notice`, pick the active branch, skip tool input and output, scrub the text. Scan with `pread` and `memchr::memmem`; parse only lines that match.
4. Run the shared contract suite over the fixtures, then the benchmark on the large fixture against the time and peak-RSS budgets.
5. Register the adapter by agent name. An unreadable or unknown-version file disables search for this agent only and is reported in the Search sheet.
6. Update fact 0038/0065 with the agent version measured.
