---
id: 0116-an-account-is-a-provider-and-an-id
title: An account is a provider and an id, so a second OAuth provider can be added without confusing two users
status: active
date: 2026-10-17
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [security, auth, protocol]
---

## Decision

Amends 0053, 0062 and 0094, which key everything on a bare GitHub id.

- `Identity` becomes `{ provider, id, login }`. `provider` is a short lowercase name (`github` today); `id` is a string, because other providers do not use numbers.
- The device certificate (0062) carries `provider` and `id`. The owner check compares both fields. `owner.json`, `shares.json` (0094) and audit entries store and log both.
- Files and certificates without a provider are read as `github`, so installed servers and signed certificates keep working with no migration step. Servers keep accepting the old certificate form until its 7-day expiry has passed everywhere.
- Only GitHub is implemented. Adding a provider later is a login flow on the hosted function plus a new `provider` value; the server logic does not change.

## Why

With a bare numeric id, a GitHub user `12345` and a GitLab user `12345` would be the same owner. Fixing that after a second provider ships means changing the certificate, the server and its files together; doing it now costs a field.
