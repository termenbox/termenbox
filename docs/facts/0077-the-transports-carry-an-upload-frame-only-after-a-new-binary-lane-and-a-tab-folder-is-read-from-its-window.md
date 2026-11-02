---
id: 0076
title: Neither transport carries a client-to-server upload frame yet; a tab's live folder is the path of its window state
status: active
tags: [protocol, server, client, upload]
date: 2026-11-02
kind: observed
last-verified: 2026-11-02
---

# What exists today (read from the code, 2026-11-02)

- **Direct transport (`rtc.rs`).** Three negotiated data channels: `status` (JSON text), `term` (id 1, binary, raw terminal input and output) and `control` (id 2, JSON text). A message over `MAX_MESSAGE` (64 KiB) closes the connection. A binary message on `status` or `control`, or a text message on `term`, closes it too. Terminal output is joined into messages of `TERM_MESSAGE` (16 KiB). Inbound messages wait in a backlog capped at `BACKLOG_BYTES` (256 KiB) and 4096 messages; beyond that the connection is closed.
- **WebSocket fallback (`websocket.rs`, `link.rs`, `client/src/lib/ws.ts`).** Text frames are JSON. A binary frame starts with a lane tag; only `LANE_TERM = 1` exists and an unknown tag is ignored. No frame limit beyond the library's.
- **Client.** `rtc.ts` sets `binaryType = 'arraybuffer'` on `term` only; `ws.ts` strips the tag and calls `onterm`. Both implement `Transport` (`transport.ts`) with `sendTerm`; there is no other binary send.
- So binary exists only as the terminal lane. Nothing carries file bytes, and base64 inside JSON would also hit the 64 KiB cap and cost a third more.

# What an upload frame needs

- An upload frame is a new binary kind: `LANE_UPLOAD = 2` on the WebSocket (old servers and clients already ignore an unknown tag), and on the direct transport a fourth negotiated channel `upload` (id 3, binary, client to server) so a chunk never queues in front of keystrokes on `term`. The server adds `Inbound::Upload(Vec<u8>)`; every `match` on `Inbound` (`workspaces.rs`, `conductor.rs`, `rtc.rs::inbound_len`) must handle it. A client that does not open the channel simply cannot upload; the WebSocket path is the fallback.
- Layout: tag (WebSocket only), upload id `u32` big-endian, byte offset `u64` big-endian, then the payload. A header is 12 bytes without the tag.
- Largest safe payload: 16 KiB on both transports (the size the direct transport already uses for terminal output and well under the 64 KiB cap). The decoder rejects a frame above 16 KiB plus the header.
- The 256 KiB backlog would close the connection under a 1 MiB window. Upload frames therefore bypass `BACKLOG_BYTES` and are bounded by their own cap, the window plus one frame.
- Proposed window: 1 MiB of unacknowledged bytes (DECISION-0194's starting value). At 100 ms RTT that is about 10 MB/s, above a phone uplink; the server holds at most one window, about 0.2% of the 512 MB budget. Confirm by measurement in step 3.1.

# A tab's live working folder

- `WorkspaceBackend::windows(&self, id) -> Result<Vec<WindowState>>` (`backend.rs`) lists a workspace's tmux windows; `WindowState.info.path` is `#{pane_current_path}` of the window's active pane, parsed by `control::parse_windows` from `PANE_FORMAT`.
- A registry tab maps to its window by `record.tabs[..].window` equal to `WindowState.window_id` (as `Workspaces::window_list_for` does).
- Caveats for the resolver: `parse_windows` drops control characters and cuts the path to 256 characters, so a longer folder would be wrong; `upload.rs` must refuse a path that reached 256 characters, or read the path with an uncapped `display-message -p -t` as `new_window` does. A tab with no live window falls back to the registry directory, which the upload must not use (the decision wants the live folder).
