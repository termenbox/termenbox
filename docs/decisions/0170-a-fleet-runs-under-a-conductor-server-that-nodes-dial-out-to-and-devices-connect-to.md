---
id: 0170-a-fleet-runs-under-a-conductor-server-that-nodes-dial-out-to-and-devices-connect-to
title: A fleet runs under one owner-pinned conductor server; nodes dial out to it and every device holds a single connection to it
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [control-plane, fleet, server, network, architecture]
related: [0053, 0057, 0123, 0141, 0169, 0171]
---

## Decision

- **Agents run on many nodes** the owner provides (laptops, VPSes, spot machines). Each node is an ordinary termenbox server inside the 512 MB / 1 vCPU envelope for its own processes; agent children are sized by the box (`resource-constraints`).
- **One server is pinned as the conductor.** Nodes connect outward to it over the existing signed identity (0123), so a node behind NAT needs no open port. The conductor merges the nodes' attention events, rules, digests and usage into one stream.
- **A device connects to the conductor only.** Today's per-server connection stays for opening a terminal; the conductor proxies a node's terminal on demand, or hands the device a signed path to the node.
- **Links carry attention metadata only**, not terminal streams (`conductor-links-carry-attention-metadata-only`).
- **No paid service.** `no-paid-services` governs termenbox's own required stack. Nodes, model tokens and the optional deputy are the owner's own spend. The control plane never routes through the owner's Cloudflare free tier except for the rendezvous already in 0141, because fleet event volume would exceed it.
- **The conductor is a single point of failure by design in the first version.** A warm standby that takes over from replicated state is a later step in the plan (R88 part 2).

## Why

One WebRTC connection per server fails at around ten servers on a phone, and nothing server-side runs (rules, escalation, digests) while no device is open. A conductor gives one connection per device and a place for the brain.

## Not chosen

- Each device connects to every node (N connections, no server-side brain), and a Cloudflare hub (free quotas, agent metadata in a third party's account).
