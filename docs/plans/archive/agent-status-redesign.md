> Archived 2026-10-20. All steps A.1-A.8 are done (landed in #135); decisions 0095-0097 are active. The agent-state derivation was later reworked by decision 0122 (turn-and-obligation reducer); this file is history.

# Termenbox Plan: agent status redesign (Activity + Attention), two data channels, push, herdr removal

Decisions (all draft until the owner accepts): 0095 (status model, task info, notifications), 0096 (two data channels), 0097 (herdr removed). Replaces the state part of 0061; 0079 and 0083 stand. Guardrails: `notifications-carry-no-agent-text`, `audit-logging-no-secrets`, `security-secrets-handling`, `internal-network-binding`, `resource-constraints`, `user-settings-are-wrapped-not-replaced`. Facts: 0029 (tool surfaces), 0034 (scan fallback), 0036 (pane title, unmeasured), 0037 (one shared channel today). Absorbs roadmap step R12 (Web Push and inbox). Not decided yet: nothing here is built until the owner accepts the decisions. This file alone is enough for a cold session.

## Cold start

Rust server `packages/server/src` (`events.rs` hub and sanitiser, `agentwatch.rs` scan and screen watchers, `adapters.rs` parsers, `sessions.rs` the per-connection loop, `rtc.rs` WebRTC, `websocket.rs`, `link.rs` the transport-agnostic JSON `Link`, `backend.rs`, `control.rs`, `claude_hooks.rs`, `main.rs`). Wire types in `packages/core/src/protocol.rs` and `packages/core/src/events.rs`, mirrored by hand in `packages/client/src/lib/protocol.ts`. Svelte 5 client `packages/client/src` (`lib/agents.ts` store, `lib/notify.ts`, `lib/cards.ts`, `lib/rtc.ts`, `lib/transport.ts`, `SessionCard.svelte`, `TabStrip.svelte`, `NeedsYouBanner.svelte`, `lib/pwa.ts`). Tests: `cargo test --workspace`; `cd packages/client && npx --yes pnpm@9 test`. One squashed PR per step from its own branch off master, green CI, merge only on the owner's "merge" (`changes-land-as-one-squashed-pull-request`, `no-commits-on-the-default-branch`); commit subject `type(scope): ...` with a one-word scope. 512 MB RAM, 1 vCPU. The owner tests on an iPhone. Never touch the owner's own `termenbox.service`. Plans are tracked in GitHub issues (0049); ask before writing to the tracker.

## Goal

Make "what is each agent doing, what is it working on, and does it need me" correct, fast and notified: a server-derived Activity and Attention per session, carried on its own channel apart from terminal bytes, with Web Push on an Attention edge.

## Design in one place (from 0095-0097)

- Activity `working | blocked | idle | unknown` with `task { title, started_at, source }`; sources by precedence hook > screen > scan; 60 s of silence while `working` decays to `unknown`.
- Attention `needs_input(prompt) | finished_unseen | failed`; `needs_input` clears only by fact; the other two clear on `ack` from a foreground view or a dismiss; scan and screen only hint (low confidence, never pushed).
- Channels `term` (binary) and `status` (JSON) on one peer connection; WebSocket uses a lane tag; `AuthOk.channels` negotiates; absence means the old single pipe.
- Push on a rising edge when no device shows the session in the foreground, 5 s grace, payload names no agent text.

Legend: `[ ]` pending, `[x]` done, `[!]` blocked.

## Steps

### A.1 Measure the pane title and the status wait — [x] (measured with Claude Code 2.1.300 and Codex 0.162.1: the pane title is not a task source; the status wait is in A.4)
Class: research · Needs: none · Check: `test -s docs/facts/0036-pane-title-as-a-task-source-is-unverified.md` shows `last-verified:` with a date.
Context: Facts 0036 and 0037 say two things are unmeasured. Use a throwaway tmux server (`tmux -L scratch`), never the owner's sessions.
Do: 1. Run Claude Code (and Codex if installed) in a scratch tmux, record `tmux list-panes -a -F '#{pane_title}'` while idle, working on a prompt, and at a permission prompt; save samples under `packages/server/testdata/titles/`. 2. In a debug build with a loopback client, flood `Output` while sending an agent event and record the delay of the event. 3. Update facts 0036 and 0037 with the numbers and set `last-verified`.
Done-when: fact 0036 states what each tool writes to the pane title (or that it writes nothing) and fact 0037 has a measured delay; both files have a real `last-verified` date.

### A.2 Core types: Activity, Attention, task, ack — [x] (committed b5cb2c8 on feat/status-core-types, PR pending)
Class: implement · Needs: none · Check: `cargo test -p termenbox-core`
Context: `packages/core/src/events.rs` has `AgentState`, `SessionAgentState`, `EventKind`; every enum has a `serde(other)` `Unknown`.
Do: 1. Add `Activity { Working, Blocked, Idle, Unknown }`, `AttentionKind { NeedsInput, FinishedUnseen, Failed }`, `Task { title, started_at, source }` and an `Attention { id, session, kind, raised_at, task, prompt }`. 2. Add `ServerMessage::Activity`/`Attention` and `ClientMessage::Ack { attention_id }`/`Dismiss`; keep `AgentSnapshot`/`AgentEvents` for old clients. 3. Mirror in `packages/client/src/lib/protocol.ts`.
Done-when: round-trip tests pass with wire names; an old-format snapshot still parses; an unknown Attention kind is ignored, not fatal.

### A.3 Server: derive Activity and Attention — [x] (no pane-title task source, see A.1; `stale` stays on the legacy snapshot for old clients until A.5)
Class: implement · Needs: A.2 · Check: `cargo test -p termenbox-server events scan`
Context: `events.rs` hub currently stores the latest state per session with `stale`; `agentwatch.rs` posts scan and screen events; `claude_hooks.rs` posts hook events.
Do: 1. In `events.rs` derive Activity per session by source precedence hook > screen > scan with a 60 s decay to `unknown` (remove the `stale` logic once clients use `unknown`). 2. Raise Attention on transitions: hook `needs_input` or a confirmed prompt -> `needs_input`; working -> idle by hook `Stop` -> `finished_unseen`; error -> `failed`. Scan and screen raise nothing but a low-confidence hint field. 3. Keep per-session `task`: from `UserPromptSubmit` (first line, plan item when present), else `#{pane_title}` added to the `list-panes` format in `agentwatch.rs` (only if A.1 found it useful), sanitised (control chars stripped, 120 cap, scrubber). 4. Store Attention items (cap 200, memory only) and clear by fact or by `ack`/`Dismiss`; keep audit entries to session, kind and source only.
Done-when: unit tests prove a scan event cannot overwrite a hook's `needs_input`; silence decays `working` to `unknown`; an `ack` clears `finished_unseen` but not `needs_input`; a title with control characters and a token is sanitised; the 20-session load test still passes within budget.

### A.4 Two channels: server and client — [x] (channel `t` id 0 stays the JSON channel, new `term` id 1 carries bytes; the browser throughput gate of `e2e/run.mjs` times out here with master's server too)
Class: implement · Needs: A.1, A.2 · Check: `cargo test -p termenbox-server rtc e2e && cd packages/client && npx --yes pnpm@9 test`
Context: Today one negotiated data channel carries JSON (`rtc.rs:322`, `rtc.ts:90`); `link.rs` is one JSON stream; `Output { data }` is base64.
Do: 1. Server `rtc.rs`: open a second negotiated channel (`status`); route `Output` and input as binary on `term` and everything else on `status`, each with its own bounded queue; under pressure coalesce `term`, never delay `status`. 2. `websocket.rs`: one socket, a one-byte lane tag per frame. 3. Add `channels` to `AuthOk`; a server without it, or a client that does not ask, keeps the single pipe. 4. Client `rtc.ts` and `transport.ts`: open both, fall back to the single pipe when `channels` is absent. 5. Update `packages/server/src/e2e.rs` for both paths.
Done-when: an e2e test shows an `Activity` message arriving within the A.1 target while a burst of output is flowing; an old server (no `channels`) still works in a client test; a new server with an old client still works; WebSocket carries both lanes.

### A.5 Client: store, cards, tabs and banner read Activity and Attention — [x]
Class: implement · Needs: A.3, A.4 · Check: `cd packages/client && npx --yes pnpm@9 test`
Context: `agents.ts` holds a flat state and recomputes "needs you" in `tabs.ts` `needsYou`; `notify.ts` builds the title count and badge; `SessionCard.svelte` renders cards.
Do: 1. `agents.ts`: store Activity, `task` and Attention items; running time from `started_at` plus the clock offset. 2. Cards show the task title and running time; `needsYou`, `waitingList`, the title count, the app badge and `NeedsYouBanner.svelte` read Attention. 3. Send `ack` when a session is shown in the foreground and the page is visible; dismiss action on an item. 4. Drop `stale` handling in favour of `unknown`.
Done-when: unit tests cover ack on foreground only when visible, running time with a clock skew, `needs_input` not cleared by viewing; a card shows the title and time in a browser e2e (`packages/client/e2e/cards.ts`).

### A.6 Web Push and inbox (absorbs R12) — [x] (the key is created by `termenbox-server --setup-push`, never shown in the app; one subscription per service worker, so a phone follows one server)
Class: implement · Needs: A.3, A.5 · Check: `cargo test -p termenbox-server push && cd packages/client && npx --yes pnpm@9 test`
Context: Decision 0061/0095, 0080 (VAPID key from a root-owned file), guardrail `notifications-carry-no-agent-text`. Web Push on iOS needs the installed PWA (16.4+). No push code exists yet.
Do: 1. Load the VAPID key per 0080 (the app shows the one line to add, and the public key). 2. Store a subscription per device, remove it on a 404 or 410. 3. On an Attention rising edge, wait 5 s; send unless a connected device reports the session in the foreground; one push per edge, replacing the older by tag. 4. Payload: session display name, kind and time only. 5. Inbox in the client with unread badge, bounded to 200, fed from Attention; works after an app restart.
Done-when: a fake push endpoint receives a payload with no marker text from the title or prompt (the guardrail's test); an expired subscription is removed; a viewing device suppresses the push; the inbox renders after a restart.

### A.7 Remove herdr — [x] (the only remaining `herdr` string is the refusal message of `TERMENBOX_BACKEND=herdr`)
Class: implement · Needs: none (can run first) · Check: `cargo test --workspace && cd packages/client && npx --yes pnpm@9 test`; `grep -rli herdr packages README.md AGENTS.md CLAUDE.md` is empty.
Context: Decision 0097. Footprint: `packages/server/src/{adapters,agentwatch,backend,control,events,main,sessions}.rs`, `packages/core/src/{events,protocol}.rs`, `packages/client/src/{lib/agentIcons,lib/protocol,lib/windows,lib/terminal/inputBatch,lib/terminal/touchScroll,SessionActions,Terminal,WindowMenu}`, `packages/client/e2e/touch.mjs`, `README.md`, `AGENTS.md`, `CLAUDE.md` (the "Codex, Claude Code, Herdr" line, kept within the 150-line cap), `docs/plans/roadmap/R24.md`.
Do: 1. Delete the herdr poller, `HerdrBackend`, `TERMENBOX_BACKEND=herdr` and the `auto` probe; keep `CommandBackend` generic. 2. Remove the `Source::Herdr` variant (keep deserialising an unknown source as `Unknown`) and the client icon and menu entries. 3. Update README and agent lists. 4. Remove the herdr line from R24.
Done-when: the grep is empty apart from `docs/plans/archive/` and decision/fact history; all tests pass; `TERMENBOX_BACKEND=herdr` fails with a clear message.

### A.8 Accept the decisions and close out — [x] (0080, which the push key relies on, is still draft)
Class: process · Needs: A.3-A.7 · Check: `lint` reports no dangling references.
Do: 1. With the owner, set 0095-0097 to `active` with `accepted-by`; mark 0061 `superseded` with `superseded-by: 0095-agent-status-is-activity-plus-attention`; remove the draft notes from 0054 and 0085. 2. Update `docs/decisions/INDEX.md`. 3. Set R54 and R12 to `done (PR #n)` and run `python3 docs/plans/roadmap/rank.py`.
Done-when: no decision named here is `draft`, 0061 shows superseded in its file and the index, and `grep -l '^Status: done' docs/plans/roadmap/R54.md docs/plans/roadmap/R12.md` lists both (R13, R16 and R40 depend on R12, so it must be marked done, not deleted).
