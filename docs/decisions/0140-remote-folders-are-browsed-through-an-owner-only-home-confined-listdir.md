---
id: 0140-remote-folders-are-browsed-through-an-owner-only-home-confined-listdir
title: Remote folders are browsed through a new owner-only, directories-only list_dir request confined to the server user's home
status: active
date: 2026-10-28
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [protocol, security, client, ux]
---

## Decision

- **A new `list_dir` request.** Given an absolute path (or none, meaning `$HOME`) the server replies with the directories directly inside it: name, whether it is a symlink, whether it is hidden. Directories only, never file names or contents. Capped at 500 entries per reply with a `truncated` flag; the client filters, and may pass a `prefix` so the server filters first.
- **Owner only.** Session guests (decision 0094) are refused. One owner per server (0053) already holds a shell, so the owner learns nothing new.
- **Confined to `$HOME`.** The path is canonicalised and must be `$HOME` or below it; a symlink that leaves `$HOME` is listed as a symlink and not followed. Anything else is a refusal with a short reason, not an empty list.
- **Hidden directories are off** unless the request asks for them (a toggle in the picker).
- **Creating is not confined.** `new_workspace` still accepts any absolute path, so a typed `/srv/work` or a recent directory outside `$HOME` keeps working. The picker's path bar checks a typed path with `list_dir` when it is under `$HOME`, and otherwise accepts it as typed.
- **Audited, content-free.** Each call logs time, key fingerprint and a hash of the requested path, never entries (`audit-logging-no-secrets`).
- **No third-party picker.** The data arrives over our signed channel, not HTTP; the picker is our own component on the existing sheet primitives.

## Why

Choosing a folder on the server is the main job of the New workspace dialog, and the browser cannot do it (fact 0058). Typing `/home/you/project` on a phone keyboard is slow and error-prone. Browsing is the only way to pick a folder never used before. The surface is a view of the server's filesystem, so the scope is the narrowest that still serves the common case: directories under the user's home.

Not chosen: confining to nothing (a wider read surface than creating needs); owner and guests (needs a per-grant scope model that does not exist); path autocomplete only (poor discovery, fiddly on a phone); recents only (cannot reach a new folder).

## Consequences

- Core gains a request and reply type; the server a handler with canonicalisation tests; the client a picker (plan `new-workspace-dialog-ux`).
- A workspace outside `$HOME` cannot be reached by browsing; the owner types its path. Revisit if that proves common.
- The reply is a new unknown-kind surface for older clients: they simply never send it.
