---
id: listdir-is-owner-only-home-confined-and-directories-only
title: Directory listing for the folder picker is owner-only, confined to the home directory, and returns directories only
status: active
governed-by: 0140-remote-folders-are-browsed-through-an-owner-only-home-confined-listdir
grounded-in: [0140-remote-folders-are-browsed-through-an-owner-only-home-confined-listdir, 0058, audit-logging-no-secrets]
derivation-note: A request that shows part of the server's filesystem to a browser (0140) must reveal no more than the picker needs, to no one but the owner, and leave a trace that holds no content.
tags: [security, protocol, server]
---

# Guardrail: list_dir shows only the owner's directories under home

## Rule
The server **MUST** answer `list_dir` only for the owner, and **MUST** refuse a session guest.
The server **MUST** canonicalise the requested path and refuse one outside the server user's `$HOME`; a symlink leaving `$HOME` **MUST NOT** be followed.
The reply **MUST** carry directory names only: no file names, sizes, times or contents; at most 500 entries, with a `truncated` flag.
The server **MUST** log each call (time, key fingerprint, hash of the path) and **MUST NOT** log the entries.
Creating a workspace **MUST NOT** depend on `list_dir` succeeding.

## Checked by
Server tests: a guest is refused; `..` and an absolute path outside home are refused; a symlink out of home is listed but not followed; a directory with a file in it lists no file; 501 entries give `truncated`.
