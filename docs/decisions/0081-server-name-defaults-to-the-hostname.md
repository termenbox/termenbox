---
id: 0081-server-name-defaults-to-the-hostname
title: A server's display name defaults to the host's short hostname, and a name the owner types wins
status: active
accepted-by: Sergey Strebulaev <s@vivantel.dev>
date: 2026-10-13
track: product
tags: [server, client, deployment]
---

## Decision (draft: needs the owner's acceptance)

- Without `TERMENBOX_SERVER_NAME` the server names itself with the short hostname (`vps-1` for `vps-1.example.com`), cleaned to letters, digits, `-` and `_`, at most 40 characters; `termenbox-server` when it is empty or `localhost`.
- The container installer passes the host's hostname, because inside a container the hostname is the container id.
- Add Server leaves the name empty; a name the owner types is kept, otherwise the name the server reports on first connect is stored.
- Not `user@hostname`: the user is already shown by the mode badge, and the name is what the owner types to confirm a remote uninstall (0072), so it stays short.

## Why

A name the owner did not type should still tell their servers apart. The hostname is the one name every VPS already has and the owner already recognises.

Refined by [[0117-a-claim-link-is-previewed-and-consumed-only-on-confirm]]: the typed name is entered in the Add Server confirmation, pre-filled with this default.
