---
id: 0147-a-workspace-is-a-server-record-and-each-tab-is-a-tmux-window
title: A workspace is a server record and each tab is a tmux window
status: active
date: 2026-10-28
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [workspaces, tabs, protocol, client, roadmap]
amends: [0138, 0137, 0101, 0082]
related: [0054, 0146]
---

## Decision

- **A workspace is a record the server owns**, not a tmux session: `id`, `name` (a label, 0146), `dir`, `backend`, `order`, `focus`, and an ordered list of **tabs**. A tab is `id`, `label`, `kind` (the template it started from), `dir` (relative to the workspace), `order`, and the tmux window it runs in.
- **A tab is a tmux window.** The workspace has one tmux session, created lazily when its first tab needs one and recreated when it is gone. All new tabs are terminal tabs: `shell`, `claude`, `codex`, `copilot`, `opencode`, `gemini`, `kilo`, `aider`, `goose`, `amp`, `qwen`, `cursor-agent`. The tab id and label live in tmux user options (`@termenbox_tab`, `@termenbox_label`), so a restart reconciles windows by tab, not by index.
- **The registry is authoritative for tmux and command workspaces.** It is a JSON file next to the server's state, written atomically, with the order and labels every device sees. A tab's relative directory is resolved and canonicalised inside the workspace; `..`, absolute paths and symlinks that escape are refused.
- **A workspace is never empty.** Creating a workspace creates its first tab. Deleting the last tab creates a shell tab in the workspace directory. Closing the workspace kills every tab and removes the record.
- **ACP is kept for a future variant B, not removed.** An existing ACP workspace is shown as a legacy workspace with one agent tab and no tab actions; new ACP workspaces are not created from the UI (only an old protocol client during the legacy window). `AcpView`, the ACP backend, its state and the `acp_*` messages stay.
- **Compatibility.** New messages (`new_tab`, `rename_tab`, `close_tab`, `move_tab`, `focus_tab`, `move_workspace`) and `attach.tab` are served next to the old `windows`/`new_window`/`select_window`/`close_window`, which address tabs in registry order. `auth_ok` carries `tabs` and protocol 2; a client that does not see it keeps the old workspace strip and windows menu, and a server that does not serve tabs keeps the old view.
- **Active tab is per device.** The server's `focus` is the default for a new device and for old clients; an incoming focus never switches a screen. The phone shows one plaque with the current tab and a dropdown; the desktop shows the strip and opens the same dropdown from the active tab. Detach is gone from the new path.

## Why

The owner works in one sandbox with several processes and wants the tab to be the process, not the workspace. Today a tab is a workspace, so two agents in one directory are two workspaces and three devices sort and name them separately. A server-owned record makes the list, the order and the labels identical everywhere, and makes a tab a first-class process that can be renamed, reordered and deleted without touching the others.

## Consequences

- tmux keeps one current window per session. All devices attached to a workspace share it unless per-device grouped sessions are added later; the client still remembers its own active tab and asks for it on attach.
- The server caps simultaneously running agent tabs separately from ACP children; a cap refusal is one readable message.
- A tmux server that dies leaves the record dormant: opening it recreates the session and its tabs as shells with the same labels and order, and says so.
