---
id: 0141-the-new-workspace-dialog-shows-recents-and-puts-the-rest-in-layers-inside-it
title: The New workspace dialog shows three recent agents and a readonly folder with recent chips, and puts the full lists in layers inside the dialog
status: active
amends: 0137-agents-start-in-a-terminal-or-agent-view-chosen-per-agent
date: 2026-10-28
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, mobile, ux]
---

## Decision

- **Agents.** Three most-recent agent tiles plus a fourth "More" tile: a 2 x 2 grid on xs, one row of four from md. With no history the three are claude, codex and shell (filtered to what the server supports). The last-used agent is preselected. "More" opens a layer listing every supported agent, a filter box and a Recent section first.
- **Folder.** A readonly field shows the chosen path (`~/` for the home prefix, middle ellipsis on xs, tail segment emphasised) with a Change button. Two or three recent-folder chips sit above it; tapping one fills the field. "More" on the chips opens a layer with the limited recent list (up to 10) and a filter. Change opens the folder picker (decision 0140): breadcrumb, up and home, filter, hidden toggle, and an editable path bar, which is the only place a path is typed.
- **Recents are per device.** localStorage, per server, written when a workspace is created (not derived from open windows), falling back to memory where storage is off. Existing workspace directories seed an empty list.
- **Layers live inside the dialog.** The dialog is a native `<dialog>` (fact 0058), so its pickers are descendants of it: on xs and sm a bottom sheet over the form with a grabber and a sticky Select; from md the picker replaces the form body in place with a Back header, so the dialog never grows or stacks. Escape closes the top layer first; focus returns to the control that opened it; one focus trap.
- **Layout.** Below 480 px a full-width bottom sheet; above it a centred modal capped near 560 px, one column at every width. A sticky footer reads "Claude Code in ~/code/termenbox" beside Create, so the whole intent is checked in one glance. Name is an auto-suggested editable line under it; Server and Manager selects appear only when there is a real choice; the last-used server is the default.
- **Videos, not photos.** The dialog is documented by short videos of simple use cases (start in a recent folder, pick another agent, browse to a new folder, type a path), rendered by a standalone `workflow_dispatch`-only workflow with human pacing and a visible cursor or touch spot. This is a deliberate exception to the `ux` workflow named in the `render-a-feature-video` procedure, because that workflow (R73) does not exist yet; the recorder is written as a reusable module so R73 can adopt it. Publishing a video to the docs is a separate step on the owner's word.

## Why

Three recents cover most starts with one tap; the long tail needs a list, not a longer grid. A readonly field with a Change button removes the typo state from the main form and lets the person validate the path at a glance. The previous sheet showed six agents, the same directories twice, a Browse button that opened the wrong machine's folders, and pickers that never appeared (fact 0058). Layers inside the dialog make the stacking problem impossible instead of patched.

Not chosen: a separate stacked native dialog (two modals, double backdrop, fiddly Escape and focus); replacing the native dialog everywhere (touches unrelated call sites); an editable input on the form (brings the typo state back); a server-side shared recents list (more protocol for little gain now).

## Consequences

- The `Browse…` button, `webkitdirectory` input and the `__other__` option are removed.
- Phone and laptop keep separate recents.
- A UI change here is not verified until `mobile-ui-device-check` has been run on a real iPhone.
