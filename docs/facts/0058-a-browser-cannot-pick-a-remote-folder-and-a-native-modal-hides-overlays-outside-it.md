---
id: 0058
title: A browser cannot pick a folder on the server, and a native modal dialog hides overlays that are not inside it
status: active
kind: observation
governed-by: 0140-remote-folders-are-browsed-through-an-owner-only-home-confined-listdir
tags: [client, mobile, ux, browser-support]
date: 2026-10-28
last-verified: 2026-10-28
---

# A browser cannot pick a remote folder; a native modal hides outside overlays

## What is true

- **Folder pickers are local.** `showDirectoryPicker()` and `<input webkitdirectory>` open the folder dialog of the device the browser runs on. Neither returns an absolute path (only a handle, or `webkitRelativePath` starting at the chosen folder's own name), and iOS Safari has no `showDirectoryPicker`. A workspace directory lives on the server, so neither can name it. The sheet used them from commit `605798e` to `e89424a`; the path it built was `'/' + <folder name>`.
- **The protocol has no directory listing.** Nothing in `packages/core/src/protocol.rs` or `packages/server/src/control.rs` lists directories (checked 2026-10-28). `new_workspace` takes a free `dir` string and the server accepts any absolute path.
- **A native modal is above everything that is not inside it.** `NewWorkspaceSheet` opens with `<dialog>.showModal()`, which puts it in the browser's top layer. `lib/ui/sheet/Sheet.svelte` (bits-ui `Dialog`, no portal, `absolute` `z-[100]`) mounted beside the dialog is painted under it and is inert, whatever its z-index. A picker must be a descendant of the dialog, or be a top-layer element itself.
- **The numbers on the sheet at `e89424a`.** Agents: 6 tiles then "Show all". Directories: `recentDirs` (from currently open windows, most used first, limit 6) shown twice, as a select and as chips.
