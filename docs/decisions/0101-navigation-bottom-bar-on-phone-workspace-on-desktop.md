---
id: 0101-navigation-bottom-bar-on-phone-workspace-on-desktop
title: Phone navigation is a four-destination bottom bar; desktop is a three-pane workspace with a command palette
status: active
date: 2026-10-16
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, mobile, desktop, roadmap]
---

## Decision

- **Destinations** (same on both): Home (0100), Sessions (all servers, grouped), Activity (timeline, digest, usage and cost, server health: 0105), Settings.
- **Phone:** a thumb-reachable bottom bar with those four. A session opens as a full-height sheet with swipe-back. The tab strip and session sheet of 0082 remain as the in-session window and session switcher, not the top-level model. Nothing important lives at the top edge.
- **Desktop:** a sidebar (destinations, servers, sessions with status), a main pane (Home, or the selected session view), and an optional right pane holding the live terminal or a second session. A command palette (Ctrl/Cmd+K) jumps to any session, server or action. Keyboard shortcuts cover approve, deny, next needs-you and focus composer.
- **Workspace strip:** the in-session switcher of 0082 is shown while a workspace is open, including beside the open desktop sidebar, and carries **+ New** on the desktop. On the desktop Workspaces list page it is shown only while the sidebar is collapsed; with the sidebar open the page list and the sidebar already switch, so the strip is not repeated there. On a phone it stays above the page.
- One component set and one data layer serve both; the arrangement differs. Desktop is designed as desktop, not as a stretched phone.

## Why

Mobile must be outstanding, not a degraded desktop, and desktop must not be a big phone. A bottom bar keeps the common targets in reach of one hand; a sidebar and palette serve the desktop user who wants everything visible and keyboard-driven. Builds on [[0082-navigation-tabs-sheet-and-auto-connect]] (kept for the in-session switcher) and [[0100-home-is-a-decision-first-board]].

The strip is where an open workspace is switched. #328 dropped it whenever the desktop sidebar was open, on the grounds that the sidebar lists the workspaces; with the sidebar collapsed the strip is the only workspace chrome left at the top, so the desktop create action lives there as well as on the page. The strip stays beside the sidebar in a workspace and carries **+ New**; on the list page it is not shown beside the open sidebar, where the page list and the sidebar would make it a third list of the same workspaces.
