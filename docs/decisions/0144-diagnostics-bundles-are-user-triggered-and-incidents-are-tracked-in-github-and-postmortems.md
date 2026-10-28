---
id: 0144-diagnostics-bundles-are-user-triggered-and-incidents-are-tracked-in-github-and-postmortems
title: Diagnostics bundles are user-triggered, and incidents are tracked in GitHub issues and postmortems
status: active
date: 2026-10-28
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [server, observability, privacy, client]
fitness-functions: [a bundle test with realistic secrets, addresses and ACP text finds none in the output; the client shows the bundle before download]
---

## Decision

- **Bundle.** The app asks the server for a diagnostics bundle: recent log lines (last 2 MB), the newest crash file, the unclean-exit report, versions and channel, and the health history. The server scrubs it with the problem-report rules (guardrail `diagnostics-never-include-secrets`, extended). The client shows it, then the user downloads it or attaches it to a GitHub issue. Nothing is sent by the server on its own and no new service exists (`usage-stays-on-device`, `no-paid-services`).
- **Prompt after a crash.** When the server reports `previous run ended uncleanly`, the app says so once and offers: build the bundle, open a prefilled GitHub issue labelled `incident`.
- **Issue template.** `.github/ISSUE_TEMPLATE/incident.yml` asks what the user was doing, the version, and for the bundle.
- **Postmortems.** Each real incident gets `docs/incidents/<date>-<slug>.md`: timeline, cause (or "unknown, and what evidence was missing"), what changed, and links to the facts, decisions and guardrails it produced. Procedure: `investigate-a-server-incident`.
- **No telemetry.** An opt-in crash ping to the directory was considered and not chosen now.

## Why

Without a way to get evidence from a user's server, an incident like 2026-10-28 stays unknown. Making it user-triggered keeps the privacy line the project already holds.

## Rejected

| Option | Why not |
|---|---|
| Opt-in crash ping | Needs a new directory endpoint and storage; revisit if issues are too few to see crash rates. |
| Automatic upload | Conflicts with `usage-stays-on-device` and needs storage. |
