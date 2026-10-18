---
id: 0053-single-owner-servers
title: A server belongs to one GitHub user
status: active
date: 2026-10-11
tags: [security, auth, roadmap]
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
---

## Decision

Registering a server binds it to the registering GitHub user id; every later connection from another user is rejected and logged (no secrets, per audit-logging-no-secrets). Transport security is Cloudflare TLS; app-layer end-to-end encryption is deferred (0051).

## Why

Without an owner check any GitHub user with a valid token could attach to a shell on the VPS.

Refined by [[0094-session-guests-get-a-scoped-expiring-grant-not-ownership]]: a guest holds a scoped, expiring grant on one session; the server still has exactly one owner.

Refined by [[0115-several-accounts-on-a-machine-are-one-server-instance-per-os-user]] (several accounts are several instances), [[0116-an-account-is-a-provider-and-an-id]] (the owner is a provider and an id) and [[0117-a-claim-link-is-previewed-and-consumed-only-on-confirm]] (preview before the claim binds).
