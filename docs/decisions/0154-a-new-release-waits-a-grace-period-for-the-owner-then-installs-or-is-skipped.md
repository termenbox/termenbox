---
id: 0154-a-new-release-waits-a-grace-period-for-the-owner-then-installs-or-is-skipped
title: A new release waits a grace period for the owner, then installs or is skipped as the host configured
status: active
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [release, updater, notifications]
date: 2026-10-31
---

## Decision

- **Grace under `notify`.** `[update]` gains two optional keys, valid with `mode = "notify"`: `grace = "12h"` and `on_timeout = "install|skip"` (default `skip`). No new mode is added. When a release is first seen, its deadline is `first_seen + grace`. At the deadline the updater does what `on_timeout` says; before it, the owner may answer. Without `grace`, `notify` behaves as decision 0131 describes.
- **The owner answers per release, from the app.** *Update now*, *Postpone* (one more grace, at most three times) and *Skip this version* are per-release state, sent as request files with fixed actions (`update-now`, `update-postpone`, `update-skip`), under the owner checks of decision 0077. The policy itself (`channel`, `mode`, `grace`, `on_timeout`, `maintenance_window`) stays host-only; the app cannot write `update.toml`, as decision 0131 says.
- **Check timing.** The updater checks at boot and every 6 hours with a random delay of at most 15 minutes, and a notice stays valid until a newer release replaces it or it is installed (no one-hour expiry). A separate local tick every 5 minutes makes no network call and only acts when a deadline has passed.
- **Deadlines respect the window.** A deadline outside `maintenance_window` moves to the next window start (across midnight too). An owner's *Update now* ignores the window, as in decision 0131.
- **A major version never installs by itself**, not even at the deadline; it stays available until the owner confirms.
- **A failed release is not retried.** After a rollback or a failed install the version is pinned as skipped until a newer release appears or the owner asks again.
- **Rollout order.** A release whose parser accepts and ignores `grace` and `on_timeout` ships first; only a later release lets the installer or the docs write them, so a server rolled back to an older binary never meets keys it refuses.

## Why

The daily timer fired between 00:00 and 01:00, so a window such as 03:00-04:00 was never reached; the notice expired after an hour while the check ran daily; and an owner had no way to say "not now" or to let a default act while away. A grace period lets the owner decide when present and lets the host choose the fallback when not, with the verification, journal and rollback rules of decisions 0063, 0076 and 0077 unchanged.

Not chosen: a fourth mode `ask` (a second way to say `notify`); changing the policy from the app (a web session would decide what runs as root, decision 0131); an hourly network poll to honour deadlines (the local tick does it without traffic).

## Consequences

- Refines decision 0131 (policy and window) and decision 0076 (the timer); decisions 0063 and 0077 are unchanged.
- Container installs have no updater yet; that is a separate decision, not covered here.
- Plan: `docs/plans/server-update-flow.md`.
