---
id: 0049
title: The U14 health sampler costs about 62 microseconds of CPU per 10 s tick and about 36 KiB of RSS, far under the 1% CPU budget
status: active
tags: [server, performance, health]
date: 2026-10-20
kind: environmental
governed-by: 0105
last-verified: 2026-10-20
---

# Health sampling cost

Decision 0105 adds server health (CPU, RAM, disk) sampled every 10 s into a six-hour in-memory ring (`packages/server/src/health.rs`). The step's budget was under 1% of one vCPU. This is what it costs, measured in a container, not on the dev VPS.

**Per tick:** 62 µs of process CPU time (`CLOCK_PROCESS_CPUTIME_ID`, mean of 20,000 `Sampler::sample()` calls, release build, in a container limited to 512 MB and 1 CPU). At one tick per 10 s that is 0.0006% of one vCPU, about 1/1600 of the budget. Recording a sample into the ring takes about 85 ns; building one `health_history` answer (360 points) about 16 µs.

**Whole process, 10 minutes idle, 512 MB / 1 CPU container, musl release builds, with and without the sampler** (the same commit with the one `health.spawn` call commented out; two containers side by side; no client attached; WebRTC off):

| Build | CPU time (utime + stime) | RSS at start / end | Threads |
|---|---|---|---|
| with sampler | 0.56 s (17 + 39 ticks of 10 ms) | 5764 / 5776 kB | 3 |
| without sampler | 0.55 s (12 + 43 ticks) | 5732 / 5740 kB | 2 |

The difference in CPU time (one tick) is below the 10 ms resolution of `/proc/<pid>/stat`, and the 600 s run cannot show the sampler's 6 ms share; the per-tick figure above is the measurement of its cost. RSS grew by about 36 kB, which is the blocking-pool thread the reads run on (one more thread). The ring itself is at most 2160 points, about 40 bytes each (about 85 KB) once six hours have passed.

**What it does not cover:** the dev VPS (this was an 8-core host with a cgroup limit of 1 CPU, so the figures are from this container, not that machine); a connected client (each push is one small JSON message per 10 s per connection); a cgroup v1 host (the v1 reads are tested against fixture files, not a real v1 kernel). In the limited container the sampler found the cgroup v2 limits (`scope: container`), so the RAM figure is against 512 MB, not the host's memory.
