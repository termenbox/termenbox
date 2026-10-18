---
id: 0117-a-claim-link-is-previewed-and-consumed-only-on-confirm
title: A claim link is previewed first and consumed only when the user confirms, and the name is editable there
status: active
date: 2026-10-17
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [security, auth, client, protocol]
---

## Decision

Refines 0053 and 0056; reconciles 0081.

- The link or QR from the installer is one-time and expires after 15 minutes, as before. Whoever holds it and confirms first becomes the owner of that instance, which is its admin.
- The link's token first authorises one **read-only preview**: the server returns its name, run mode, the OS user sessions run as and its version, and binds nobody. Opening the preview never consumes the token; closing the dialog leaves it valid until it expires.
- The Add Server dialog shows that preview and a **name field**, pre-filled with the server's reported name (the short hostname, 0081). The user may overwrite it before confirming; the typed name is what the client stores and what a remote uninstall asks for (0072).
- Only the confirm step presents the claim, binds the owner and deletes the token. A preview shows no secret, no session and no path, and is audit-logged without the token.
- Re-claiming an owned server stays root-proven (the installer run again with a new secret).

## Why

The old path bound the owner on the first authenticated connection, so the dialog could not say what the user was adding and the name could not be chosen first. Splitting preview from claim lets the user see what they are about to own, and a closed dialog does not burn the link. The name stays short and typed by the owner, which keeps 0081's reason for rejecting `user@host`.

Not chosen: locking the token to whoever previews first. It stops a seen link from being confirmed by someone else after the owner opened it, but a stranger who opens it first locks the owner out.
