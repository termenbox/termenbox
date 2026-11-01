# Session search review fixes

Follows `archive/session-search-followups.md` (PR #476). Two read-only reviews of that branch found the items below; they are unverified claims until each step reproduces them with a failing test first. Decisions 0156, 0157, 0167. Guardrails `session-search-is-bounded-scrubbed-and-never-logs-the-query`, `resource-constraints`, `security-secrets-handling`.

## Cold start

Read `CLAUDE.md` and the archived plan's "Cold start". Work only in a pool worktree (`sh scripts/pool.sh acquire fix/<slug>`), commits `type(scope): summary` with `Why:`/`What:`. Commands via `sg docker -c "COMPOSE_PROJECT_NAME=termenbox-sergei scripts/dev.sh ..."`; cargo needs `env HOME=/tmp/h CARGO_TARGET_DIR=/cache/target-sergei CARGO_HOME=/cache/cargo-sergei` (memory note `dev-container-commands`). Land on the PR #476 branch `docs/session-search-followups` unless the owner says otherwise. Each step: write the failing test, fix, run its Check.

Owner decisions still open (defaults assumed): fix steps 1 to 4 before merge, later steps may follow; a per-file error skips that file only and never switches an agent off (step 3).

### 1 Client paging state — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Files: `lib/sessionSearch.ts`, `SearchSheet.svelte`, `sessionSearch.test.ts`.
Do:
- B1: `SearchSheet.svelte:24` resets `shown = PIECE` on every state change (App is legacy mode, so the `search` prop is a new object per hit). Keep the last query and range in plain variables; reset only when they differ.
- B2: take `older` from the first page only (`:252`); the server resends the full count each page.
- B3: when a server is at `MAX_HITS_PER_SERVER`, drop its cursor so the end marker stops asking.
- B4: `more()` clears `next` up front and Stop discards the cancelled reply; restore the cursors on Stop, or clear `next` only when the reply arrives.
- B5: `expand()` marks the key expanded before an answer; undo it on Stop or a lost server.
- B6: filter `more()` and `expand()` targets through `deps.servers()` so a dropped server is never added to `pending`.
- R4: remember the `since` `run()` sent and reuse it for every page.
- R7: drop the time/bytes cut sentence while a cursor offers more, as the `hits` one is.
- Keys: use the `\u0000`-joined hit key (`sessionSearch.ts:231`) for the keyed `each` (`SearchSheet.svelte:111`).
Done-when: tests for Stop during a page and during an expansion, `older` on a page reply, a cursor returned at the cap, more/expand after a server dropped, a moving window; a client render test (not SSR) that a plain hit during paging does not shrink the list.

### 2 Resume safety and feedback — [ ]
Class: implement · Needs: none · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'` and server tests for ids
Do:
- R2: session ids now come from file names. `plain_id` (`session_log/mod.rs:95-100`) and `adapters.rs:387-402` accept a leading `-`, so a file `--dangerously-skip-permissions.jsonl` makes Resume type `claude --resume --dangerously-skip-permissions`. Reject a leading `-` or require the agent's id shape; test with such a file name.
- R5: the Resume confirmation can sit off-screen above the chips; render it at the tapped row or scroll it into view and move focus to its Resume button.
- R6: `sessionAsked` (`App.svelte:377-397`) has no timeout and survives a dropped server; clear it on disconnect and match a reply to its ask.
Done-when: a hit whose id starts with `-` is never listed or resumed; component tests for focus and for a lost reply.

### 3 Listing under the caps, and file identity — [ ]
Class: implement · Needs: none · Check: `scripts/dev.sh env HOME=/tmp/h CARGO_TARGET_DIR=/cache/target-sergei CARGO_HOME=/cache/cargo-sergei cargo test -p termenbox-server session_log session_search`
Do:
- B7: `logs.list(since)` runs inside the budget clock but is charged to no cap and cannot be cancelled (`session_search.rs:179-186`; `claude.rs` `check_head`/`head_cwd`; `codex.rs`; `mod.rs:507-526`). Count head bytes against `max_bytes`, check time and cancel per file, and read each head once (the later `open` reads it again). Decide what a cut during listing reports (`time`, with the files not yet listed counted as older or unsearched). `locate` (every Resume) and per-session expand (`since` None) walk all roots with no window: bound them too.
- B8 and symlink R1: carry the listed path in `SessionFile` instead of rebuilding it from `cwd` (`claude.rs:79-109`, `:146`); a non-BMP directory name may encode differently from Claude's, and a mid-file `cwd` change reads as missing. Open with `O_NOFOLLOW` and check the file type; Codex matches by name (`codex.rs:96-101`, `:154` `is_dir` follows links). The test at `session_search.rs:635-662` passes only because the link's id differs; add one where the link has the real file's id.
- R3: errors from `File::open` (EMFILE, EACCES), a truncation during the scan (`scan.rs:654`) or `Chain::build` become `Disabled` and nothing clears `off` (`mod.rs:487-491`). Skip that file and keep the agent on.
Done-when: a 1,000-file fixture with a low `max_bytes`/time cap returns hits or a cut with a cursor, never zero by listing alone; listing a cancelled search stops; a link carrying a real id is never read; one unreadable file does not disable the agent. Update the guardrail wording only if the rule itself changes.

### 4 Re-run the end-to-end check — [ ]
Class: review · Needs: 1, 2, 3 · Check: `sg docker -c "COMPOSE_PROJECT_NAME=termenbox-sergei scripts/dev.sh --full sh -c 'cd packages/client && node e2e/session-search.mjs'"`
Do: replace the forgiving "Show more" retry loop in `e2e/session-search.mjs` with a strict one (the list never shrinks, `older` stays constant across pages); add a scenario for a file named with a leading `-`. Run `conform` over the new diff.
Done-when: the e2e passes with the strict assertions and `conform` reports no violation.

### 5 Scan allocations and cursor details — [ ]
Class: implement · Needs: 3 · Check: `scripts/dev.sh env ... cargo test -p termenbox-server session_log::scan`
Do:
- `Needle::new` builds an Aho-Corasick per file (`claude.rs:543`, `codex.rs:415`) and `check_head` allocates a 256 KiB `vec!` per file (`claude.rs:217`, `codex.rs:169`), above glibc's mmap threshold, which step 3 of the archived plan removed. Build the needle once per `SearchRun`; reuse the scanner's buffer for heads. Extend the counting-allocator test to cover listing.
- `stopped_at` doc (`scan.rs:383-385`) says "just after a newline" but a top-of-loop cut records the newline itself (`:467`); fix the doc or add 1 and check that a resumed scan sees the line as complete.
- The per-session page filters by id only (`session_search.rs:202-204`); carry the agent in `search.session` (both protocol files) or filter on the pair.
- The Codex `found` cache (`codex.rs:51`, `:103`) is unbounded; cap it.
Done-when: the 100-search test also shows no per-file big allocation while listing; the cursor tests still pass.

### 6 Resume races — [ ]
Class: implement · Needs: 2 · Check: `scripts/dev.sh env ... cargo test -p termenbox-server workspaces`
Do: `WorkspaceManager::resume_session` checks then creates (`workspaces.rs:199-246`); two clients resuming the same conversation can create two workspaces. Serialise per (agent, session). `d.is_dir()` (`:214`) blocks the async thread: use the async or blocking-pool form.
Done-when: a test with two concurrent resumes of one session yields one workspace and one `resumed`, one `switched`.

### 7 Sheet performance and labels — [ ]
Class: implement · Needs: 1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Do:
- Per hit, `sessionSearch.ts:229` scans the whole list to count a server's hits and `:239` copies the array; each change regroups and sorts everything (`SearchSheet.svelte:25`) and `lastOfSession`/`mayHoldMore` are quadratic per row. At 1000 hits that is about 50 full regroups per page on a phone. Keep a per-server counter and batch `onchange` per animation frame; measure before and after on 1000 hits.
- Agent and None modes: the section `aria-label` is empty in None mode and a hit's full directory has no tooltip. Add `title={h.dir}` on `search-where` (`:117`) and a label for the flat list.
- The archived plan says "evict far-off pages' DOM" but only `content-visibility:auto` is done (up to 1000 rows). Either evict (windowing) with a measurement, or reword the Done-when to what is true.
Done-when: a 1000-hit test shows the regroup count per page bounded; labels covered by tests; the memory claim matches the code.

### 8 Smaller server hardening — [ ]
Class: implement · Needs: 3 · Check: `scripts/dev.sh env ... cargo test -p termenbox-server`
Do: the gap between `readdir` and `open` lets a regular file swapped for a link be followed (covered by `O_NOFOLLOW` in step 3; add a test). Add a test that no log line holds an id or a path when listing fails.
Done-when: tests pass and nothing new is logged.
