# Session search follow-ups

Follows `cross-session-search.md` (R78, landed in #448, shipped in `v0.1.57-beta.20`). Decisions: 0156 (what is searched), 0182 (amends it: every session in the agents' session roots, no ledger; written in step 7), 0157 (the SessionLog port). Fact 0065. Guardrails `session-search-is-bounded-scrubbed-and-never-logs-the-query`, `resource-constraints`, `security-secrets-handling`.

## Cold start

Termenbox is a web terminal for remote AI coding agents: Rust server `packages/server`, shared wire types `packages/core/src/protocol.rs` (mirrored by hand in `packages/client/src/lib/protocol.ts`), Svelte 5 client `packages/client/src`. Read `CLAUDE.md` first. Work only in a pool worktree (`sh scripts/pool.sh acquire <type>/<slug>`), one squashed PR per step off `develop`, commit subjects `type(scope): summary` with `Why:`/`What:` body. No compiler on some machines: prefix commands with `scripts/dev.sh`; without the `docker` group loaded, `sg docker -c "COMPOSE_PROJECT_NAME=termenbox-sergei scripts/dev.sh ..."` (see `server-update-flow.md`). Constraints: 512 MB RAM, 1 vCPU, secrets and the query never logged.

What search does today (verified at `a31db0d`):
- **Scan** (`packages/server/src/session_log/scan.rs`): each file is read backwards, newest lines first, with `pread` into reusable buffers; a line is folded (case, accents) into a scratch buffer and searched with `memchr::memmem`; only a matching line is parsed. `Scanner::new()` runs in `SearchRun::new` (`session_log/mod.rs`), so the buffers are allocated again for every search. Limits (`Limits::default`): 50 hits, 3 per session, 2 s, 200 MB.
- **Known sessions** (`registry.rs` `known_sessions`): one `agent_session` per tab. `set_agent_session` overwrites it when the agent restarts in the tab; `close_tab` and `close_workspace` delete the record, so a closed or replaced conversation has no pointer any more.
- **Client** (`lib/sessionSearch.ts`, `SearchSheet.svelte`): hits are grouped by workspace only; the client keeps at most 100 hits per server; a cut says "narrow the words" and there is no way to ask for more.

Goals: (a) switchable grouping, (b) fast, allocation-free scanning that does not read what a time bound rules out, (c) every session in the agents' session roots is found, closed or never opened in termenbox, (d) more results on demand.

Defaults assumed until the owner says otherwise (each is a choice, not a fact):
- "Lazy load" means more hits as the list is scrolled, past the 50 cap, plus rendering in pieces.
- A hit that no workspace holds offers Resume: switch to the workspace that already holds the session, else a new workspace named after the directory (0182). Never started by the search.
- Map/reduce across threads is out of scope: the target is 1 vCPU, so it adds a merge and no throughput. Revisit only with a measurement.

### 1 Grouping modes — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: `groupHits` in `lib/sessionSearch.ts` groups by workspace; `Group.agent` comes from the first hit. Per-viewer conveniences may use `localStorage` wrapped in try/catch (see the Artifact rules; the page renders without it).
Do: Add a segmented control **Agent / Workspace / None** to `SearchSheet.svelte`. `groupHits(hits, mode, nameOf)`: `agent` groups by `hit.agent`, `workspace` is today's behaviour, `none` is one flat list newest first. With no header a row shows its workspace and tab. Remember the choice per viewer.
Done-when: component tests cover the three modes, the order inside each, and the remembered choice; phone width shows the control without overflow.

### 2 Baseline: measure the scan — [x]
Class: explore · Needs: none · Check: the benchmark in `session_log/scan.rs` (plan R78 step 3) prints MB/s and peak RSS
Context: Every later performance step is a guess without numbers. The existing benchmark generates 200 MB; use `perf` or a flamegraph in the container.
Do: Record MB/s hot and cold-cache, peak RSS growth, allocations per search (counting allocator) and the share of time in fold, `memmem`, `pread` and JSON parse, for an ASCII-only and a non-ASCII corpus. Save the figures in `session_log/README.md` next to the fixtures' notes.
Done-when: the README lists the figures and the machine they were taken on; steps 3 to 5 each name the figure they expect to move.
Result: `session_log/README.md`. Fold is the cost (75 to 97% of the hot scan); `pread` is 4 GB/s; ASCII 601 MB/s hot, mixed 206, Cyrillic 36; cold is disk-bound (30 to 150 MB/s). Step 4 helps only blocks that are entirely ASCII, and a mixed corpus has non-ASCII in most blocks, so it gains little on real histories unless the fold is also reworked (see the note in step 4).

### 3 Pool the scan buffers — [x]
Class: implement · Needs: 2 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log::scan`
Context: A search allocates the 256 KiB block and the line, carry and fold buffers each time, and the sheet starts one about every 250 ms while typing. Allocations of 128 KiB and up are `mmap`ed by glibc, so each pays page faults and is returned at once.
Do: Keep a small pool of `Scanner`s (at most 2, so the idle footprint stays bounded) taken per search and returned on drop, with buffers cleared and capped on return. Cancel and early return must return the scanner too.
Done-when: a test running 100 searches shows one set of block-sized allocations in the counting allocator, not 100; RSS after the run does not grow with the count.
Result: `ScannerPool` / `PooledScanner` in `scan.rs`; `SearchRun` takes its scanner from the process pool (max 2 idle, buffers over 64 KiB dropped on return, returned on drop so cancel and unwind are covered). The 100-search test sees 0 allocations after the first and a heap peak under two blocks; RSS is not asserted (tests share the process), no allocation leaves nothing to grow.

### 4 Search blocks, not lines — [x]
Class: implement · Needs: 2, 3 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log::scan`
Context: Today every line is copied through the fold buffer before `memmem`. `aho-corasick` with `ascii_case_insensitive(true)` runs a SIMD (Teddy) search for one pattern with no copy. Accent-insensitive matching must stay: "cafe" matches "café", so a raw ASCII search is only valid where the needle has no letter that has accented forms, or on a block that is entirely ASCII.
Note from step 2: with one non-ASCII line in ten, almost every 256 KiB block holds one, so the whole-block ASCII path would rarely fire on real histories. Consider also folding only the non-ASCII lines of a block (search the block with the automaton, fold-check just the lines `is_ascii` rejects).
Do: Per block: if it is ASCII (SIMD `is_ascii`), search the whole block with the case-insensitive automaton, and find the enclosing line of a hit with `memrchr`/`memchr` for `\n`; never split the block into lines. A block with non-ASCII bytes takes today's fold path. Write down the accent rule in the module header and test it with a needle and text that differ only in accents.
Done-when: the contract suite and the existing scan tests pass unchanged; the benchmark's hot-cache MB/s on the ASCII corpus rises and the figure is in the README; the zero-allocation-per-non-matching-line test still passes.
Result: `BlockPlan` in `scan.rs`; ASCII hot 601 -> 3081 MB/s, mixed and Cyrillic unchanged (README, "After step 4"). Differential and accent tests added. Folding only the non-ASCII lines of a mixed block was not done: it is the next lever if real histories are mixed.

### 5 Reject candidates before parsing — [x]
Class: implement · Needs: 4 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log::claude session_log::codex`
Context: `memmem` also hits inside tool input and output, which the adapters then parse in full and discard (tool text never leaves the server).
Do: In each adapter, test the record type or role with a `memmem` on its marker before any JSON parse and drop tool records there. Parse the rest into borrowed structs (`#[serde(borrow)] &str`) over the line buffer with one reused scratch `String` for unescaping.
Done-when: a fixture with a large tool output holding the needle costs no parse (counting allocator) and yields no hit; the contract suites for both adapters pass.
Result: `may_hold_text` in both adapters drops tool-only lines on quoted-marker `memmem` checks (escaped text in a string never matches, an unexpected layout falls through to the parse). Tests compare allocations with and without the needle in a 500 KB tool output: equal (and a mutation check made it fail). Not done: borrowed `serde(borrow)` structs with a scratch `String`. `parts()` is shared with `window`/`page`, so a second parser would drift from the contract suites, serde cannot unescape into a caller's buffer, and after the reject only real message lines parse (~2 us each, step 2).

### 6 Time window and time bounds — [x]
Class: implement · Needs: 4 · Check: `scripts/dev.sh cargo test --workspace`
Context: Session files are append-only, so a file last written before a cutoff holds nothing after it; its `mtime` is free. Inside a long file, records are in time order, so the timestamp of the first complete line of a block (one `memmem` per block, not per line) tells where to stop reading backwards. `mtime` can lie after `cp`, `rsync` or a restore; with no ledger (0182) that risk is accepted. Codex puts the date in the rollout file name and in its records; the time-of-last-event function belongs in the `SessionLog` port.
Do: Add `since` (milliseconds) to `search` in both protocol files. Skip a file whose last-event time is before `since` without opening it; stop a file at the first block older than `since`. Add the cut reason `range` and a count of sessions skipped, shown as "N older conversations not searched. Search older". Default window 30 days; chips *24 h / 7 d / 30 d / Any* in the sheet. The time and byte caps stay as the safety net.
Done-when: a server test shows a file older than `since` is never opened (read counter is zero) and the skipped count is reported; a long-file test stops mid-file; the sheet test covers the chips and the "Search older" action; the query is still absent from the log.
Result: `since` on `search`, `older` and the cut `range` on `search_done` (both protocol files). `SessionLog::last_event` (file mtime for both adapters; there is no recorded time to override it (0182, no ledger)) lets `session_search::run` skip a file without opening it; `scan_file` reads the first whole line's `"timestamp"` per block with one `memmem` and ends with `End::Range` after the first block older than `since`. `older` counts both. The sheet has the 24 h / 7 d / 30 d / Any chips (remembered per viewer, 30 d default) and "Search older". Tests: a stale file costs zero bytes, a 4 MB file reads under two blocks, chips and the older action. The block's time is the record's own top-level `timestamp`, read with a borrowing serde struct (no allocation; a nested one is skipped; an unreadable or over-64 KiB first line never stops a scan), and a file ends only at the second block in a row whose first line is older, so one out-of-order record cannot hide newer ones (up to three blocks are read). Not run here: clippy `-D warnings` stops in `bridge-contract` (existing, `explicit_counter_loop`), and 20 server tests need real tmux (environment).

### 7 Decision: search every session in the agents' session roots, no ledger — [x]
Class: decide · Needs: none · Check: `lint` over `docs/` passes
Result: Decision 0182 (accepted by the owner, 2026-11-01) amends 0156: the corpus is every session file inside each adapter's session root, termenbox stores nothing new and there is no ledger. Guardrail `session-search-is-bounded-scrubbed-and-never-logs-the-query` changes its confinement from "known folders" to "the adapter's root, no symlink out, no client path, no unrecognised format". A hit's directory comes from the file's `cwd`; badges are `open` and `resume`; hits group under the holding workspace or the directory; Resume switches to the workspace that already holds the session, else creates a workspace named after the directory. Fact 0065 notes the folder name is lossy; the adapter skill gained a root-and-symlink step.

### 8 Enumerate the session roots — [ ]
Class: implement · Needs: 6, 7 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log`
Context: `known_sessions` in `packages/server/src/registry.rs` and `session_search::run`; `SessionLogs::standard()` already resolves each agent's home. Decision 0182.
Do: Add `SessionLog::list` (or equivalent) that walks the adapter's root and yields session files with agent, session id, `mtime` and the `cwd` read from the file, newest first, without following symlinks out of the root. `session_search::run` searches that list. The registry's sessions only mark a hit `open` (a running tab holds it) or `resume`, and name the holding workspace; `distinct` keeps one entry per conversation, preferring `open`. The wire `up` flag stays for old clients and gains a `state` beside it.
Done-when: tests show a closed tab, a closed workspace, a replaced session and a session started outside termenbox are all found; a symlink pointing outside the root is never read; a file of an unrecognised format is skipped; no log line holds an id, a path or the query.

### 9 Landing and Resume for a hit no tab holds — [ ]
Class: implement · Needs: 1, 8 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: `landingOf` and `resumeOutcome` in `lib/sessionSearch.ts`; Resume reuses 0152's command table (`claude --resume <id>`, `codex resume <id>`, `opencode --session <id>`) on the server. Decision 0182.
Do: Badge `resume`; the group is the holding workspace or the directory (last two segments, full path as a tooltip). A tap asks for confirmation. The server then switches to the workspace that already holds the same agent and session id, else creates a workspace named after the directory and runs the resume command; the reply says `switched`, `resumed` or `shell`. If the directory is gone, say so and offer nothing. Nothing in the client types into an agent.
Done-when: component tests cover the confirmation, the shell fallback message and the missing directory; server tests show an already-held session opens no duplicate and an unheld one creates a workspace in the hit's directory.

### 10 More results: per-session and continuation — [ ]
Class: implement · Needs: 6, 8 · Check: `scripts/dev.sh cargo test --workspace`
Context: The cap of 3 per session and 50 overall stays the default so a reply fits 48 KiB. Decision 0157's cursor carries the file fingerprint; the server keeps no search state.
Do: `search` takes an optional `session` (show all matches of that one conversation, cap lifted to a fixed larger number) and an optional `after` cursor (last session and byte offset, fingerprinted) that makes the scan resume where the cap stopped. A changed fingerprint says so and restarts that file. Replies stay within 48 KiB.
Done-when: a test pages through a fixture with more than 50 matches and sees each hit exactly once; a test with a file appended between pages does not repeat or skip a hit; the log-capture test passes.

### 11 Lazy load in the sheet — [ ]
Class: implement · Needs: 10, 1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: The client keeps at most 100 hits per server (`MAX_HITS_PER_SERVER`); `IntersectionObserver` is available on iOS Safari 16.4.
Do: Render the list in pieces with an `IntersectionObserver` sentinel, and request the next page with the cursor when it is reached and the server reported a cut. Each group shows "N more in this conversation" which runs the per-session search. Bound memory: evict far-off pages' DOM, keep the hits. Cancel on close or a new query, as now. Works in all three grouping modes.
Done-when: tests cover scroll-to-more, the end of results, a new query during paging, the per-session expansion and the memory bound after many pages.

### 12 End-to-end and privacy check — [ ]
Class: review · Needs: 9, 11 · Check: `sg docker -c "COMPOSE_PROJECT_NAME=termenbox-sergei scripts/dev.sh --full sh -c 'cd packages/client && node e2e/session-search.mjs'"`
Context: `packages/client/e2e/session-search.mjs` already covers open, dormant and capped searches against the real server and tmux. Phone checks must name their platform (guardrail `a-phone-check-names-its-platform`).
Do: Extend it with a closed tab, a session started outside termenbox, an agent restarted in the same tab, a grouping switch, the time chips with a skipped count, and paging past 50. Assert the query and any session id or path appear in no server log line. Run the `conform` skill over the diff, then move the follow-ups out of this plan per the roadmap's landing procedure.
Done-when: the Chromium e2e job passes with the new scenarios, `conform` reports no violation, and the plan is archived.
