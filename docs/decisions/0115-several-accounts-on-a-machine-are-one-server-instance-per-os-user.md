---
id: 0115-several-accounts-on-a-machine-are-one-server-instance-per-os-user
title: Several accounts on one machine are served by one server instance per OS user, never by one multi-account server
status: active
date: 2026-10-17
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [security, auth, deployment, server]
---

## Decision

Refines 0053 (it stays true: a server has exactly one owner) and builds on 0113.

1. **One instance per OS user.** To let several people use one machine, each runs the `host-me` install (0113) as their own Linux user. Each instance has its own server key, server id, owner, state directory, UDP port, tmux and sessions. There is no member list, no per-account session table and no multi-account mode in the server.
2. **Visibility follows ownership.** An account sees only the instances it owns. Within an instance, sessions belong to that instance's owner alone, until shared. Sharing is the guest grant of 0094 (a scoped, expiring grant on one session) and stays the only way another account sees a session. Sharing is not built yet; the model leaves room for it.
3. **The OS is the boundary.** One user's instance cannot read another's files, tmux or keys, because they are different Unix users. No protocol-level isolation between accounts is built or promised.
4. **Root is needed only to set up, never to run.** Creating the Linux account, installing tmux when missing (until R23 bundles it), opening the firewall (once per machine with the reserved range of 0118) and lingering when the host denies it are done by the machine's admin, once. No sudoers rule, setuid binary or root socket is added for any server (0077).
5. **Memory.** Each instance is a separate process, so its idle footprint is the cost of every extra account. There is no hard limit per instance and no `MemoryMax`; the target is as low as possible. The plan measures idle RSS with 1, 3 and 5 instances, trims what shows up, and reports the numbers. The 300 MB server figure in `resource-constraints` was written for one server and is not a per-instance budget.
6. **Not chosen.** One server serving several accounts (an allowlist or invited members, per-account OS users created by the server, an in-app root helper that creates users). It needs either a weak boundary (everyone shares one shell user) or root in the server, and it adds a member model to the one component that must stay small.

## Why

A shell on the machine is the product, so the only isolation worth promising is the operating system's. Reusing the install that already exists (0113) gives that for free, keeps 0053's single-owner check as the one access rule, and adds no privileged code. The cost is one server card and one process per account, which is acceptable at the sizes this targets.
