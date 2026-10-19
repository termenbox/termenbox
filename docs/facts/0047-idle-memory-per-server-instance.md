---
id: 0047
title: One termenbox-server instance idles at about 8.3 MiB RSS and about 8.7 MiB with three sessions, so five instances cost about 41 to 44 MiB
status: active
tags: [deployment, server, performance]
date: 2026-10-19
kind: environmental
governed-by: 0115
last-verified: 2026-10-19
---

# Idle memory per server instance

Decision 0115 allows one server instance per OS user with no hard memory cap, "as low as possible". This is what an instance costs today, measured, so later changes have a number to beat. It is the baseline for plan step 1.7 (`docs/plans/multi-account-servers.md`).

**Result** (mean of 3 runs; RSS of the `termenbox-server` process only, from `ps -o rss=`, summed over the instances):

| Instances | Idle total | Idle per instance | With 3 sessions each, total | Per instance |
|---|---|---|---|---|
| 1 | 8.3 MiB | 8.31 MiB | 8.7 MiB | 8.66 MiB |
| 3 | 24.9 MiB | 8.30 MiB | 26.1 MiB | 8.70 MiB |
| 5 | 41.2 MiB | 8.24 MiB | 43.5 MiB | 8.70 MiB |

Cost is linear in the number of instances. The kernel shares the binary's pages between processes, but RSS counts them in each, so the sum overstates real use a little. Three sessions add about 0.4 MiB per instance. Five instances use about 15% of the 300 MB server budget in `resource-constraints` and about 8% of the 512 MB machine, before tmux and the agents themselves. Each instance had 9 threads idle (10 with a client attached) on an 8-core host.

**How it was measured:** the release build the installer ships (`cargo build --release --locked -p termenbox-server --target x86_64-unknown-linux-musl` with `-C target-feature=+crt-static`, no `[profile.release]` settings, unstripped 18 MB binary) from the dev container (`scripts/dev.sh`, 8 cores, 24 GB host, tmux 3.3). Each instance got its own `TERMENBOX_CONFIG_DIR`, `HOME`, `TMUX_TMPDIR` and TCP `TERMENBOX_PORT`, and no `TERMENBOX_UDP_PORT`, so they took 4443, 4444, ... from the reserved range (decision 0118). RSS was read 8 seconds after `/health` answered (idle). Then one WebSocket client per instance claimed it, created three tmux sessions with a shell, attached to the first and resized it to 120x30, and RSS was read 8 seconds later.

**What it does not cover:** tmux's own processes and the shells in the sessions (separate processes, per user); a client streaming heavy output, which grows buffers; WebRTC connections (the clients used WebSocket); a 1 vCPU host, where the tokio runtime would start one worker instead of eight. Absolute figures move by a few hundred KiB between runs (spread within one row about 0.4 MiB); differences smaller than that are not a result.

The earlier reading in fact 0046 (8 MB and 12 MB for two unknown processes) agrees with this order of magnitude.
