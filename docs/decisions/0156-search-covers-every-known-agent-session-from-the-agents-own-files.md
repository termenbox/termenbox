---
id: 0156-search-covers-every-known-agent-session-from-the-agents-own-files
title: Search covers every agent session on the host, read from the agents' own files and scanned on demand
status: active
date: 2026-10-31
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, workspaces, client, server, privacy]
amended-by: [0182-search-covers-every-session-in-the-agents-session-roots-with-no-ledger]
related: [0105-reports-are-fleet-board-timeline-usage-and-health, 0152-a-dormant-workspace-resumes-its-agents-when-reopened, 0149-a-workspace-is-a-server-record-and-each-tab-is-a-tmux-window, 0139-control-bytes-and-events-travel-on-separate-channels-and-events-replay-from-a-sequence-log, 0095-agent-status-is-activity-plus-attention]
---

## Decision

- **One global Search sheet** (sidebar on desktop, header on phone) finds text in agent conversations. It is separate from R72's Find, which stays inside one terminal pane.
- **Corpus: the agents' own session files** on the owner's host. Amended by 0182: every session file inside each adapter's session root, not only sessions termenbox has a record of. termenbox stores no new copy of any conversation and no ledger, so 0105 (history is metadata only) stands.
- **Scan on demand, no index in v1.** The server streams session files newest first with a bounded scan (files, bytes and time capped) and returns hits as it finds them, saying when a cap cut the search. A persistent index is deferred; the trigger to revisit is a scan that regularly exceeds its time cap on a real owner's history.
- **The query goes to the server** (the text lives there). The sheet says so. This is the opposite of R72, where the query stays in the browser.
- **A hit is a scrubbed snippet** (same scrubber as the event log, 0095), with agent, workspace, tab, date and a state badge (`open` or `resume`). Raw tool input and output are not searched or shown.
- **Tapping a hit:** the workspace is up, so switch to the tab; the workspace is down, so offer Resume (0152's command table or ACP `session/load`), always as an explicit action, never started by a search.
- **Two landing modes.** An ACP workspace scrolls `AcpView` to the turn and highlights the occurrence, with older turns brought in by the session-log port (0157). A terminal (TUI) tab only switches: the agent's own UI cannot be scrolled from outside (fact 0065), and nothing is typed into an agent to try.
- **Matching:** case- and accent-insensitive; newest first; at most 50 hits and 3 per session; the scan stops at 2 s or 200 MB read.

## Why

The owner works with several agents across workspaces and wants to find where something was said, including in sessions whose workspace is not running. The agents already keep full conversations on disk, so reading them is lossless, costs no storage and needs no agent process. An ACP view can honestly scroll and highlight; a TUI cannot, and driving its keys risks submitting text to the model, so the TUI path stays a switch.

## Rejected

| Option | Why not |
|---|---|
| Scrollback of every tab (R72 generalised) | Dies with the tmux server, blind inside a full-screen agent, misses dormant workspaces |
| Titles and metadata only (0105 history) | Cannot find anything said in a session |
| ACP `session/list` + `session/load` as the corpus | Spawns an agent per pass, slow, may need a login |
| Persistent full-text index | A second copy of every conversation (secrets, retention, deletion sync) and CPU/RAM on a 512 MB box, before a scan is shown to be slow |
| Search every transcript under the agents' dirs | Reaches past what the owner opened in termenbox |
| Drive the agent TUI's own search on tap | Only Claude Code fullscreen is documented; a missed Ctrl+O types the query into the prompt |

## Consequences

- Sessions of other projects on the same host are found since 0182 widened the scope.
- Each agent's file format is outside our control (fact 0065): readers fail soft and are fixture-tested (0157).
