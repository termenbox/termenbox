---
id: uploads-are-owner-only-pinned-home-confined-and-never-executable
title: An upload is owner-only, lands in the pinned working folder under home, never executes or overwrites without a choice, and leaves a content-free trace
status: active
governed-by: 0194-a-file-is-uploaded-to-the-active-tabs-working-folder-pinned-at-start-resumable-and-owner-only
grounded-in: [0194-a-file-is-uploaded-to-the-active-tabs-working-folder-pinned-at-start-resumable-and-owner-only, 0147-remote-folders-are-browsed-through-an-owner-only-home-confined-listdir, listdir-is-owner-only-home-confined-and-directories-only, audit-logging-no-secrets, resource-constraints]
derivation-note: A request that writes a file from a browser onto the server's disk (0194) is stronger than reading a listing (0147), so it must reach no one but the owner, no place but the folder the owner was shown, create nothing that can run, and leave a trace without content.
tags: [security, protocol, server]
---

# Guardrail: an upload writes only where the owner was shown, and nothing that can run

## Rule
The server **MUST** accept `upload_start` only from the owner, and **MUST** refuse a session guest.
The destination **MUST** be resolved by the server from the named tab, canonicalised, and **MUST** be `$HOME` or below; a symlink leaving `$HOME` **MUST NOT** be followed. The client **MUST NOT** supply a path.
The resolved folder **MUST** be fixed at `upload_start`; a later tab switch or `cd` **MUST NOT** change where the file lands.
The server **MUST** refuse a file name that is empty, over 255 bytes, `.`, `..`, dot-leading, or that contains `/`, `\`, NUL, a control character or leading or trailing whitespace, and **MUST NOT** rewrite it.
The server **MUST** create the file with mode `0600` by the run user, never executable and never setuid, on a temporary file it moves into place at finish; it **MUST NOT** overwrite an existing name unless the user chose Replace, and **MUST NOT** write through a symlink.
The server **MUST** hold at most one window of file bytes in memory and **MUST** refuse or fail an upload that would leave free disk below the floor.
The server **MUST** delete a partial file on cancel, on error and when the resume grace period ends.
The server **MUST** log each upload event (time, key fingerprint, hash of the folder, hash of the name, size) and **MUST NOT** log the name, the path or any content.

## Checked by
Server tests: a guest is refused; a path supplied by the client is ignored; `..` and a symlink out of home are refused; a name with `/`, a dot, a control character or 256 bytes is refused; the created file has mode `0600`; an existing name is never overwritten without Replace; a `cd` after start does not move the file; a partial is gone after cancel and after the grace period; a start below the disk floor is refused; the audit lines contain no name, path or content.
