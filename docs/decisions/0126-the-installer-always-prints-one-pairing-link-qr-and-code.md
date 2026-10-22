---
id: 0126-the-installer-always-prints-one-pairing-link-qr-and-code
title: The installer prints one pairing link, QR and short code on every run, carrying the claim and the signed candidates
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [onboarding, lan, client, server, protocol]
date: 2026-10-22
---

## Decision

Refines 0117, 0123 and 0056.

- The installer ends every run, first install and re-run, with the same block: a terminal **QR**, the **link** it encodes, and a short **code** (0128). The command carries no secret (0127): the host mints the claim, and the block is how the browser learns of the server. A server no longer re-announces itself to the directory every 5 minutes while unclaimed; it announces once and then on change or every 7 days.
- The link is `https://<client>/#/pair/<payload>`. The payload holds the one-time claim token and the signed candidate bundle (`lan`, `wan4`, `wan6`, …). One scan finds the server and authorises the claim, with or without internet or directory. The fragment never reaches a host.
- The claim rules of 0117 are unchanged: 15 minutes, previewed first, consumed only on confirm.
- Re-running the installer on an owned server prints a re-add link with the bundle and no claim token.
- A LAN-only server (no directory path) is added by this link or code alone; waiting for a directory announcement is not the only way a browser learns of a server.

## Why

The default one-liner printed nothing to act on, and the QR lived behind a second command, so a user on a LAN with no public address waited 15 minutes for an announcement that could not arrive. The claim and the addresses were two links; a LAN user needed both. One printed block that always works removes the choice.

Not chosen: printing the QR only when no public path exists. It adds a second behaviour to explain and test, and a public server could not be re-added by QR.

## Consequences

- Extends `--register` and `--print-pairing` to one output; the installer prints it for container and native modes.
- The installer takes the claim only from `TERMENBOX_CLAIM` (tests and automation), not from the command line.
- Amends the scope note in `bootstrap-simplicity`: printed output is optional for the paste path.
- Plan: R65.
