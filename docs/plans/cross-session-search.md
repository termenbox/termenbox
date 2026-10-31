# Cross-session search

Decisions: 0156 (what is searched and how a hit lands), 0157 (the SessionLog port). Fact 0065 (what the agent TUIs offer). Guardrail `session-search-is-bounded-scrubbed-and-never-logs-the-query`. Roadmap step R78. Related, not changed: R72 (find inside one terminal pane), decision 0152 (dormant workspaces resume their agents).

## Cold start

Termenbox is a web terminal for remote AI coding agents: Rust server `packages/server`, shared wire types `packages/core/src/protocol.rs` (mirrored by hand in `packages/client/src/lib/protocol.ts`), Svelte 5 client `packages/client/src`. Read `CLAUDE.md` first. Work only in a pool worktree (`sh scripts/pool.sh acquire feat/session-search`), one squashed PR per step off `develop`, commit subjects `type(scope): summary` with `Why:`/`What:` body. No compiler on some machines: prefix commands with `scripts/dev.sh`. Constraints: 512 MB RAM, 1 vCPU, secrets never logged.

The feature: one global Search sheet finds text in agent conversations by reading the agents' own session files (Claude Code JSONL under `~/.claude/projects/<project>/<session-id>.jsonl`, Codex rollout files), for every session termenbox has a record of, open or dormant. The server scans on demand, with no index. A tap switches to the tab, or offers Resume when the workspace is down. In an ACP workspace the view also scrolls to the turn and highlights it; a terminal tab only switches.

Agreed defaults: case- and accent-insensitive; newest first; at most 50 hits, 3 per session; scan stops at 2 s or 200 MB; replies at most 48 KiB; scrubbed snippets; the query is never logged.

### 1 Measure the Claude and Codex session files — [x]
Class: explore · Needs: none · Check: `ls packages/server/testdata/session_log/claude packages/server/testdata/session_log/codex`
Context: Decision 0157 needs adapters for two formats that the vendors call internal. Fact 0065 records only what the docs say. Use a throwaway `HOME` and a throwaway tmux (`tmux -L scratch`), never the owner's sessions or the owner's `termenbox.service`.
Do: Run Claude Code and Codex on small prompts to produce real session files. Record where they live, the record shape, how a rewind or fork, a compaction and a tool call are written, and what a torn last line looks like. Save fixtures under `packages/server/testdata/session_log/<agent>/`: a short conversation, a rewind or fork, a torn last line, a truncated file, an unknown version, and a tool output holding a fake secret. Extend fact 0065 with the paths, versions and shapes measured.
Done-when: both fixture directories hold the six cases above, and fact 0065 lists each agent's version and file location.
Progress: done. Real sessions run for Claude 2.1.308 and Codex 0.166.0 (resume, fork, tool call, compaction for Claude). Not measured, fixtures are synthetic there (listed in `testdata/session_log/README.md` and fact 0065): Codex fork and compaction, Claude rewind. Step 4 must treat the Claude tree as the source of truth for the active branch; step 5 must tolerate an absent fork marker.

### 2 SessionLog port, unified entries and the contract suite — [x]
Class: implement · Needs: 1 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log`
Context: Decision 0157. The unified entry kinds are the ones `packages/client/src/lib/acpStore.ts` `Entry` already renders (`user`, `message`, `thought`, `tool`, `plan`, `notice`). Scrubbing is `crate::events::sanitize_block` (as `packages/server/src/event_log.rs` uses it). Model the port on `WorkspaceBackend` in `packages/server/src/backend.rs`.
Do: Add `packages/server/src/session_log/mod.rs` with the trait (`search`, `window`, `page`), the cursor type that carries the file fingerprint (inode, size, mtime), the unified entry and hit types, and an adapter registry keyed by agent name. Add a shared contract suite that runs any adapter over its fixtures: a torn last line is dropped, a truncated file and an unknown version turn search off for that agent without a panic, an unmappable record becomes a `notice`, tool input and output never appear in the output, secrets are scrubbed, and a changed fingerprint invalidates a cursor.
Done-when: `session_log` compiles, a stub adapter passes the contract suite, and the suite fails when the stub copies tool output.
Progress: done. `session_log/{mod,contract,stub,tests}.rs`; 9 tests pass, clippy and rustfmt clean for the module. Notes for later steps:
- The suite (`contract::run`, `Cases`) needs, besides step 1's six cases, a fixture with an unmappable record (`unmappable`, its turn id) and one with text the scrubber hides (`secret_text`; the tool-secret fixture's "token variable" works, since "token" hides the next word). Steps 4 and 5 add the unmappable fixture; step 1's README should list it then.
- A search matches the *scrubbed* text only (`snippet`), so a query cannot confirm a guess at a hidden secret. The engine in step 3 must prefilter on raw bytes but confirm with `snippet`.
- The scrubber is not idempotent (`[secret]` is itself a keyword), so the suite checks "already scrubbed" only on sessions without secrets.
- `Fingerprint::still_holds`: same inode, not smaller, and equal mtime when the size is equal. Appends keep a cursor valid.
- `snippet` folds case only; step 3 adds accent folding.
- Build on this machine: `sg docker -c "scripts/dev.sh env CARGO_TARGET_DIR=/cache/target/<user> cargo test ..."` (the shell may predate the docker group; the shared `target` dir is owned by another uid). `cargo fmt` needs `HOME=/tmp`.

### 3 Bounded scan engine and its benchmark — [ ]
Class: implement · Needs: 2 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log::scan`
Context: Decision 0157 performance paragraph. Scan with `pread` into one reusable buffer and `memchr::memmem` over a pre-folded needle; parse JSON only for lines that match. No `mmap` (guardrail). Matched lines do allocate and that is allowed. Case folding is ASCII fast path with Unicode fallback; accent folding uses NFD stripping of the needle and the candidate line only after a byte-level prefilter hit. Caps: 50 hits, 3 per session, 2 s, 200 MB, newest file first.
Do: Add `session_log/scan.rs`: a file scanner that yields line offsets and byte ranges of candidate lines, a lazily built and incrementally extended byte-offset index per file kept in a small LRU, cancellation checked between buffer reads, and the cap accounting that reports `cut` with a reason. Run it on a blocking thread, never on the control path. Add a benchmark test on a generated 200 MB fixture with budgets for time and peak RSS (assert RSS growth under the limit in guardrail `resource-constraints`), and a test that the scan allocates nothing per non-matching line (a counting allocator in the test).
Done-when: the scan test and the benchmark pass in CI, and a search that hits the 2 s cap returns its partial hits with `cut` set.

### 4 Claude Code adapter — [ ]
Class: implement · Needs: 2, 3 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log::claude`
Context: Fixtures from step 1. The JSONL is a tree (parent ids), so a rewind or fork leaves abandoned branches; the adapter shows the active branch only. Claude's format is internal and can change.
Do: Add `session_log/claude.rs` implementing the port: locate the file for a session id and directory, `search` through the scan engine, `window` returning the entries around a hit's turn within the 48 KiB cap, `page` with fingerprinted cursors. An unknown version or unreadable file disables Claude search only.
Done-when: the contract suite passes over the Claude fixtures, including the rewind case, and a window around a hit returns the hit's turn id and fits 48 KiB.

### 5 Codex adapter — [ ]
Class: implement · Needs: 2, 3 · Check: `scripts/dev.sh cargo test -p termenbox-server session_log::codex`
Context: Same as step 4 with the Codex rollout format measured in step 1. Follow `docs/skills/add-a-session-log-adapter.md`.
Do: Add `session_log/codex.rs` implementing the port the same way, and register it.
Done-when: the contract suite passes over the Codex fixtures.

### 6 Known sessions: tab to session id to file — [ ]
Class: implement · Needs: 2 · Check: `scripts/dev.sh cargo test -p termenbox-server registry`
Context: Decision 0152 adds `TabRecord.agent_session` in `packages/server/src/registry.rs`, written from hook events (`adapters::agent_session_id`). Run `grep -n agent_session packages/server/src/registry.rs` first: if the field is absent, implement that part of 0152 here (the id only, in the registry file, never logged). The server must also know the ACP `sessionId` of ACP workspaces.
Do: Add a function that lists the known sessions: for each workspace and tab, the agent, directory, session id, workspace id, tab id and whether the workspace is up or dormant. Only sessions whose directory belongs to a known workspace or tab are returned, so the scan can never read outside them. Never log the ids.
Done-when: a test with one open and one dormant workspace lists both sessions with the right state, and a session file whose directory is not a known one is not returned.

### 7 Protocol: `search` and `window` messages and handlers — [ ]
Class: implement · Needs: 4, 5, 6 · Check: `scripts/dev.sh cargo test --workspace`
Context: Wire types are in `packages/core/src/protocol.rs` and mirrored by hand in `packages/client/src/lib/protocol.ts`. The direct transport carries 64 KiB a message, so replies stay at most 48 KiB. The existing `history` request (`packages/server/src/history.rs`) shows the pattern for a size-capped reply. Hits stream in more than one message so the sheet can fill while the scan runs.
Do: Add `search` (query, optional cancel) and `search_hit`/`search_done` (with `cut`), and `window` (session, anchor turn id, before, after) with a reply of entries, in both type files. Route them to the registry and the adapters. Search is owner-only. Handle cancel and a closing connection. Make sure no log line, audit entry or error text contains the query or a snippet, and add a test that captures all log output during a search and asserts the query string is absent.
Done-when: a server test searches a fixture session and gets scrubbed hits, a cut flag at the cap, and a window around a hit; the log-capture test passes; the client protocol types compile.

### 8 Search sheet — [ ]
Class: implement · Needs: 7 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: Decision 0156. Entry point: the desktop sidebar (`packages/client/src/shell/SideNav.svelte`) and the phone header. Look at `FolderPicker.svelte` and `WorkspaceSheet.svelte` for how sheets are built at phone and desktop width. Minimum browser: iOS Safari 16.4.
Do: Add `SearchSheet.svelte` and a small store: a debounced query input, hits grouped by workspace (agent icon, tab, date, scrubbed snippet with the match highlighted, an `open` or `resume` badge), a note that the query is sent to the server, a "search was cut" line when `cut` is set, and cancel on close or a new query. Empty, loading and error states fit one screen.
Done-when: component tests cover typing, streaming hits, cut, cancel and the empty state; the sheet shows the server notice.

### 9 AcpView history block and scroll-to-turn — [ ]
Class: implement · Needs: 7 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: Decision 0157. `acpStore.ts` keeps at most 400 entries (`MAX_ENTRIES`) and `AcpView.svelte` renders them with a plain `{#each shown as e (e.id)}`; its jump uses `querySelector('[data-entry=...]')`, so the target must be rendered first. The reducer and its cap stay as they are.
Do: Keep fetched older entries in a separate history block above the live list, windowed and evicted so memory is bounded. Prepending must be scroll-anchored so the view does not jump. A hit makes the view request the window, render it, then scroll to the turn and highlight the occurrence with the existing `data-hit` marks. If the window's turn id overlaps the live ring, the ring wins and the file entries stop at its first entry. A changed cursor fingerprint re-runs the search and says so.
Done-when: tests show a hit older than the ring appears above the live entries, the view scrolls and highlights it, a prepend does not move the visible content, and memory stays bounded after many windows.

### 10 Tap actions: switch or resume — [ ]
Class: implement · Needs: 8, 9 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: Decision 0156. Resume is an explicit action and reuses 0152's command table (`claude --resume <id>`, `codex resume <id>`, `opencode --session <id>`) on the server; the reopen reply says `resumed` or `shell` per tab. Nothing in the client types text into an agent.
Do: A tap on a hit in an open workspace switches to the tab; in an ACP workspace it then runs step 9's jump. A tap on a hit in a dormant workspace shows a Resume action, and after it completes does the same landing. A terminal tab only switches. If the resume fell back to a shell, say so and do not claim the hit is shown.
Done-when: component tests cover the three landings (ACP open, terminal open, dormant resume) and the resume fallback message.

### 11 End-to-end test and privacy check — [ ]
Class: review · Needs: 10 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/run.mjs'`
Context: The browser e2e harness is in `packages/client/e2e/` (see `search.mjs` for R72's experiment and how it is registered in `.github/workflows/ci.yaml`). Phone checks must name their platform and never call an emulator a phone (guardrail `a-phone-check-names-its-platform`).
Do: Add `e2e/session-search.mjs`: fixture Claude and Codex sessions in an open and a dormant workspace; search a phrase; assert scrubbed hits from both, the `cut` message at a forced cap, the three tap landings, and that the query appears in no server log line. Run the `conform` skill over the diff against the guardrails.
Done-when: the Chromium e2e job passes with the new scenario, the conform check reports no violation, and R78 is moved to `roadmap/archive/` per the roadmap's landing procedure.
