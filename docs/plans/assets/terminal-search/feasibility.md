# Terminal search: feasibility (R59)

Question: can a user find a token that has scrolled out of view in a Termenbox terminal, and from which data source? Answered with a repeatable experiment against a real `termenbox-server`, real tmux (the private `-L termenbox` socket) and real Chromium.

## Run it

```bash
# once: server binary, client build, e2e harness build
scripts/dev.sh --full sh -c 'pnpm install --frozen-lockfile && cargo build --release -p termenbox-server && pnpm --filter termenbox-client run build && pnpm --filter termenbox-client run e2e:build'
# the experiment (SEARCH_RESULTS=path also writes the numbers as JSON)
scripts/dev.sh --full sh -c 'pkill tmux; cd packages/client && node e2e/search.mjs'
```

`packages/client/e2e/search.mjs` is registered in the Chromium e2e job (`.github/workflows/ci.yaml`, job `e2e`, after `wizard.mjs`). It prints one `ok` line per scenario and a `RESULTS {...}` JSON line, and fails if a premise below stops being true. The "snapshot" it measures is `termenbox-server tmux capture-pane -p -J -S - -t =<session>:`, which is what a server handler would run. Nothing in the product changes.

## What the experiment shows

Numbers are from one local run in the dev container (tmux 3.3a, Chrome with the DOM renderer, 390x844 phone viewport). CI re-runs the same assertions; timings are indicative, not gates.

| # | Scenario | Result |
|---|---|---|
| 1 | Print `TOK-0001..TOK-3000`, read the app's xterm rows | The browser holds 38 rows (`TOK-2974..3000`); `TOK-1500` is not in it. The tmux client runs in the alternate screen, so xterm keeps no scrollback (see `touchScroll.ts`). |
| 1 | Ask tmux for the pane history | `TOK-1000..3000`, 2004 lines, 18 KB (4 KB gzip), 14 ms. Older lines are gone: `history-limit` is 2000 rows (tmux's default; Termenbox's config does not change it). |
| 2 | tmux copy-mode search by keystrokes through the terminal (`C-b [`, `C-r`, `TOK-1500`, Enter) | Works: the browser then shows `TOK-1478..1506`. About 1 s end to end, mostly the 300 ms pauses between keystrokes in the script. The query reaches the server as keystrokes. |
| 3 | Search the 2004-line snapshot in the page | 0.6 ms, one hit. |
| 4 | A token wrapped at the pane width; accents, CJK and emoji | Without `-J` the token is split over two lines and not found; with `-J` it is whole. UTF-8 is intact when the server runs a UTF-8 locale (the experiment sets `LANG=C.UTF-8`). Accent-insensitive matching is not given by tmux; the page must fold (`normalize('NFD')`). |
| 5 | A full-screen program (stand-in for less, vim, an agent TUI) with 400 lines of its own text | While it runs (`alternate_on=1`) `capture-pane -S -` returns the shell history plus the program's current screen only: `ALT-0300` is not there. Only the program can scroll to it. After it quits, shell history is intact. |
| 6 | Reload the page and reattach | The browser buffer is rebuilt from the repaint: current screen only, `TOK-1500` absent. tmux still holds it, so history survives reattach on the server and never in the browser. |
| 7 | 25 more lines arrive after a snapshot | The snapshot lacks them. At the history limit `history_size` went 1977 to 1808 (tmux trims in blocks) while the oldest token moved from `TOK-1000` to `TOK-1200`: size is no signal of change. |
| 8 | Two sessions, two windows, a rename | A capture of another session or window never contains the token. A name target (`=s2:`) fails after `rename-session`; a pane id (`%n`) still resolves. |
| 9 | `history-limit 20000`, 25000 lines of about 50 characters in a 44-column pane | 19976 history rows held. The limit counts rows, so wrapped lines use two and about 10000 logical lines remain. Snapshot 0.53 MB (26 KB gzip), capture 30 ms, page search 9 ms. tmux RSS 4.7 to 8.4 MB, server RSS 11 MB. The first retained line can start mid-line. Browser heap growth was not measurable (below the granularity of `performance.memory`). |

## Options

| | A. Bounded tmux history snapshot, searched in the browser | B. tmux copy-mode search | C. Current-screen-only Find |
|---|---|---|---|
| Finds `TOK-1500` after it scrolled off | Yes, within the bound | Yes, within `history-limit` | No: only the visible rows (38 here) |
| Retrieval bound | Ours: last N rows and a byte cap, via `capture-pane -S -N` | tmux `history-limit` | The viewport |
| Wrapped lines | `-J` joins them; the first line may start mid-line | tmux searches across wraps | Visible rows; a split token needs `isWrapped` joining as `links.ts` does |
| Unicode | Intact with a UTF-8 locale; case and accent folding done in the page | Literal or regex, no accent folding | Same page folding |
| Query privacy | The query stays in the browser. The request names server, session, window, pane and the bound; the reply is terminal text the browser already receives as live output | The query is typed into the remote terminal as keystrokes | The query stays in the browser |
| Server and protocol change | One request/response pair in `termenbox-core`; one tmux call in `backend.rs` | None, but needs a copy-mode UX and the user's prefix key (already read by `runner_prefix`) | None |
| Latency | 14 to 30 ms capture, 1 to 9 ms search, plus transfer (20 KB to 0.5 MB, 4 to 26 KB gzip) | Interactive; about 1 s in the script | Instant |
| Memory against 512 MB / 1 vCPU | Snapshot is transient: 0.5 MB at 20000 rows. tmux keeps the history regardless | None added | None |
| Shell history vs application content | Shell and normal-screen output only; blind inside a full-screen program (scenario 5) | Same blindness | Sees what the program has drawn |

## Limits that hold for every option

- The data source is tmux's pane history. Text a full-screen program (an agent TUI, less, vim) keeps off screen is invisible to tmux and to xterm. Find inside those is the program's own feature.
- `history-limit` (2000 rows by default) bounds what exists. Raising it is the owner's tmux setting (`tmux.local.conf`), not part of Find, and costs tmux memory (about 4 MB per 20000 rows in this run).
- The browser never has history: reload or reattach rebuilds the buffer from the current screen. A snapshot is fetched, not cached across attaches.
- A snapshot is stale as soon as the pane emits output.
- Only the tmux backend can do this. `CommandBackend` has no history call and must offer current-screen Find only.
- Not measured here: iOS Safari, a slow link (transfer time), the WebGL renderer's memory, and CI timings (the first CI run supplies them).

## Binding and staleness (scenarios 7 and 8)

A snapshot is keyed by (server id, tmux session, window id `@n`, pane id `%n`) plus the client's count of output bytes at fetch time.

- Switching session or window discards the open results; Find on the new target fetches again.
- Output arriving while Find is open marks the results "older than the screen" with a visible label. It does not re-run silently, because history rows shift (scenario 7). A refresh action refetches and keeps the query.
- Reattach, reconnect or a missing pane id discards results. Target by pane id, not name, once a snapshot exists (scenario 8).
- Results are line numbers in the snapshot, not xterm positions: a hit opens in a read-only panel with context. The live terminal cannot scroll to it (no scrollback), except through option B.

## Recommendation

Ship A with C as the labelled fallback:

1. Find searches a bounded tmux history snapshot in the browser: last 5000 rows and 1 MB at most (`capture-pane -p -J -S -5000`), fetched on demand when Find opens, matched case- and accent-insensitively in the page.
2. Where the backend has no history call, or the pane is in a full-screen program, say so ("this program keeps its own text; searching the screen only") and search the current screen.
3. Leave tmux copy-mode (B) as the existing gesture, not a Find implementation: its query goes to the server as keystrokes.
4. This is separate from R43 (paid cloud history): it reads the tmux history already on the owner's server, stores nothing, and is free.

**Does a query leave the browser?** Not in A or C: the snapshot request carries the session, window, pane and a row bound, never the query. In B it does (typed into the remote terminal). Do not describe A as "device-only": the searched text comes from the server. A future server-side search (to avoid the transfer on very large histories) would send the query to the owner's server and must say so.

## Estimate

S=5, scored in R72: a history request/response in `termenbox-core` and `termenbox-server` (an optional `history(id, window, rows)` on `SessionBackend`), a Find panel for mobile and desktop, staleness and fallback labels, tests for the bound and the folding, and `search.mjs` promoted to the feature's e2e.

## Acceptance criteria for the implementation

1. In real tmux, a token printed 500 lines earlier and no longer on screen is found and shown with context in Chromium, on desktop and at phone width.
2. The request names the pane id, a row bound (at most 5000) and a byte cap (at most 1 MB); longer history is cut and the panel says "last N lines". The query string appears in no server-bound message (asserted on the recorded frames) and in no log.
3. A token split by wrapping is found; a query with different case or accents matches.
4. Output arriving while Find is open labels the results "older than the screen"; refresh updates them. Switching session or window clears them; a pane that no longer exists gives a clear message, not stale text.
5. Inside a full-screen program the panel says so and falls back to the current screen; a non-tmux backend offers the current screen only.
6. A 20000-row snapshot is captured in under 100 ms and searched in under 50 ms on the CI runner; server RSS does not grow over ten Finds.
