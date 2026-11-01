---
id: 0182-search-every-session-in-the-agents-session-roots-with-no-ledger
title: Search covers every session in the agents' own session roots, with no ledger of closed sessions
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, server, client, privacy]
amends: [0156-search-covers-every-known-agent-session-from-the-agents-own-files]
related: [0152-a-dormant-workspace-resumes-its-agents-when-reopened, 0157-agent-session-files-are-read-through-a-session-log-port-with-one-adapter-per-agent, 0105-reports-are-fleet-board-timeline-usage-and-health]
---

## Decision

- **Corpus:** every session file inside each adapter's own session root on the host (Claude's projects directory, Codex's sessions directory, resolved from the agent's home). It is no longer limited to sessions termenbox has a record of. Closed tabs, closed workspaces, replaced sessions and sessions run in a plain terminal are all found.
- **No ledger.** termenbox stores nothing new: no session ids, directories or names are kept after a tab closes, nothing needs pruning and there is no "forget" action. 0105 stands. The agent's own file is the only source of truth, so a deleted file means the hit disappears.
- **Confinement moves from folders to roots.** Search reads only inside an adapter's root, never follows a symlink out of it, never reads a path the client supplied and never reads a format the adapter does not recognise. The other rules of the guardrail `session-search-is-bounded-scrubbed-and-never-logs-the-query` are unchanged.
- **A hit's directory** is read from the file (`cwd` in Claude's records, `session_meta.payload.cwd` in Codex's rollout), never decoded from the folder name.
- **Badges and grouping:** `open` for a session a running tab holds, `resume` for every other. A hit groups under the workspace that holds it, or under its directory (last two path segments, full path as a tooltip) when none does. The grouping modes of the follow-ups plan stay.
- **Resume** stays an explicit tap with a confirmation, never started by a search. If a workspace already holds the same agent and session id, it switches to that tab and opens nothing. Otherwise it creates a new workspace named after the directory and runs 0152's resume command there. If the directory is gone, it says so and offers nothing.
- **Limits are unchanged:** 50 hits, 3 per session, 2 s, 200 MB, and the 30-day window. The trigger to revisit is a scan that regularly hits its cap on a real owner's history; a scope chip ("this server's workspaces") is the first remedy considered.

## Why

The owner wants to find where something was said wherever it was run. A ledger would guard the "only what termenbox opened" boundary the owner can already cross by running an agent anywhere, and would add a store, pruning and a clear action to do it. The server runs as the owner, with one owner per server, so reading the agents' own files widens no one's access.

## Rejected

| Option | Why not |
|---|---|
| A ledger of closed sessions (id, directory, names, time) | A new store to bound, prune and clear, to protect a boundary that gives no real protection |
| Remember opened directories only | Half a ledger; labels closed hits by directory anyway |
| Keep closed records in the registry | Changes what close means (0149, 0152) and risks ghost workspaces in every list |
| Drop the confinement altogether | Removes the rule that stops a path bug from reading arbitrary files |

## Consequences

- Sessions of unrelated projects on the host appear in results, scrubbed and bounded like the rest.
- Closed hits carry no workspace or tab name, only the directory.
- File `mtime` stays the free skip heuristic for step 6; there is no recorded last-event time to override it.
- Resume can create a workspace, which is a new side effect of an explicit tap.
