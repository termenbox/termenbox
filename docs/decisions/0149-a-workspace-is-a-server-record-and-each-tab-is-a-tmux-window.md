---
id: 0149-a-workspace-is-a-server-record-and-each-tab-is-a-tmux-window
title: A workspace is a server record and each tab is a tmux window
status: active
date: 2026-10-28
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [workspaces, tabs, protocol, client, roadmap]
amends: [0138, 0101, 0082]
related: [0054, 0136, 0137, 0146]
---

## Decision

- **A workspace is a record the server owns**, not a tmux session: `id`, `name` (a label, 0146), `dir`, `backend`, `order`, `focus`, and an ordered list of **tabs**. A tab is `id`, `label`, `kind` (the template it started from), `dir` (relative to the workspace), `order`, and the window it runs in (a tmux window in this stage).
- **A tab is a tmux window in this stage.** The workspace has one tmux session, created lazily when its first tab needs one and recreated when it is gone. All new tabs created here are terminal tabs: `shell`, `claude`, `codex`, `copilot`, `opencode`, `gemini`, `kilo`, `aider`, `goose`, `amp`, `qwen`, `cursor-agent`. The tab id and label live in tmux user options (`@termenbox_tab`, `@termenbox_label`), so a restart reconciles windows by tab, not by index. An ACP agent window is not a tmux window: ACP stays a first-class backend (0137), and its window host joins this model in R75 without changing the tab identity.
- **The registry is authoritative for tmux and command workspaces.** It is a JSON file next to the server's state, written atomically, with the order and labels every device sees. A tab's relative directory is resolved and canonicalised inside the workspace; `..`, absolute paths and symlinks that escape are refused.
- **A workspace is never empty.** Creating a workspace creates its first tab. Deleting the last tab creates a shell tab in the workspace directory. Closing the workspace kills every tab and removes the record.
- **Agent tabs are capped per workspace.** A workspace holds at most six agent tabs at once (`MAX_AGENT_TABS`, the same default as ACP's `max_sessions`). A tab counts unless its kind is `shell`; a dormant record still counts because it is a workspace record. Shell tabs stay creatable at the cap, ACP sessions keep their own cap, and a create that would exceed it is refused with one readable error before any agent starts.
- **ACP stays a first-class backend and the target, not a legacy path.** The UI keeps the per-agent Terminal/Agent view choice (0137); an ACP workspace remains creatable when the server offers `acp` and the owner enables it. For now it is represented as a single-agent workspace with one agent tab and no tab actions because its host is an ACP child, not a tmux window. `AcpView`, `AcpBackend`, its state and the `acp_*` messages stay; R75 folds the ACP window host into the tab model.
- **Compatibility.** New messages (`new_tab`, `rename_tab`, `close_tab`, `move_tab`, `focus_tab`, `move_workspace`) and `attach.tab` are served next to the old `windows`/`new_window`/`select_window`/`close_window`, which address tabs in registry order. `auth_ok` carries `tabs` and protocol 2; a client that does not see it keeps the old workspace strip and windows menu, and a server that does not serve tabs keeps the old view.
- **Status, answers and pushes are per tab.** The server sends `tab_states`; a hook's pane names its tab, so one tab's work, question or failure never colors another. Each tab has its own turn and obligation reducer (0122), fed by its hooks and its own pane. An answer goes to the tab that asked, a finished or failed item is acknowledged only on that visible tab, and a push names it (`&tab=`) and replaces only that tab's earlier notification. The workspace-level agent state stays the aggregate Home and history use.
- **Active tab is per device.** The server's `focus` is the default for a new device and for old clients; an incoming focus never switches a screen. The phone shows one plaque with the current tab and a dropdown; the desktop shows the strip and opens the same dropdown from the active tab. Detach is gone from the new path.

## Why

The owner works in one sandbox with several processes and wants the tab to be the process, not the workspace. Today a tab is a workspace, so two agents in one directory are two workspaces and three devices sort and name them separately. A server-owned record makes the list, the order and the labels identical everywhere, and makes a tab a first-class process that can be renamed, reordered and deleted without touching the others.

## Consequences

- tmux keeps one current window per session. All devices attached to a workspace share it unless per-device grouped sessions are added later; the client still remembers its own active tab and asks for it on attach.
- ACP workspaces are listed with one agent tab until R75. The single-tab view is a staging choice, not a deprecation: creation and the ACP view stay available.
- The server caps agent tabs at six per workspace separately from ACP children, counting dormant records and ignoring shell tabs; a cap refusal is one readable message.
- A tmux server that dies leaves the record dormant: opening it recreates the session and its tabs as shells with the same labels and order, and says so.
