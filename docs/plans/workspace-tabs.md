# Workspace tabs

Decision: 0147. Supersedes the in-session model of 0101/0082 where a tab was a workspace; keeps 0138's workspace/window
vocabulary, 0054's backend port and 0146's minted id with a label.

## Model

- A **workspace** is a server record: `id`, `name` (label), `dir`, `backend`, `order`, `focus`, `tabs`.
- A **tab** is a process in that workspace: `id` (`t1`, `t2`, …), `label`, `kind` (`shell`, `claude`, …), `dir`
  (relative to the workspace), `order`, and the tmux window it runs in (`@12`, absent while dormant).
- New tabs are terminal tabs. ACP workspaces stay in the ACP backend and are shown as a legacy workspace with one
  agent tab; the UI never creates a new ACP workspace.
- The tmux session is created lazily with the first tab and recreated when it is gone. The record stays and its tabs
  come back as shells with the same labels and order.

## Server (delivered in PR1)

`registry.rs` owns the record, migration and order; tmux windows carry `@termenbox_tab` and `@termenbox_label`, and
`WindowInfo` keeps its old shape. Handlers: `new_tab`, `rename_tab`, `close_tab`, `move_tab`, `focus_tab`,
`move_workspace`, plus `attach`/`peek`/`answer`/`foreground` with a tab. Old `windows`/`new_window`/`select_window`/
`close_window` address tabs in order. `auth_ok.tabs` tells a client the server serves them.

## Client

- **PR2 (this one).** Types, `HubWorkspace.tabs`, `hub` tab methods, server order, `workspaceTabs.ts` (active tab,
  aggregate status, path validation), per-tab snapshots, Workspaces page pencil and tab count. No new screen yet.
- **PR3.** Top bar plaque/strip, `TabMenu` (switch, drag, rename, delete, `+ New window`, Close workspace), workspace
  creation with its first tab, `ServerStatusIcon`, deep link groundwork.
- **PR4.** Per-tab states (`tab_states`), prompts/answers routed by tab, attention ack for the visible tab, tab-level
  push and foreground, Cache expiring/Recent in the tab menu, guides.

## Rules

- Status aggregate: Needs you → Error → Working only when every agent tab works → Idle → Done/Unknown. Terminal tabs
  with no agent state are ignored; a workspace of shells is Idle.
- The active tab is the device's own; the server's `focus` is the default for a new device and never switches a screen.
- The last tab is replaced by a shell tab, so a workspace is never empty. Deleting an agent tab asks first; a shell
  tab asks only while its foreground process is not a shell. Close workspace always asks.
- Server order is the order everywhere. The device-local order is a fallback for servers without `order`.
- A tab's directory is relative, without `..`, absolute paths or symlinks that leave the workspace; the server checks
  it again.

## Tests

Rust: registry with real tmux (migration, dormant recreation, last tab, order), handler e2e on both transports, old
window aliases. Client: aggregate matrix, active-tab fallback, order and directory rules, Workspaces page. E2E: tabs on
phone and desktop, migration, old client/new server and new client/old server.
