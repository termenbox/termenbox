# ACP panic-path audit (plan step 2.1)

Read-only audit by an explorer agent on 2026-10-28 of `packages/server/src/{acp.rs,acp_stream.rs,event_log.rs,backend.rs}`, tests skipped, no build run. Release profile unwinds (no `panic = "abort"`). Not covered: `events.rs` sanitizers, `EventHub::report`, `termenbox_core::ext`, `pty.rs`, `control.rs`, `workspaces.rs` outside lines 285-345 and 870-1050. Line numbers are as of commit 4c3c039.

No agent-controlled panic was found in non-test code: decoding is lenient, lines are capped at 1 MiB, text is capped. The real risks are panic-to-poison spread, unbounded queues, unkillable children and cross-session lock coupling.

## acp.rs

| # | Risk | Line | What | Effect across sessions | Fix |
|---|---|---|---|---|---|
| 1 | High | 1029 (under the lock at 1246, 1351, 1607); global map 1155 | `running_in()` locks every session's `inner` while holding the global `workspaces` guard. A poisoned `inner` panics there, poisoning the global map; every later `workspaces.lock().unwrap()` (1197, 1207, 1218, 1230, 1459, 1560, 1602, 1614, 1733, 1779) and `write_state` (1039) then panics. | One session's panic breaks list, create, restore, save, stream view and prompt for all, until restart. | `parking_lot::Mutex` on both. Never take a per-session lock under the global one: snapshot the `Arc<Live>` list, release, then read status. |
| 2 | High | 855, 627-633, 687-690, 716-718 | The agent stdin queue is an unbounded channel; every agent request queues a reply, and the reader runs at full speed while the writer (879-888) may be blocked. | Memory grows without bound from one agent: an OOM path on 512 MB. | Bounded mpsc (about 256) with `try_send`; on full, fail the session ("agent not reading") and kill it. |
| 3 | High | 890-916 (panic sites under the lock at 596, 643, 698, 715, 768, 671-694) | The reader task has no unwind guard. A panic skips `finish()` (915): status stays Running, `waiting` is not drained, the prompt waits forever. | The session keeps its `max_sessions` slot; the poisoned `inner` feeds #1. | Drop guard whose `Drop` calls `finish(Failed("panic"))`, or `catch_unwind` around `on_message`; keep the JoinHandle and log the JoinError. |
| 4 | High | 907, 911-915 | After the read loop breaks, `child.wait()` runs outside the select, so `kill_rx` (894) is no longer polled. A child that closes stdout but keeps running is never killed. | Leaked process, hung reader task, cap slot held forever. | After the break call `child.start_kill()` and wait with a timeout; treat a writer failure (881-886) as end of session. |
| 5 | Medium-High | 1758-1760, 1471 | `acp_events` and `stream_view` do CPU work (ring absorb, up to 400 entry clones) under the session's `inner` lock. | A big reconnect replay stalls that session, and via `running_in` create and list server-wide. | Copy an `Arc` snapshot under the lock; build the replay outside it or in `spawn_blocking`. |
| 6 | Medium | 569, 573, 576 and every `inner.lock().unwrap()` (596 … 1759) | `std::sync::Mutex` on per-session state: a panic poisons it and every later `unwrap` panics. | One session directly; global via #1. | `parking_lot::Mutex`, or `unwrap_or_else(\|e\| e.into_inner())`. |
| 7 | Medium | 1659-1716 (1667-1668, 1672, 1642-1643) | The prompt task is fire-and-forget with `session/prompt` timeout `None`; `turn` is reset only on normal return. A hang or panic leaves `turn = true` ("still answering" forever), silently. | Session wedged. | Drop guard that resets `turn`; a prompt timeout or rely on `finish()`; log the JoinError. |
| 8 | Medium | 1318-1324 vs 1779-1793, 1392-1396 | `kill()` during a restore finds no live entry; `start()` then inserts it and the child is not killed. | An agent resurrected after the owner closed it. | Tombstone or generation counter checked before insert. |
| 9 | Medium | 838-852, 895, 1787-1789, 1334 | Agents are not in their own process group; kill reaches the direct child only; a server crash reparents children, and restore starts a second agent for the same session id. | Leaked RAM on a small box. | `process_group(0)`, kill the group, and `PR_SET_PDEATHSIG` in `pre_exec`. |
| 10 | Medium | 879, 890, 1659; `main.rs:552` | Spawns with no JoinHandle observed: a panic disappears; in restore the placeholders keep `restoring = true` (1377-1380) forever. | Silent loss. | `catch_unwind` plus `tracing::error!`; a supervisor that logs JoinErrors. |
| 11 | Medium | 1157, 1346-1351, 1376, 1409, 1620 | The `starting` counter has no RAII guard; a cancelled future or panic leaks a slot, a double decrement wraps in release. | The cap silently shrinks or stops working. | RAII slot guard and `saturating_sub`. |
| 12 | Medium-Low | 649, 653-656 | A timed-out request leaves its oneshot in `waiting`. | One leak per timeout (repeatable with `set_mode`, 1495-1536). | Remove the id on timeout. |
| 13 | Medium-Low | `workspaces.rs:1016`, 327; `acp.rs:1271` | `create_with` is awaited in the connection's serve loop (handshake up to 30 s per step); a dropped future orphans the child and leaks `starting`. | Latency on one connection; leak on cancel. | Run create on a task and report via a channel; kill the child in a cancel guard. |
| 15 | Low | 1074 | `std::fs::write` of the state file is not atomic; a crash mid-write truncates `acp-workspaces.json`. | Every restorable workspace lost. | Write a temp file, then rename. |
| 14, 16, 17 | Safe | 1274-1276, 1433, 1594 | Checked: correct or guarded. | None. | Optional `let-else` / `.get()`. |

## acp_stream.rs

No lock here; a `Stream` lives in the session's `Inner`, so a panic poisons that `inner` and spreads through acp.rs #1.

| # | Risk | Line | What | Fix |
|---|---|---|---|---|
| 1 | Medium | 915-944, 440-456, 422-428 | `view()` runs under the `inner` lock and clones up to 400 entries per poll. | Snapshot the `Arc`'d entries and clean outside the lock, or cache the cleaned body. |
| 2 | Medium | 408-413 vs 427 | Hand-kept byte accounting: drift makes the unchecked subtraction at 410 underflow (debug panic, release wrap, then every push evicts everything). | `saturating_sub` and a `debug_assert` on the total. |
| 5 | Low | 262-285, 290-341 | `fit()` serialises every entry on every split (bounded, 16 iterations). | Cache the serialised size on the slot. |
| 3, 4, 6, 7 | Safe/Info | 739-744, 388-437, 788-790, 864, 599-697 | Boundary checks, checked subtraction and caps hold. | None. |

## event_log.rs

No lock; the only global is the `TOTAL_BYTES` atomic (34).

| # | Risk | Line | What | Fix |
|---|---|---|---|---|
| 1 | Medium | 227-233, 360-395 (381, 391-393) | `resume` clones the base and absorbs the whole ring: O(n²) under the `inner` lock. | Index tool-call ids in a `HashMap`; build the replay outside the lock. |
| 2 | Medium | 175, 183 | `base_bytes` is re-summed on every popped frame and `base.remove(0)` is O(n): quadratic CPU on a flood of small frames. | Maintain `base_bytes` incrementally; use a `VecDeque`. |
| 3 | Medium | 24; `acp.rs:168-171` | Config allows 16 sessions × 64 MiB ring = 1 GiB of logs on a 512 MB box; nothing caps the sum. | Validate `max_sessions × event_ring_bytes` against a total budget, or a global pool using `TOTAL_BYTES`. |
| 4 | Low-Medium | 207 (client `after` from `workspaces.rs:912-913`) | `a + 1` overflows at `u64::MAX`: debug panic, release wraps harmlessly. | `a.checked_add(1).is_some_and(\|n\| n >= f)`. |
| 5-7 | Low | 136-151, 372/175/184, 34-38/194-199/243-246 | Scrub copies before the size check; size drift; process-wide counter drift. | Check raw size first; recompute sizes; `debug_assert`. |
| 8 | Safe | 297-317, 121 | Recursion bounded at 128 by `serde_json`. | None. |

## backend.rs

No std `Mutex`/`RwLock`/`tokio::spawn` in non-test code. Low items only: `self.parts[0]` (1007, unreachable today), `run_as` `.output()` without `kill_on_drop` (486-492), a synchronous `tmux -V` with no timeout at startup (310, 317-341).

## Top risks (what plan steps 2.2 and 2.3 must cover)

1. Poison spread through `running_in` under the global lock (acp.rs #1, #6).
2. Reader task with no unwind guard, leaking the cap slot and the open turn (#3, #7, #10, #11).
3. Unbounded agent stdin queue (#2) and the 1 GiB event-log ceiling (event_log #3).
4. Children that are never killed or outlive the server (#4, #9) and a kill during restore that resurrects a workspace (#8).
5. Non-atomic state write (#15), which loses every restorable workspace on a crash.
