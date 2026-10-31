---
id: 0165-native-wrappers-ship-no-self-updater-until-the-owner-opts-in
title: The native wrappers ship no self-updater, and nothing publishes a release, until the owner opts in and updates are signature-checked
status: draft
date: 2026-11-01
track: process
tags: [distribution, security, desktop, platform]
---

## Decision

- **No self-update path in a wrapper.** The Electron wrapper does not depend on `electron-updater` or poll a release feed, and `electron:make` builds with `-p never`. Mobile wrappers update through the stores.
- **An updater is a later, owner-opted decision.** It may be added only with signed installers and a signature check before install, and it is off until the owner turns it on.
- **Why it follows from releases-are-signed.** That guardrail forbids unattended updates without opt-in and without verification. `electron-updater` against a GitHub provider does both on every launch, and the installers are unsigned until R76 step 5.

## Rejected

| Option | Why not |
|---|---|
| Keep the template's `checkForUpdatesAndNotify()` | Polls a release feed from every packaged app, with no signature check |
| Publish on `electron:make` (`-p always`) | A local build could upload an unsigned installer |

## Consequences

- Desktop users update by installing a new signed build, until a later decision adds an opt-in updater.
- R76 step 5 (signing) must precede any updater work.
