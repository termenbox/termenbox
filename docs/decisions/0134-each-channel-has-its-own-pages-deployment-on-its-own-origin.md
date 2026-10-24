---
id: 0134-each-channel-has-its-own-pages-deployment-on-its-own-origin
title: Each release channel has its own Cloudflare Pages deployment on its own origin, deployed by its release workflow, and its installer trusts only its keys
status: draft
track: product
tags: [release, ci, cloud, client]
date: 2026-10-23
---

## Decision

- **Three origins, three projects.** stable `app.termenbox.dev` (Pages project `termenbox`), beta `beta.app.termenbox.dev` (`termenbox-beta`), nightly `nightly.app.termenbox.dev` (`termenbox-nightly`). Never a `/beta/` path: a PWA service worker, its caches, localStorage and the OAuth callback are bound to an origin, and an experimental build must not share them with stable.
- **A release deploys its own client.** `deploy-pages.yaml` is reusable and takes a channel; `release-stable.yaml`, `release-beta.yaml` and `release-nightly.yaml` call it after they publish (decision 0133). A push to `master` or `develop` deploys nothing, so stable's client moves only with a stable tag. A Pages Functions fix therefore waits for a patch release, which makes D1 migrations additive-only; beta and nightly apply them first.
- **One build variable.** `VITE_CHANNEL` (stable, beta, nightly) selects the channel; the build also defines `VITE_API_ORIGIN` and `VITE_APP_VERSION` (a nightly shows its stamped version).
- **The installer is baked per channel.** `/i` defaults to the channel of the app that serves it and carries only the release keys that channel trusts: stable `r1`; beta `r1` and `b1` (the beta alias can hold a stable release); nightly `n1` (decision 0130). `--channel` still works, but a copy without the matching key says so instead of failing the signature.
- **Shared and separate.** All three share one D1 database, `CERT_SIGNING_KEY` and `DIRECTORY_SALT`. Each GitHub Environment (`stable`, `beta`, `nightly`) holds its own GitHub OAuth app, because a callback is per origin.

## Why

A beta or nightly server needs a matching client, and sharing one origin would let an experimental service worker or cached bundle break stable users. Deploying from the release workflow instead of from a branch ties the client to the same gate, key and environment as the server it pairs with. Trusting only a channel's own keys in its installer keeps a leaked nightly key from signing anything a stable installer accepts.

## Consequences

- Owner work: create the two OAuth apps and put each pair into its Environment, set `TERMENBOX_CLIENT_ORIGIN` per Environment, run `deploy-infra.yaml` (apply) to create the two projects and DNS records, then run a beta and a nightly release. Until the apply, a beta or nightly deploy fails at the Pages step.
- Stable's client no longer updates on a merge; it updates on the next stable tag.
- The Cloudflare free budget (fact 0022) is shared by the three projects; the request-budget alarm (R07) is not yet in place.
