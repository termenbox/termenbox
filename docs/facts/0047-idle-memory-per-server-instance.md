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

Decision 0115 allows one server instance per OS user with no hard memory cap, "as low as possible". This is what an instance costs today, measured, so later changes have a number to beat. It is the baseline for plan step 1.7 (`docs/plans/archive/multi-account-servers.md`).

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

## Trims tried

Each was built and measured on its own against the table above (same method, 3 runs each, totals over the instances; idle / with 3 sessions).

| Trim | 1 instance | 3 instances | 5 instances | Verdict |
|---|---|---|---|---|
| Baseline (no `[profile.release]`) | 8.3 / 8.7 MiB | 24.9 / 26.1 | 41.2 / 43.5 | |
| Release profile: `opt-level = "s"`, `lto`, `codegen-units = 1`, `strip` | 6.2 / 6.6 | 18.8 / 19.6 | 31.5 / 33.1 | **kept**, PR #204; about 25% less; binary 18.1 to 8.5 MB; keystroke echo p50 about 2 to 3 ms before and after |
| Release profile plus `panic = "abort"` | 6.0 / 6.3 | 17.6 / 18.6 | 29.7 / 31.4 | rejected: 0.2 to 0.3 MiB more, but one panicking task would end the whole server and every client on it |
| Tokio runtime capped at 2 worker threads | 8.4 / 8.6 | 25.5 / 26.1 | 42.3 / 43.3 | rejected: threads fall from 9 to 3 but RSS does not move (idle thread stacks are never touched); on a 1 vCPU host the runtime already starts one worker |
| mimalloc as global allocator | 19.7 / 22.6 | 58.1 / 67.3 | 96.5 / 114.3 | rejected: more than double, its per-thread heaps are reserved up front |
| jemalloc (`tikv-jemallocator`) with `narenas:1`, `dirty_decay_ms:0`, `muzzy_decay_ms:0` | 8.2 / 9.1 | 25.1 / 27.5 | 41.9 / 45.7 | rejected: no gain idle, 0.4 MiB worse with sessions; also checked by setting the same options through `_RJEM_MALLOC_CONF`, same numbers |
| Hard caps on scrollback and output buffers | not built | | | rejected: nothing to cap. The server keeps no scrollback (tmux does), and its channels are already bounded (the PTY channel holds 64 chunks, a WebRTC message is at most 64 KiB, agent events at most 30 entries per session). A client attached to a pane printing `seq 1 3000000` raised peak RSS from 8.9 to 9.2 MiB |

The jemalloc and mimalloc builds used the unprofiled build, so they are compared with the baseline row, not the profile row. Allocator, thread cap and the profile were not combined; the profile is the only change that moved the number.
