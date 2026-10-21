---
id: 0124-connection-telemetry-is-opt-out-sampled-and-enum-only
title: The hosted client reports connection outcomes as sampled, enum-only events through an IP-stripping proxy, opt-out
status: draft
track: product
tags: [privacy, network, client, performance, roadmap]
date: 2026-10-21
---

## Decision

Deferred: recorded now so the connectivity work (0123) can be measured later; nothing ships until the open items below are closed.

- **Who reports.** Only the hosted client. The self-hosted client and the server are off by default, so `paranoid-mode` and air-gapped installs send nothing.
- **Opt-out, with a notice.** On by default for the hosted client, with a plain first-run notice, a settings toggle, and off when `DO_NOT_TRACK` or Global Privacy Control is set. Opt-in would sample only enthusiasts.
- **Payload.** One batched event per sampled session: server and client topology (S/C ids from 0123), paths tried and failed with a reason enum, winning rung, NAT type, bucketed time to connect, platform, version, `sample_rate`. Enums and buckets only: no address, port, fingerprint, server or user id, or free text. No persistent id.
- **Sampling.** About 5-10% of sessions, chosen by a per-session random draw; every failure is sent. Directional rates need thousands of events, not all of them.
- **Route.** Client to a Pages Function that discards the source IP and forwards to Honeycomb (free: 20M events and 100M metric points per month). D1 is not used, to leave its free-tier budget to the directory. Telemetry never delays or blocks a connection.
- **Sink swap.** The sink URL is one config value, so a vendor change is a config change.

## Why

The ladder in 0123 has rungs whose value depends on numbers nobody has: how many users hit CGNAT, how often hole punching works, how iOS Safari behaves. Opt-out is the only way to get a sample worth reading.

## Open before shipping

- Consent for EU users: an IP is personal data and ePrivacy may require prior consent for non-essential reads; get qualified advice. Fallback: default off, or an opt-in prompt, in EU regions.
- Honeycomb: free-plan behaviour at the monthly cap, retention, and whether its terms allow this proxy pattern.
- Request budget: the proxy shares the 100k requests per day (FACT-0018); sampling must keep it a few percent.
- A guardrail, once accepted: the event type has enum and bucket fields only, enforced by a schema test.

## Risks

Trust cost with a privacy-minded audience (mitigated by a public schema and a "view what is sent" screen); opt-out skews toward the hosted client's users; stats are directional only and never feed security or billing.
