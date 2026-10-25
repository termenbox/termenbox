---
id: 0138-control-bytes-and-events-travel-on-separate-channels-and-events-replay-from-a-sequence-log
title: Control, terminal bytes and agent events travel on separate channels, and events replay from a sequence log
status: active
amends: 0096-terminal-and-status-use-two-data-channels
date: 2026-10-25
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [protocol, transport, performance, architecture]
---

## Decision

- **Three logical channels.** (1) **Control**: small, ordered, reliable. It carries session cancel, permission answers, prompt submit, interrupt and kill, focus, window create, close and resize, and workspace administration. Every control message gets an acknowledgement with a result. (2) **Terminal bytes**: raw bytes of any PTY window, addressed by `windowId`, as today (0096). A PTY agent window has both its conversation stream and its bytes, linked by `windowId`, so the owner can switch to the raw TUI when an adapter is wrong. (3) **Agent events**: the protocol's `session/update` stream and requests (0136), multiplexed by ACP `sessionId`.
- **Control is never queued behind bulk.** On WebRTC, control has its own data channel. On the WebSocket fallback (one TCP stream, no transport priority) the sender drains a control queue before event and byte frames, and bulk frames have a bounded size, so a cancel waits at most one frame. This holds on both transports (guardrail `control-never-waits-behind-bulk`).
- **Events replay from a sequence log.** Every outbound event carries a per-session `seq` and the server `epoch` in an extension. A client reconnects with "resume from seq N" and the server replays the tail from a bounded ring. If N has fallen off the ring or the epoch changed, the server sends a **compacted replay**: standard `session/update` notifications that reconstruct current state (chunks merged, tool calls collapsed), flagged `reset`. The stream stays valid ACP throughout. Ring size is a **budget** (bytes per window, configurable, default set by measurement), not a hard fit into 512 MB.
- **Budgets, not a cap.** 512 MB and 1 vCPU are the reference envelope for termenbox's own processes, and CI measures budgets per feature (server RSS per idle and active window, replay encode time, fan-out CPU). The product does not promise to fit any workload into 512 MB; agent children are sized by the owner (plan `acp-session-modes`, choice F). Guardrail `resource-constraints` is amended to say so.

## Why

A flood of tool output must not delay a cancel or a permission answer on a slow phone link; that is the failure the control channel prevents. Terminal bytes are high-volume and latency-sensitive, so they stay off the JSON-RPC stream (0096) and an agent window with a raw TUI keeps its escape hatch. A sequence log gives gapless catch-up after the frequent mobile reconnects without making every connect pay for a full snapshot, and the compacted fallback bounds memory. The owner's position is that efficiency (footprint and performance) is a guarantee to chase and measure, while the total workload is the owner's to size.

## Rejected

| Option | Why not |
|---|---|
| Control and events on one channel | Output volume can delay a cancel on slow links |
| Control priority on WebRTC only | The WebSocket fallback is where iOS Safari users are (fact 0016) |
| Terminal bytes inside the event stream | High volume in JSON-RPC on 1 vCPU, and it bends the meaning of ACP terminals |
| Always replay a fresh snapshot | Wasteful on flaky reconnects, no gapless live continuation |
| Defer to the agent's `session/load` | Only native agents with load; costs agent work; not for synthesized adapters |
