---
id: 0153-the-tab-strip-follows-tmuxs-current-window-on-a-control-mode-server
title: The tab strip follows tmux's current window on a control-mode server
status: active
date: 2026-10-30
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
amends: [0149]
tags: [workspaces, tabs, client, server]
---

## Decision

- **tmux's current window is the workspace focus.** With control mode (tmux 3.2+), the server writes the tab of the window tmux has current into the registry as `workspace.focus` whenever it moves, and sends the fresh workspace list to every connection. `window_list.active` and `representative` name the same tab.
- **A device viewing the workspace follows that focus.** The desktop strip and the phone plaque follow the shared current window without re-attaching the terminal: the attached tmux client already shows the new window, so only the selection moves. A device viewing another workspace is unaffected.
- **Opening a tab is a focus move too.** Attaching a tab makes it tmux's current window, and every other device viewing the workspace follows it.
- **A server without control mode keeps 0149's per-device behaviour.** An older server, or a tmux older than 3.2, reports no current window, so the focus stays where the server last set it and the strip does not move by itself.
- **A new device opens the tab tmux has active**, because the focus is the server's default on attach.

## Why

The top bar disagreed with the terminal: `Ctrl+B <n>` or `tmux select-window` changed the window but not the tab strip, because the server never wrote the active window into `focus` and the client preferred its own remembered tab. tmux keeps one current window per session and every attached device shares it, so the strip must show it too; per-device grouped sessions stay out of scope.

## Consequences

- Issue #375 and step 2 of `docs/plans/workspace-tabs.md`.
- On a control-mode server a device opens the tab tmux has current, not its remembered one (`termenbox_active_tab`), so a reload never drags the other devices to a stale tab. Where the server has no control mode, the remembered tab stays in charge. Per-device focus would need one grouped tmux session per device and is not planned.
- The foreground report follows the moved tab, so a push is not sent for the window the user is looking at.
- Tab order and tmux window order are one order (issue #376): a reorder moves the windows first, so a failed move leaves both as they were, and closing a tab packs the window numbers. This holds for any tmux, with or without control mode.
