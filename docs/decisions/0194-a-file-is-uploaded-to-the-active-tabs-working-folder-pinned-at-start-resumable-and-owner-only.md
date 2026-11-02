---
id: 0194-a-file-is-uploaded-to-the-active-tabs-working-folder-pinned-at-start-resumable-and-owner-only
title: A file is uploaded from the device to the active tab's working folder, pinned at the start, resumable, owner-only and bounded by free disk
status: draft
date: 2026-11-02
track: product
accepted-by: -
tags: [protocol, security, client, ux]
---

## Decision

Termenbox copies files from the device in use to the server host, into the working folder of the active workspace tab. This is a file copy, not R39 (handing a dropped image or file to the agent); the two share one confirm sheet.

- **Destination: the tab's live working folder.** The client names a workspace and tab, never a path. The server reads the tab's current folder (tmux `pane_current_path`), canonicalises it, and refuses unless it is `$HOME` or below (the `list_dir` line, decision 0147). A symlink leaving `$HOME` is refused. Dot folders below `$HOME` are allowed: an agent may work in one, and the owner chose the tab. A tab whose folder is outside `$HOME`, or gone, is refused with a short reason.
- **Pinned at the start.** The folder is resolved and canonicalised once, at `upload_start`, and kept for the whole upload. Switching tabs or a `cd` in the shell afterwards changes nothing. The sheet and the result show the exact folder.
- **Inputs.** A file picker (several files), drag-and-drop, and clipboard paste of a file or image. Plain text paste still goes to the terminal, bracketed paste intact. A drop or file paste opens one confirm sheet showing the tab, the folder and the files; nothing is written before the user confirms. R39 later adds "Send to agent" to the same sheet, shown only when the agent supports it. The picker is the keyboard and phone path.
- **Name collisions.** Per file the user chooses Keep both (the default; the server picks `name (1).ext` at finish and never overwrites), Replace, or Skip, with a "same action for the rest" tick. Replace is an explicit tap.
- **No size cap, a free-disk guard.** There is no per-file limit. The server refuses a start when the free space left after the declared size would be below a floor (1 GiB), and fails an upload that would cross it. Raising or adding a cap is a new decision.
- **Resumable.** The server keeps the partial file and its pinned folder for a grace period (10 minutes) after the connection ends, and the client continues from the server's byte offset with `upload_resume {id}`. After the grace period the partial is deleted. Cancel deletes it at once.
- **Binary frames, a window bounded by bytes.** File bytes travel as binary chunks (no base64), each under the direct-transport message limit, with the in-flight window measured in bytes (1 MiB to begin with, to be measured). The server holds at most one window. The WebSocket fallback carries the same frames. Whether the transports already allow binary frames is checked in the plan's first step.
- **One at a time, bounded.** A connection uploads one file at a time; the client queues the rest. At most 60 `upload_start` per minute and 100 files per batch; beyond that a short refusal.
- **Owner only.** Session guests (decision 0094) are refused at `upload_start`. One owner per server (0053) already holds a shell.
- **File name.** The server never rewrites a name; it refuses one that is empty, over 255 bytes, `.` or `..`, starts with a dot, contains `/`, `\`, NUL or a control character, or has leading or trailing whitespace. The client replaces such characters before it asks, and shows the final name on the sheet.
- **No execution, no mode bits.** The file is created `0600` (never executable, no setuid, ignoring the client's mode), by the run user, with `create_new` on a hidden temporary file in the pinned folder (`.tb-upload-<random>.part`) and moved into place at finish. Nothing is run, opened or scanned.
- **Audited, content-free.** `upload_started`, `upload_refused`, `upload_resumed`, `upload_done`, `upload_cancelled`, `upload_failed` log time, key fingerprint and account, a hash of the folder, a hash of the name and the size; never the name in clear, the path or any content (`audit-logging-no-secrets`).
- **Visible states.** Choosing, confirming, queued, uploading with progress, paused (connection lost, will resume), denied (with the reason), failed (with Retry), cancelled, done (with the final path). Dropping on another UI element must not navigate the page.

## Why

A phone or laptop holds the screenshot, log or document that the project on the server needs, and the active tab's folder is where the user is already working. Resolving the folder on the server means the client cannot name an arbitrary path, and pinning it at the start means the user is shown what will happen and it does not change. Resume and binary frames serve the product's promise of Mosh-level persistence: with no size cap, a 2 GiB upload over a phone's connection must survive a lock screen and a tunnel. The destination rules reuse the one confinement already reviewed for browsing (0147); the sheet keeps this feature and R39 from fighting for the same gesture.

Not chosen: a folder browsed by the owner (the first draft, 0151, never accepted), because the active tab already says where the owner is working; dot folders refused, because agents legitimately work in them; folders anywhere the run user can write, which widens the write surface to the whole host; re-resolving the folder at finish, which can put the file somewhere the user never saw; failing at once on a dropped connection, which makes large files unusable on a phone; base64 text chunks, which cost a third more bytes and a parse per chunk; refusing a name collision, which stalls a batch and breaks pasted screenshots that are all called `image.png`.

## Consequences

- Core gains upload messages and a binary chunk frame; the server a destination resolver, a writer with resume and a disk guard; the client an upload queue, a confirm sheet, drop and paste (plan: `docs/plans/file-upload-to-the-working-folder.md`, roadmap R103).
- A tab working outside `$HOME` cannot receive a file. Revisit if that proves common.
- The server keeps a partial file for up to 10 minutes after a drop, so a flaky link can leave disk used for a while; the free-disk floor bounds the harm.
- With no size cap, one owner upload can use the disk down to the floor and hold the connection for a long time. A stuck upload is visible in the sheet and cancellable.
- Draft: the 1 GiB floor, the 10-minute grace period, the 1 MiB window, 60 starts a minute and 100 files a batch are starting values the owner has not confirmed. The decision stays a draft until accepted.
- Supersedes the never-accepted draft 0151 (folder browsed by the owner, 25 MiB, no resume), which was not committed.
