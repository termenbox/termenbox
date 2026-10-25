# Workspaces and one protocol for every agent window (ACP v1 core plus extensions)

Status legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved. `‖` after the marker means the step can run in parallel. Update the marker in place.

## Cold start (read this first)

Termenbox is a web terminal for managing remote AI coding agents (Rust server `packages/server`, shared types `packages/core`, Svelte 5 client `packages/client`). Read `CLAUDE.md` for the rules. Branches are cut from `develop`, PRs target `develop`, one squashed PR per step with green CI, commit subjects are `type(scope): summary` with `Why:`/`What:` bodies and no attribution lines. No compiler on this machine: run everything through `scripts/dev.sh` (decision 0114), e.g. `scripts/dev.sh cargo test --workspace` and `scripts/dev.sh pnpm -r test`. Ask the owner before merging.

**What this plan does.** Today an agent is integrated three ways and the client has two render paths:

- ACP: the server spawns an agent as a stdio child (`packages/server/src/acp.rs`, `acp_stream.rs`) and the client draws `AcpView.svelte` from `AcpView`/`AcpEntry` snapshots (`packages/client/src/lib/acpStream.ts`).
- Hooks and scan/screen for an agent TUI in tmux: events go through `adapters.rs` and `events.rs` into `SessionViewMessage`, drawn by `SessionView.svelte`.
- The unit users manage is a "session" (tmux session or ACP entry), one agent each.

**Target (owner's decisions, all `active`):**

- Decision 0136: the wire protocol is ACP v1 verbatim plus a namespaced, negotiated, ignorable **extension registry**; three built-in **adapters** (native ACP, hook, scan/screen) and optional out-of-process stdio adapters sit behind one port; protocol, adapter, transport, window host and client renderer are each replaceable (SOLID). The browser loads no dynamic code.
- Decision 0138: a **workspace** (the old "session") is a sandbox of **windows** (agent or terminal) with a focus pointer; a **session** is now one agent conversation, one ACP `sessionId`; a workspace can hold several agent windows, each addressable. tmux stays under the workspace. The rename is complete at once, with a legacy wire shim for one release window.
- Decision 0139: three channels (control, terminal bytes keyed by `windowId`, agent events keyed by `sessionId`); control never waits behind bulk traffic; events replay from a per-session `seq` log with a compacted `reset` fallback; memory is a budget measured in CI, not a hard 512 MB fit.
- Guardrails `protocol-core-stays-acp-v1`, `control-never-waits-behind-bulk`, `resource-constraints` (amended).
- Related, read when a step names them: 0054 (session backends), 0095 (status), 0096 (two data channels), 0103 (tiers), 0111 (ACP event model and quirk profiles), 0122 (reducer), fact 0040 (ACP measured per agent), skills `add-an-agent-profile`, `probe-an-agent-over-acp`.

**Prerequisite plan.** `docs/plans/acp-session-modes.md` landed in PR #313 (decision 0137): the schema crate (`agent-client-protocol-schema`), built-in `claude`/`opencode` profiles, `on_path`, `DEFAULT_MAX_SESSIONS = 6`, the Terminal/Agent View choice and restore hardening are on `develop`, so Phase 0 is done. Two things stay open in roadmap step R74: built-in ACP agents are **off** by default (`TERMENBOX_ACP=off`) because the composite backend prefixes every id once `acp` sits next to `tmux`, and the owner has not settled the adapters' terms of use. Step 1.1 and 3.1 must account for the id prefixing (stable workspace ids). In that plan "session" still means the old thing; Phase 1 renames it.

---

## Phase 0: prerequisites (plan `acp-session-modes`, done in PR #313)

### 0.1 Move the ACP client onto the schema crate — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh cargo test --workspace`
Context: Step 0 of `docs/plans/acp-session-modes.md`. `packages/server/src/acp.rs` builds `json!` requests and reads `v["a"]["b"]`; fixtures live in `packages/server/testdata/acp/<agent>/<scenario>.jsonl`. The crate `agent-client-protocol-schema` (types and serde only, `default-features = false`, protocol v1) becomes the typed ACP core shared with the client later (decision 0136).
Do: Follow step 0 of the plan exactly: add the dependency, replace the `json!` requests and field reads for `initialize`, `session/new|load|resume|prompt|cancel|set_mode|set_config_option`, `session/request_permission` and the `session/update` kinds; keep the stdio transport, caps, sanitising and lenient per-message decoding; keep a golden test comparing old and new mapper `EventKind` streams over every fixture; record release binary size and crate count before and after in the PR; stop and ask the owner if the binary grows over 1 MB or the tree over 20 crates.
Done-when: `scripts/dev.sh cargo test --workspace` passes with the existing `acp` tests unchanged and the golden test green, and the PR states the size and crate-count delta.

### 0.2 Built-in ACP agents without hand-written config — [x]
Class: implement · Needs: 0.1 · Check: `scripts/dev.sh cargo test --workspace`
Context: Step 1 of the plan. Owner sign-off on the adapters' terms of use (choice A) is needed before merge; the agent writes a one-paragraph summary of the primary text into the PR and does not decide.
Do: In `packages/server/src/main.rs` add `TERMENBOX_ACP` (`on` default, `off` kill switch) and `resolve_backends(spec, acp_on)`; in `acp.rs` add `AgentProfile.elicitation_form`, `builtin_agents()` (`claude`, `opencode`), `AcpConfig.builtin`, `DEFAULT_MAX_SESSIONS = 6`, `on_path`, installed-only `acp_agents()`, the `create_with` "not installed" error and the login-failure message; update comments and tests as the plan lists.
Done-when: the plan's step-1 tests pass; with no config file and a fake `claude-agent-acp` on `PATH`, the hello lists `claude` in `acp_agents`; with `TERMENBOX_ACP=off` the `acp` backend is absent.

### 0.3 Dialog: choose the view per agent — [x]
Class: implement · Needs: 0.2 · Check: `scripts/dev.sh pnpm -r test`
Context: Step 2 of the plan. `packages/client/src/NewSessionSheet.svelte`, `packages/client/src/lib/newsession.ts`. The control is called **View** (Terminal | Agent view), never "mode".
Do: Implement the plan's step 2: `view` as source of truth, derived `backend`, the segmented control shown only when the template is in `server.acpAgents` and `tmux` is available, default Terminal, remembered per `serverId + template` with guarded storage.
Done-when: component tests cover every case in the plan plus default Terminal, a remembered choice and throwing storage; `scripts/dev.sh pnpm -r test` passes.

### 0.4 Harden restore of ACP sessions — [x]
Class: implement · Needs: 0.1 · Check: `scripts/dev.sh cargo test --workspace`
Context: Step 3 of the plan. `Saved`, `Entry`, `save`, `restore`, `stream_view`, `create_with` in `packages/server/src/acp.rs` and `status` in `acp_stream.rs`. This is the base for restoring agent windows in Phase 3. Touches the same file as 0.2, so do not run in parallel.
Do: Implement the plan's step 3: persist entries without a live agent with `reason`/`retry`/`turn_open`, the cap, retry, show a failure rather than "Done", the interrupted-turn notice and one Failed attention item, and the restore versus create race placeholder.
Done-when: the plan's step-3 tests pass (cap exceeded keeps entries across `save`/`restore`; failed start retried and reports `Error`; invalid dir kept with a reason; create during restore respects cap and name; `turn_open` gives the notice and one attention item; an old `acp-sessions.json` loads).

### 0.5 Prerequisite docs and knowledge — [x]
Class: implement · Needs: 0.2, 0.3, 0.4 · Check: `python3 docs/plans/roadmap/check.py`
Context: Step 4 of the plan. Decisions 0136–0139 and the `resource-constraints` amendment already exist (written by the roadmap interview of 2026-10-25), so do not duplicate them.
Do: Write the one decision that remains from the plan (view chosen per agent, restart survival is resume with the mid-turn loss visible, cap 6, schema-crate adoption, amends 0111 allowlist), update `docs/guide/sessions.md` and `docs/guide/configuration.md` as step 4 lists, add its roadmap step, commit `docs/plans/acp-session-modes.md` with the PR.
Done-when: `python3 docs/plans/roadmap/check.py` and the knowledge-base lint are clean and the guide pages describe every key the plan names.

---

## Phase 1: the rename

### 1.1 Rename session to workspace across code, wire, UI and docs — [ ]
Class: implement · Needs: none · Check: `scripts/dev.sh sh -c 'cargo test --workspace && pnpm -r test'`
Context: Decision 0138. Stable ids: with `acp` registered next to `tmux` the composite backend prefixes ids (`tmux:<name>`), see R74; decide here whether workspace ids stay unprefixed for the first manager, so the rename does not orphan saved ids. About 156 source files mention "session". The rename is mechanical, **no behaviour change**, one PR, landed before any protocol work so it cannot conflict with it. Only termenbox's own sandbox is renamed to **workspace**; the word "session" stays and now means an agent conversation (ACP `sessionId`) only where the code already means that (`acp_id`, ACP `session/*` method names, `sessionId` fields). Do not rename ACP protocol method or field names, tmux's own terms, or the `SessionBackend` trait's tmux calls where they mean a tmux session; rename the trait to `WorkspaceBackend` (decision 0054 stays valid).
Do: (1) List every occurrence with `grep -rniw session packages docs scripts` and classify each as workspace, conversation or tmux/ACP-owned. (2) Rename types, fields, wire message names and TypeScript names for the workspace meaning (for example `SessionList` → `WorkspaceList`, `new_session` → `new_workspace`, `sessions.rs` → `workspaces.rs`), UI copy, test ids and `docs/guide`. (3) Keep reading the old on-disk state names (`acp-sessions.json` and any session state file) and write the new ones; add a test that loads an old file. (4) Do not add the legacy wire shim here: the old wire names are kept untouched until step 3.2, so in this PR only internal names change and the wire message strings change in 3.2 together with version negotiation. Record this split in the PR. (5) Update `CLAUDE.md` terms if they name the old meaning.
Done-when: `scripts/dev.sh sh -c 'cargo test --workspace && pnpm -r test'` passes; a grep for the old workspace meaning (`grep -rniw 'session' packages --include=*.rs --include=*.ts --include=*.svelte`) shows only conversation, ACP-owned, tmux-owned uses, or the wire message strings deliberately left for step 3.2, all listed in the PR; an old `acp-sessions.json` still loads.

---

## Phase 2: the protocol and its ports

### 2.1 Verify ACP's extension rules and the schema crate's coverage — [ ]‖
Class: explore · Needs: none · Check: `test -f docs/facts/0057-acp-extension-rules-and-schema-crate-coverage.md`
Context: Decision 0136 builds the extension registry on ACP's own extensibility (`_meta` fields and custom methods). That is not yet verified against the specification and the pinned schema crate, and the registry's shape depends on it. Read-only research; the output is a fact. The next free fact number may differ: check `docs/facts/` first and adjust the file name in the check.
Do: Read the ACP specification's extensibility section and the `agent-client-protocol-schema` source (pin the version the repo uses after step 0.1, or 1.12 before it lands). Establish: how `_meta` is reserved and namespaced; how custom methods and notifications are named and what a peer that does not know them must do; how capabilities are advertised in `initialize` for extensions; whether the schema types let an extension ride in `_meta` without losing it on a round trip; which of `session/update` kinds, content blocks and terminal types the schema crate covers; and anything in ACP v1 that conflicts with the extension rule "unknown is skipped". Write a fact with sources, the version and date, and list conflicts.
Done-when: the fact exists in `docs/facts/`, has its row in `docs/facts/INDEX.md`, and answers each question above or says it could not be established.

### 2.2 Define the protocol crate and the ports — [ ]
Class: implement · Needs: 1.1, 2.1 · Check: `scripts/dev.sh cargo test --workspace`
Context: Decision 0136. Shared types are in `packages/core/src/protocol.rs` (mirrored by hand in `packages/client/src/lib/protocol.ts`). The ACP v1 core comes from the schema crate (step 0.1). Ports must make the protocol, adapter, transport, window host and renderer replaceable.
Do: In `termenbox-core` add the extension registry: namespace, version, capability negotiation at `initialize`, a typed wrapper that carries extensions in `_meta` and custom methods per fact from 2.1, unknown-extension skipping, and the protocol version number. In `packages/server` define traits: `AgentAdapter` (agent window ↔ protocol, states its tier), `WindowHost` (create, list, focus, close, resize windows; implemented later by tmux and by the ACP child), `Transport` (send on control, bytes and events channels). Do not wire anything in yet. Mirror the TypeScript types and add a parity test between Rust and TS shapes in the style `packages/client/src/protocol.test.ts` already uses.
Done-when: unit tests show an unknown extension is skipped in both directions, a known one round-trips, the ports have a fake implementation used by a test, and the TS parity test passes.

### 2.3 Conformance harness — [ ]
Class: implement · Needs: 2.2 · Check: `scripts/dev.sh cargo test --workspace -- conformance`
Context: Guardrail `protocol-core-stays-acp-v1` is enforced by CI. Fixtures are in `packages/server/testdata/acp/`.
Do: Add a test (name containing `conformance`) that replays every fixture through the server adapter, strips all extensions, and validates each message against the schema crate. Add a client test (`packages/client/src/lib/*.test.ts`) that feeds every standard `session/update` kind and an unknown extension and content type to the client reducer and asserts a rendered or generic result, never an exception. Step 5.1 makes the client side non-trivial; here the test is written against the current ACP reducer and extended there.
Done-when: both tests exist and pass, and temporarily changing a core shape makes the server test fail.

---

## Phase 3: workspace, wire and channels

### 3.1 Workspace and window model on the existing hosts — [ ]
Class: implement · Needs: 1.1, 2.2 · Check: `scripts/dev.sh cargo test --workspace`
Context: Decision 0138. `packages/server/src/workspaces.rs` (renamed in 1.1), `backend.rs` (tmux and ACP backends behind one trait), `acp.rs`. A workspace has windows of kind `agent` or `terminal` and a focus pointer (default: the base agent window). Today a session is one agent; that becomes a one-agent-window workspace.
Do: Add `Window { id, kind, host, session_id? }` and `focus` to the workspace; persist the layout (windows, kinds, focus, directory) next to the saved ACP state; implement `WindowHost` for tmux (windows or panes) and for the native ACP child (an agent window with no pane); on restart, agent windows resume per step 0.4 and terminal windows reattach by tmux. Existing single-agent workspaces load as one agent window.
Done-when: tests show a workspace with two windows persists and reloads, focus survives a restart, an old single-agent state loads as one agent window, and a window can be addressed by id on both hosts.

### 3.2 Version negotiation and the legacy shim — [ ]
Class: implement · Needs: 2.2, 3.1 · Check: `scripts/dev.sh cargo test --workspace`
Context: Decision 0138 compatibility. The hosted client can be newer than a self-hosted server and vice versa. The hello is built in `packages/server/src/connection.rs` (`acp_agents` is already in it). Wire message names still use the old words (step 1.1 deliberately left them).
Do: Add the protocol version to the hello on both sides. A new server answers a client that announces no version, or an old one, with the legacy messages (`session*`, `acp_view`, `SessionViewMessage`) produced from the workspace model in one module `legacy.rs`; a new client against an old server falls back to the legacy view. Rename the wire messages to the workspace vocabulary for new clients. Mark the shim module and its tests so step 8.1 can delete them by name.
Done-when: e2e tests (`packages/server/tests/e2e.rs`, `packages/client/e2e/`) show new↔new, old client→new server and new client→old server (a recorded old server hello) all connect and show a session list; `legacy.rs` is the only place with the old wire names.

### 3.3 Control channel with priority — [ ]
Class: implement · Needs: 3.2 · Check: `scripts/dev.sh cargo test --workspace -- control`
Context: Decision 0139 and guardrail `control-never-waits-behind-bulk`. Transport code: WebRTC data channel and WebSocket fallback in `packages/server` (transport files) and `packages/client/src/lib/connection.ts`. WebSocket is one TCP stream, so priority is an application-level queue.
Do: Move cancel, interrupt, kill, permission answers, prompt submit, focus and window create/close/resize and workspace administration to a control channel: its own ordered reliable WebRTC data channel, and on WebSocket a control queue drained before event and byte frames, with a maximum bulk frame size so a control message waits at most one frame. Acknowledge every control message with a result. Keep the existing terminal byte path (0096) addressed by `windowId`.
Done-when: a test fills the event and byte queues and shows a control message written before the next bulk frame on the WebSocket path; an e2e test cancels during a large output flood within a bound taken from a measured baseline and recorded in the test; acks carry success or an error.

### 3.4 Event channel: sequence log and compacted replay — [ ]
Class: implement · Needs: 2.2, 3.2 · Check: `scripts/dev.sh cargo test --workspace -- replay`
Context: Decision 0139. Today `acp_stream.rs` builds `AcpView` snapshots with `rev`/`epoch`/`first`; the new wire sends standard `session/update` notifications. The ring is a byte budget per window, configurable.
Do: Stamp every outbound event with per-session `seq` and the server `epoch` (extension). Keep a bounded ring per session. Handle "resume from seq N": replay the tail; if N is not in the ring or the epoch differs, send a compacted replay (chunks merged, tool calls collapsed, still standard updates) flagged `reset`. Make the ring size a config value with a measured default and expose its current bytes in the metrics. Implement on the client in step 5.1.
Done-when: tests show gapless resume mid-turn, resume after the ring wrapped giving a `reset` replay that reconstructs the same state as the full stream, an epoch change giving `reset`, and the ring never exceeding its byte budget.

---

## Phase 4: adapters

### 4.1 Native ACP adapter emits the protocol directly — [ ]
Class: implement · Needs: 3.1, 3.4 · Check: `scripts/dev.sh cargo test --workspace`
Context: `packages/server/src/acp.rs` and `acp_stream.rs` today map ACP updates to `AcpEntry` and `AgentEvent`. With ACP as the wire the update passes through nearly unchanged; the hub still gets `EventKind` (0111, 0122), and untrusted agent text is still sanitised (0095).
Do: Implement `AgentAdapter` for the native ACP child: forward `session/update` and requests addressed by `sessionId` with `seq`, tier `t0`, window and focus extensions; keep the `EventKind` mapping for the hub; stop building `AcpEntry` on the server (the client reducer in 5.1 owns presentation). Keep per-agent quirk profiles (0111).
Done-when: the golden tests from 0.1 still pass, the conformance test (2.3) is green on all fixtures, and attention, push and Home behave as before (existing `events` tests unchanged).

### 4.2 Hook adapter (Claude, Codex, OpenCode) emits the protocol — [ ]
Class: implement · Needs: 4.1 · Check: `scripts/dev.sh cargo test --workspace -- adapters`
Context: `packages/server/src/adapters.rs` maps hook events to `AgentEvent`. Fixtures: `packages/server/testdata/events/`. For an agent TUI in a PTY window the adapter must produce the same protocol at tier t1 or t2 and never invent content.
Do: Map hook events to standard updates: `UserPromptSubmit` → user message chunk and turn start, `PreToolUse`/`PostToolUse` → `tool_call` and `tool_call_update` (with diffs only when the hook payload carries them), `PermissionRequest` → `session/request_permission` with `allow_once`/`reject_once` only (0104), `Stop` → the last agent message and stop reason, `StopFailure` → an error notice; state the tier in the extension. Decide per the open question whether a `tool_output` field is standardised or extraction stays adapter-specific, and write the decision into the PR.
Done-when: fixture-driven tests show each hook event gives the expected updates, no update contains content absent from the payload, and the conformance stripping test is green for hook sessions.

### 4.3 Scan/screen adapter emits the protocol — [ ]
Class: implement · Needs: 4.2 · Check: `scripts/dev.sh cargo test --workspace -- screen`
Context: `packages/server/src/profile.rs`, `packages/server/profiles/*.toml`, screen fixtures `packages/server/testdata/screens/`; tier t3 (0103). It shares `adapters.rs` with 4.2, so it follows it.
Do: Emit state notices as agent message or status updates and heuristic prompts as `session/request_permission` with the options the screen showed (Yes/No, numbered); tier `t3` in the extension; answers still go back only as the keys the engine derived from the screen.
Done-when: screen fixtures give the expected updates and keys, a screen where nothing should match gives none, and the conformance stripping test is green.

### 4.4 Out-of-process adapters over stdio — [ ]
Class: implement · Needs: 4.1 · Check: `scripts/dev.sh cargo test --workspace -- external_adapter`
Context: Decision 0136 pluggability. The command allowlist is server-side only (0111): the client never supplies a command.
Do: Declare an external adapter in the server-side profile file (command, args, env, tier). The server spawns it and speaks the protocol (ACP v1 core plus extensions) over stdio, using the same child handling as the native ACP child. Include a fake adapter under `packages/server/testdata/` for tests. Write `docs/guide` text and extend skill `add-an-agent-profile` check list if it changed.
Done-when: a test spawns the fake adapter, drives a turn and a permission through it, and shows an unknown extension from it is skipped; a profile entry with a relative or client-supplied command is rejected.

---

## Phase 5: the client

### 5.1 One reducer and a renderer registry over the protocol — [ ]
Class: implement · Needs: 2.3, 3.4 · Check: `scripts/dev.sh pnpm -r test`
Context: Client files: `packages/client/src/lib/acpStream.ts`, `acpStore.ts`, `sessionView.ts`, `AcpView.svelte`, `AcpEntryView.svelte`, `SessionView.svelte`. Today `AcpView.svelte` renders `AcpEntry` and falls back to `SessionViewMessage` for other sources.
Do: Write a reducer `applyUpdate(state, update)` over standard `session/update` and requests plus the replay rules of 3.4 (`seq`, `reset`, epoch) and bounded memory (the limits `acpStream.ts` has today). Add a renderer registry keyed by content or extension type, built-ins only, with a generic fallback for unknown types. `AcpView.svelte` renders from it for every tier and shows a tier badge from the extension. Keep search, diffs, copy and permissions working. Keep the legacy path only behind the shim (3.2).
Done-when: tests cover every standard update kind, an unknown extension and content type, resume and `reset`, and bounded memory; the conformance client test of 2.3 is green; `AcpView` renders a hook-tier and a screen-tier fixture with no `SessionView` involvement.

### 5.2 Workspace and window UI — [ ]
Class: implement · Needs: 5.1, 3.3 · Check: `scripts/dev.sh sh -c 'cd packages/client && node e2e/run.mjs'`
Context: Decision 0138/0139. Existing: `TabStrip.svelte`, `WindowMenu.svelte`, `SessionList.svelte` (renamed), `NewSessionSheet.svelte`. UX rules: guardrails `ui-never-moves-under-the-finger`, `soft-keyboard-never-hides-input`, `errors-fit-one-screen`.
Do: Show a workspace's windows and focus; open any window by id; for an agent window offer the conversation view and, for a PTY-backed one, the raw terminal view over the byte channel; create and close windows through the control channel with its acknowledgements and visible errors.
Done-when: e2e on the fake agent shows a workspace with an agent window and a terminal window, switching between them, and for a PTY agent window switching between conversation and raw terminal; a control error shows in one screen.

---

## Phase 6: several agent windows

### 6.1 Multiple addressable agent windows in one workspace — [ ]
Class: implement · Needs: 5.2, 4.1 · Check: `scripts/dev.sh sh -c 'cd packages/client && node e2e/run.mjs'`
Context: Decision 0138: each agent window is one ACP `sessionId`; the stream is multiplexed by it and permissions, cancel and prompts address a window.
Do: Allow creating a second (and further) agent window in a workspace, with its own agent and view; route events, prompts, cancels and permission answers by `sessionId`/`windowId`; show per-window status and attention (0095, 0122) in the workspace; the cap `max_sessions` counts agent children.
Done-when: e2e with two fake agents in one workspace shows independent turns, a permission answered for the right window only, a cancel reaching only its window, and the cap refusing the seventh child with a readable error.

---

## Phase 7: budgets

### 7.1 CI-measured budgets for the new paths — [ ]
Class: implement · Needs: 3.3, 3.4 · Check: `scripts/dev.sh sh -c 'cargo test --workspace -- budget'`
Context: Amended guardrail `resource-constraints` (reference envelope plus per-feature budgets) and decision 0139. Existing load script: `scripts/` and `docs/guardrails/resource-constraints.md` enforcement section.
Do: Measure and record budgets for server RSS per idle and per active window, ring bytes, replay encode time for a long session, fan-out CPU for several clients, and control latency under flood. Fail CI on regression beyond a stated tolerance; the defaults come from measurement, written into the guardrail table.
Done-when: a CI job reports the numbers, the guardrail table lists them, and a deliberate regression in a test branch fails the job.

---

## Phase 8: close out

### 8.1 Remove the legacy shim after the release window — [ ]
Class: implement · Needs: 6.1, 7.1 · Check: `scripts/dev.sh sh -c 'cargo test --workspace && pnpm -r test'`
Context: Decision 0138: the shim lives for one release window. The owner decides when the window ends; ask before starting. `legacy.rs` (3.2), `SessionViewMessage`, `acp_view`, `SessionView.svelte`, and the old-server fallback in the client are the targets.
Do: Delete the shim, the legacy messages and components, and their tests; the hello rejects a client with no version with an "update required" message that fits one screen.
Done-when: a grep for the legacy names finds nothing outside CHANGELOG and docs history; tests pass; an old client gets the update message.

### 8.2 Documentation and knowledge base — [ ]
Class: implement · Needs: 6.1 · Check: `python3 docs/plans/roadmap/check.py`
Context: The guides describe the old model: `docs/guide/sessions.md`, `docs/guide/configuration.md`.
Do: Rewrite the guides for workspaces, windows, agent windows and sessions as conversations; update facts 0038 and 0040 if the adapter measurements changed; add the tag `workspace` to `docs/skills/tags.md` if used; extend skill `add-an-agent-profile` with the final adapter checklist; run the knowledge-base lint.
Done-when: `python3 docs/plans/roadmap/check.py` and the knowledge-base lint are clean and the guides use the new vocabulary throughout.
