# Workspaces and tabs

Back to the [README](../../README.md). New to this? [Chapter 3, First workspace](../course/03-first-workspace.md) walks through it in a few minutes. Related: [Concepts](../concepts.md), [Workspace backends and the Agent view](./backends.md), [Notifications and agent events](./notifications.md).

## Workspaces and tabs

A **workspace** is a server-owned record: a name, a directory and the tabs inside it. A **tab** is one process window of that workspace: a tmux window in this stage, running a shell, an agent TUI or any command. The workspace is never empty; its first tab is created with it, and deleting the last tab leaves a fresh shell tab instead.

- **Desktop navigation.** The global top bar keeps Home, Workspaces, Settings and Activity visible, with Search on the right. Ctrl/Cmd+K opens the same unified workspace and conversation search. The Workspaces badge counts workspaces needing you. The sidebar contains servers and their workspaces. A server heading expands or collapses its children; it never opens a workspace. The arrow in the global header hides the sidebar completely and opens it again. Each server has its own `+` to create a workspace on that server.
- **Workspaces page.** Each server heading has its own `+`, including servers with no workspaces. It creates a workspace on that server. Desktop rows keep a pencil and close button with the same confirmation as the sidebar. Phone rows have one floating actions menu for Rename and Close workspace; closing still asks for confirmation. There is no global `+ New` button on the list.
- **Search.** Workspace-name matches and lexical conversation matches appear in one centered dialog, with their server/workspace identity. Conversation range, grouping, paging, cancellation and resume confirmation remain available. Search contains no navigation, server-management or permission commands; use the navigation, Settings → Servers or the decision itself. The query is cleared when search closes.
- **Dialogs.** Ordinary dialogs center inside the usable viewport on desktop and phone. Long content scrolls internally; folder pickers remain inside their parent dialog, and action menus stay anchored without moving the list.
- **Workspace views.** Decision and Terminal are separate views of the same workspace, switched with the one button in the tab row; Decision has no terminal button of its own. Switching preserves the terminal and connection; there is no side-by-side terminal or Alt/Option+P shortcut. Decision has two levels. The first is the question with its answers, or, when nothing is asked, the agent's state and the step it is running; the answers stay pinned above the reply field while the question is on screen. The second is the task, the plan, the latest actions (newest first), the changed files and the agent's last message, then the terminal output. The output is folded while the agent's hooks supply that history and open otherwise. On a wide pane the second level sits beside the first. Every line is what the agent, its hook or its screen reported: nothing is estimated or recommended. Each workspace keeps its last view. When you return to it, or the connection drops, the old question stays on screen, the header line says "Refreshing the view…" or "Offline · view from HH:MM", and the answers are locked until the server confirms the view again. While the terminal shows and a question waits in the open tab, a small dot on the view switch says so; there is no banner.
- **Inbox and account.** Home opens Inbox and shows its unread count. Settings contains Account (avatar, username, sign out) and the client version, plus Servers, diagnostics and appearance. Search stays in the desktop header and beside the phone Workspaces heading. There is no hamburger menu.
- **Desktop tabs.** The sidebar lists workspaces; the tab strip at the top of an open workspace lists its tabs. `+` creates another tab in the same workspace, not another workspace. A pencil renames the tab; the server keeps the order, labels and focus for every device.
- **Following tmux.** With tmux 3.2 or newer the top bar follows tmux's current window: `Ctrl+B <n>` or `tmux select-window` selects the matching tab on every device viewing that workspace, without re-attaching the terminal. A server without control mode keeps each device's own tab.
- **Same order everywhere.** The tab strip and tmux's windows share one order: dragging a tab reorders the tmux windows too, and closing a tab leaves no gap, so `Ctrl+B <n>` selects the tab shown at position `n`. Window ids and terminal contents do not change, and the current tab stays current.
- **Phone.** Home, Workspaces, Activity and Settings stay in the bottom navigation; there is no global top bar. One plaque names the current tab; tap it for the tab list. There is no separate Workspaces sheet in an open workspace. Swipe or tap a row to switch tabs; the ⋮ menu renames, reorders or deletes a tab (deleting asks first).
- **Tab actions.** The open workspace’s tab dialog uses a floating ellipsis menu with Rename and Delete tab. The list never shifts when it opens. Delete opens a compact confirmation before sending the request; Escape or an outside click closes the menu. ACP tabs have no editing actions.

**New tab.** Choose what to start in the tab. Its directory is relative to the workspace and may not leave it (`..`, an absolute path or a symlink escape is refused). The list is also in [Agents](../reference/agents.md).

Shell, Claude Code, Codex, Copilot, OpenCode, Gemini, Kilo, Aider, Goose, Amp, Qwen, Cursor.

**Agent tabs.** Shell tabs do not count and can still be added. An agent tab beyond the limit is refused with a message to close one first.

A workspace holds at most 1 agent tab.

**Status.** Each tab shows what its agent does now. The workspace row shows the first of these that any agent tab has. The same order is on [Status](../reference/status.md). A shell tab has no agent status and is ignored.

1. **Needs you**
2. **Error**
3. **Done**
4. **Idle**
5. **Working**

**No status** is not ranked: the workspace row shows it only when no tab has a status.

- **Recent and cache.** The tab menu filters by **Recent** (last reply first) or **Cache expiring** (a Claude Code prompt cache in its last minute). These filters belong to the tab menu, not the Workspaces page.
- **Detach or close.** On a server without tabs, Detach still hides the workspace and keeps it running. In the tab model, closing a workspace asks first and ends it and all its tabs. A finished or failed tab is acknowledged only when that tab is visible.
- **Restore.** When the tmux server is gone, the workspace record remains, marked dormant. Opening it recreates its tabs as shells with the same labels and order; the work inside them is gone.
- **Needs you.** When an agent waits for an answer, the banner names the workspace and offers its choices; the answer goes to the tab that asked, even when another tab of the same workspace is in front. A push deep link opens that tab, not just the workspace.
- **Servers.** The Servers screen lists the saved servers; Add Server shows the install command for a container or for host mode and waits for the server to appear.

An older server (protocol 1) still opens workspaces from the sidebar or Workspaces page and uses the same terminal and event stream. The legacy workspace strip has been removed. Only servers advertising `auth_ok.tabs` show the workspace tab bar. The tab bar appears only inside an open workspace; the Workspaces page has no Tabs dropdown or all-workspaces tab menu.

ACP remains a first-class backend. The New workspace sheet still creates an ACP workspace with the Terminal/Agent view choice; it is shown as a single-agent workspace with one tab for now. The ACP view itself is unchanged.

These behaviours are tested in a real browser (`packages/client/e2e/`), including two real server processes with real tmux. The browser runs emulate a phone; the soft keyboard, safe areas and gestures on a real iPhone are checked by hand on a device.
