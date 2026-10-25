# Start an agent in a terminal view or in termenbox's agent view (ACP)

(File name kept; "view" is used below because ACP has its own, unrelated "session modes", and `AcpView.svelte`
already has a session-mode select labelled "Mode".)

## Context

Today an agent starts one way by default: the agent's own TUI typed into a new tmux session. The ACP view exists
(server spawns the agent, web client draws `AcpView`) but is hidden: `TERMENBOX_BACKEND` defaults to `auto`
(= tmux only), `acp-agents.toml` has to be written by hand, and the dialog only shows a "Manager" dropdown when
the server runs more than one backend. The goal is to make both views a normal, visible choice per agent.

Fixed earlier with the owner:
- **No tmux pane and no TUI process for ACP sessions.** The web ACP view is the termenbox UI. The server keeps
  owning the agent child (decision 0111 stays).
- **Survival = reconnect + resume.** Clients reconnecting is free. A server restart restarts the agent child and
  `session/resume`s or `session/load`s it from the saved `acp_id`. A turn running during the restart is lost;
  the owner accepts that, and it must be visible (step 3). A detached host process was rejected for now.
- The terminal view keeps working as it does (tmux, hooks/screen tiers T1-T3).

## Decisions (all confirmed by the owner)

| | Decision | Rejected, and why |
|---|---|---|
| A | **Built-in agents are on by default** (`TERMENBOX_ACP=on`, `off` is the kill switch). The 0111 gate on adapter terms of use is an owner sign-off before merge; if it fails, flip the one default to `off`. Amends 0111: the allowlist is "built-ins plus the owner's file". | Opt-in by env (feature stays hidden, defeats the goal); opt-in from the dialog (new owner-only wire message). The authenticated owner can already run any shell via tmux, so a fixed built-in command adds no reach. |
| B | **Adapters are not shipped in the image.** The owner installs them in the container (`npm i -g @agentclientprotocol/claude-agent-acp`); availability is checked live, per `acp_agents()` call, so an install shows up without a restart. | Baking into `docker/Dockerfile.base` (image size, release tied to adapter versions, terms for everyone); `npx -y` in the profile (network at first start, runs whatever the registry serves). |
| C | **No file = built-ins. A file is exclusive unless it sets `builtin = true`**; a same-named file entry always wins. Existing owners see no change on upgrade. | Always merge (silently adds agents for owners with a restrictive file, no way to remove one); always exclusive (visibility problem stays). |
| D | **Default view is Terminal**; the last choice is remembered per (server, agent). | Default to Agent view (changes what existing users get, loses hooks/screen tiers); one choice per server. |
| E | **A restart that interrupts a running turn is made visible** (notice + one Failed attention item + a guide warning). | Draining open turns on shutdown (Docker stops after 10 s; follow-up); detached host (rejected). |
| F | **`max_sessions` defaults to 6, with no memory floor.** The owner's call: the server must handle 5-6 sessions freely, and anyone who wants more or has less memory sizes the box. `resource-constraints` is amended to cover termenbox's own processes (the server idles near 9 MiB, fact 0047), not agent children (a few hundred MB each, per the comment on `DEFAULT_MAX_SESSIONS`). Accepted risk: six adapters on a 512 MB box can hit the OOM killer; the guide says so and gives a per-session figure. | Keeping 2; a free-memory floor (would refuse starts on small boxes instead of failing late). |
| G | **Adopt `agent-client-protocol-schema`** (types and serde only) for the client; the transport stays ours. | The full `agent-client-protocol` SDK (+37 crates, its own runtime glue beside tokio); staying hand-written (tracks protocol renames by hand; ACP v2 renames fields such as `id` to `configId`). |

Spike behind G (scratch worktree, release profile, nothing committed): the server resolves 315 unique crates today;
the schema crate with `default-features = false` makes it 332 (+17; with default features 339, mostly
`schemars`); the full SDK makes it 352. Build time and peak build memory did not change in a way that matters.
Binary size is unmeasurable until code uses the crate (an unused crate is dropped at link time), so step 0
measures it. The `acp.rs` header's "SDK ... 512 MB" justification never applied.

## Prior art and what we reuse

| Concern | What others do | This plan |
|---|---|---|
| Surviving a restart | ACP leaves id storage to the client; `session/resume` is stable, `session/load` replays history; clients check `initialize` capabilities first. Orchestrators running OpenCode save the session id and re-attach after a restart. | Same. `handshake` in `acp.rs` already does resume, else load, else fail. Step 3 fixes what is lost around it. |
| Remembering sessions | `session/list` (unstable, not on gemini) lists agent-side conversations, not our named, directory-bound handles. | Keep `acp-sessions.json`. Follow-up: a "continue an earlier conversation" picker. |
| Agent-side modes | `configOptions` with `category: "mode"` supersedes `session/set_mode`. | Untouched; our control is **View**, never "mode". |
| A turn surviving a restart | A long-lived agent server (`opencode serve`, `goose serve`, a session daemon) owns the work. | Out of scope; if wanted, reuse the agents' own servers rather than writing a host. |
| Existing parts | `kill()` already closes any entry (ended ones too); `AcpView` already renders `ended` and `failed` with `statusText`; `AttentionKind::Failed` already comes from `AgentState::Error` (`events.rs`); `acp_agents` is already in the hello; the client has guarded `localStorage` helpers (`tabs.ts`). | Reused. No new wire message, no new client store. Only a PATH check is new (nothing in the server resolves commands today). |

## Who does what

- **Owner (before merging step 1):** read the adapters' terms of use from primary text (claude's subscription
  logins are the risk) and say yes or no to choice A; the agent writes the one-paragraph summary of what it found
  into the PR but does not decide. Optional: measure idle and after-a-turn RSS of `claude-agent-acp` and
  `opencode acp` to fill in the guide's per-session figure; without it the guide says "a few hundred MB".
- **Owner (manual check at the end):** a real claude or opencode adapter with a login, created in each view,
  container restarted mid-turn (see Verification).
- **Coding agent:** everything else. Anything needing a login or a real adapter is covered by the scripted fake
  agent in `packages/server/testdata/acp/` instead.

## Changes

Work in the order below. Steps 0 and 1-4 are two PRs (see the end). Each step ends with a **Done when**.

0. **Move the ACP client onto `agent-client-protocol-schema`** (own PR, lands first; no behaviour change)
   (`packages/server/Cargo.toml`, `packages/server/src/acp.rs`, `packages/server/src/acp_stream.rs`)
   - Dependency: `agent-client-protocol-schema = { version = "1.12", default-features = false }` (no `schemars`,
     no `unstable_*` features; protocol v1, as `handshake` sends `protocolVersion: 1`). `elicitation` capability
     types are in the stable v1 module (step 1 needs them).
   - Replace the `json!` requests and the `v["a"]["b"]`/`s()` reads with schema types for `initialize`,
     `session/new|load|resume|prompt|cancel|set_mode|set_config_option`, `session/request_permission` and the
     `session/update` kinds `UpdateMapper` handles. Keep as is: the stdio line transport, `SETUP_TIMEOUT`, length
     and chunk caps, `events::sanitize_kind`, "text from the agent is untrusted" (0095). Sanitising runs after
     decoding, as today.
   - **Lenient decoding.** Agents deviate (fact 0040: copilot's empty `plan`, `_meta` fields, kilo todos as tool
     calls). A message that fails typed decoding is dropped for that message only (counted, logged without
     content), never failing the turn or the session; an unknown update kind stays ignored. Fields we depend on
     stay required; everything else is optional.
   - **Golden check.** Feed every recorded fixture in `packages/server/testdata/acp/<agent>/<scenario>.jsonl`
     through the old and the new mapper before deleting the old one, and assert identical `EventKind` streams;
     keep the comparison as a test.
   - **Feature unification.** The crate turns on `serde_json`'s `preserve_order` and `raw_value` for the whole
     server (key order in every `Value`, an `indexmap` dependency). `directory.rs` signs a hand-joined string, not
     a `Value`, so signatures are unaffected; the full server suite and client e2e catch snapshot-order changes.
   - **Size gate.** Build the release binary before and after and record both and the crate count in the PR. If
     the binary grows by more than 1 MB or the tree by more than 20 crates, stop and ask again.
   - Rewrite the `acp.rs` header comment: types come from the schema crate, the transport is ours (no SDK runtime
     glue beside tokio).
   - **Done when:** `scripts/dev.sh cargo test --workspace` passes with the existing `acp` tests unchanged and the
     golden test green; the PR states the size and crate-count delta.

1. **Make ACP available without hand-written config** (`packages/server/src/main.rs`,
   `packages/server/src/acp.rs`)
   - `main.rs`: new arg `acp: String`, `#[arg(long, env = "TERMENBOX_ACP", default_value = "on")]`, values `on` or
     `off` (anything else is a startup error naming the variable). `resolve_backends(spec, acp_on)`: `auto` gives
     `tmux` plus `acp` when `acp_on`; an explicit `TERMENBOX_BACKEND` list is taken as written. Registering `acp`
     is free (no child starts until a session is created), so it is present even when no adapter is installed;
     usability is decided per call below. Update the `TERMENBOX_BACKEND` help text and the error text at
     `build_backend`.
   - `AgentProfile`: add `#[serde(default)] pub elicitation_form: bool`; `handshake` sets
     `ClientCapabilities.elicitation = Some(ElicitationCapabilities { form: Some(Default::default()), .. })` (serialises as `{"form": {}}`) when it is true (needed for opencode's questions, else
     an unanswered request blocks the turn, decision 0111). Existing files parse unchanged.
   - `fn builtin_agents() -> HashMap<String, AgentProfile>` (fact 0040):
     `claude` = `claude-agent-acp`, `env_remove = ["CLAUDECODE"]` (the maintained
     `@agentclientprotocol/claude-agent-acp`, not the deprecated `claude-code-acp`);
     `opencode` = `opencode` with `args = ["acp"]`, `elicitation_form = true`.
   - `AcpConfig`: add `#[serde(default)] pub builtin: bool`. `AcpConfig::load(path)`: no file -> `agents =
     builtin_agents()`; file -> its entries, plus the built-ins not already named when `builtin = true`.
     `max_sessions` is the file's value, else `DEFAULT_MAX_SESSIONS = 6`. Replace the comment on
     `DEFAULT_MAX_SESSIONS` (it says "set the real number for your agents"; now: the default is 6, an agent child
     is a few hundred MB, the box sizes it). Update the "missing file is an empty allowlist" doc comment.
   - `fn on_path(command: &str) -> bool` (`std` only): a command containing `/` must be an existing file with an
     execute bit; otherwise some entry of `$PATH` joined with it must be. No new crate.
   - `acp_agents()` returns only agents whose `command` passes `on_path` at call time (it is called for every
     connection hello). `create_with` checks it too and returns "That agent is not installed on this server"
     instead of a spawn error. "Usable" means installed; login is not probed (a probe would spawn a child).
   - Auth failures: when `session/new` fails with a JSON-RPC error, the error shown is
     `"{agent} would not start: {agent's message}. Log in to it on the server, then try again."` (fact 0040:
     gemini `-32000`). The agent's text stays sanitised.
   - Tests: `resolve_backends` (auto with `acp` on/off, explicit list unchanged); config (no file, file without
     `builtin`, `builtin = true`, same-name override, `max_sessions` default 6 and from file); `on_path` against a
     temp dir `PATH` (present, missing, not executable, absolute path) and an adapter appearing after startup
     showing up in `acp_agents()`; each built-in's command and args equal the table in `scripts/acp-probe.mjs`;
     an old `acp-agents.toml` without the new keys still parses.
   - **Done when:** the tests above pass; with no config file and a fake `claude-agent-acp` on `PATH`, the hello
     lists `claude` in `acp_agents`; with `TERMENBOX_ACP=off` the `acp` backend is absent.

2. **Dialog: pick the view per agent** (`packages/client/src/NewSessionSheet.svelte`, `lib/newsession.ts`)
   - The control is called **View** everywhere (UI text, code, test ids); never touch the session-mode select in
     `AcpView.svelte`.
   - New `view` (`terminal` | `acp`) next to `template` is the source of truth; `backend` is derived from it
     (`acp` -> `'acp'`, `terminal` -> `'tmux'`, other managers -> their name). The reactive line
     `backend = server.backends[0]` (line 29) must not overwrite it.
   - Replace the Manager dropdown for the tmux/acp pair with a segmented "View" control: *Terminal (agent's own
     TUI)* | *Agent view (ACP)*. It shows only when the selected template is in `server.acpAgents` and `tmux` is
     in `server.backends`. The Manager dropdown stays for other backends (`command`).
   - Cases: a template with no ACP agent (`shell`, others) -> no control, view `terminal`. A server with `acp` and
     no `tmux` -> no control, the template list is the ACP agents, view `acp`. A server with `command` and `acp`
     but no `tmux` -> the Manager dropdown stays and lists them. Changing the template to one without an ACP agent
     resets the view to `terminal`. A remembered `acp` view for an agent the server no longer lists falls back to
     `terminal`.
   - Default Terminal; remembered per `serverId + template` in `lib/newsession.ts`, stored the way `tabs.ts`
     stores hidden tabs (own key, `try`/`catch` read and write, in-memory fallback). Pure functions
     `loadView(store, serverId, template)` and `saveView(store, serverId, template, view)`, tested without a DOM.
   - The wire message is unchanged (`backend: 'acp'` + `template`); `backend` is still sent only when the server
     has more than one. ACP still needs a directory (existing validation).
   - **Done when:** component tests cover every case above plus default Terminal, remembered choice, and a
     storage that throws; `scripts/dev.sh pnpm -r test` passes.

3. **Harden restore** (`packages/server/src/acp.rs`, `acp_stream.rs`; the core event types need no change)
   - **Persist what is not running.** `Saved` gains `#[serde(default)] reason: Option<String>`,
     `#[serde(default)] retry: bool`, `#[serde(default)] turn_open: bool` (old files load unchanged; `acp_id` may
     be empty for an entry that never started). `Entry` gains `reason: Option<String>` and
     `restoring: bool`. `save()` writes running entries (with `turn_open` taken from the stream's `working` flag)
     and entries with no live agent (their `reason`, `retry`), sorted by name. `kill()` removing an entry already
     drops it from the file on its `save()`.
   - **Over the cap**: `restore` starts at most `max_sessions` children; the rest become entries with no live
     agent, `reason = "Not restored: at most N agent sessions run at once"`, `retry = true`.
   - **Retry**: every entry with `retry = true` is tried again at the next `restore`. An invalid name or directory
     is kept (not skipped silently) with its reason and `retry = false`; an invalid name that cannot be shown
     safely is logged once, without secrets, and dropped.
   - **Show a failure, not "Done".** `stream_view` for an entry without a live agent builds the stream with
     `st.status("failed", reason)` (today `"ended"`, no reason), so `AcpView` shows "Stopped: <reason>" and the
     existing Close works through `kill()`. `restore` reports a failed start as
     `Turn { open: false, outcome: Some(AgentState::Error) }` (today `Done`), which raises a Failed attention
     item and push (`events.rs`).
   - **Interrupted turn (E).** After a successful resume where the saved `turn_open` was true: add a stream notice
     (level `error`) "A server restart interrupted the running turn." and report
     `Turn { open: false, outcome: Some(AgentState::Error) }` once. Keep the existing "Restored after a server
     restart. Earlier output is not shown." notice (`acp.rs` ~864); it is not new work.
   - **Restore vs create race.** At the start of `restore`, in one lock, insert a placeholder `Entry`
     (`restoring = true`, no live agent) for every saved name and add the number of children it will start to
     `starting`; each finished start replaces its placeholder and decrements `starting`. `create_with` already
     rejects existing names and counts `starting`, so neither a name nor a slot can be taken meanwhile. For a
     restoring entry `live()` fails with "This session is still being restored", and `stream_view` shows a
     stream with the default status and an info notice "Restoring after a server restart...". (Awaiting
     `restore()` before serving was rejected: up to six sequential 30 s handshakes would block startup.)
   - **Done when:** tests pass for: cap exceeded keeps entries across a second `save()` and a second `restore()`;
     a failed start is retried next restore and reports `Error`; an invalid dir is kept with a reason; a `create`
     issued during `restore` respects the cap and the name check; `turn_open` produces the notice and one
     Failed attention; an old `acp-sessions.json` with only the old fields loads.

4. **Docs and knowledge base** (use `/roadmap` after approval; it writes these; commit this plan with the PR so
   the rejected-option rationale has a source)
   - New decision (next free id; 0136 at the time of writing, check `docs/decisions/` again before writing): the
     view is chosen per agent at session creation; ACP sessions stay server-owned; restart survival is resume,
     with the mid-turn loss accepted and made visible; no tmux/TUI host for ACP; cap 6 with no floor and
     `resource-constraints` covering termenbox's own processes, not agent children (edit the guardrail text in the
     same PR); schema-crate adoption (G). Amends 0111: the default question is answered (Terminal) and the
     allowlist is "built-ins plus the owner's file". Records the rejected options in the table above.
   - New roadmap step (next free; R74 at the time of writing; run `python3 docs/plans/roadmap/check.py` before
     and after writing).
   - Update `docs/guide/sessions.md` (the view choice; a restart interrupts a running ACP turn) and
     `docs/guide/configuration.md` (`TERMENBOX_BACKEND` default, `TERMENBOX_ACP`, built-in profiles, `builtin`,
     `max_sessions`, installing an adapter in the Docker container and logging in to it, roughly how much memory
     each session needs and that six sessions want a larger box).
   - **Done when:** `check.py` and the knowledge-base lint are clean and the guide pages describe every key above.

## Out of scope (record as follow-ups in the roadmap step)

- Detached ACP host that outlives the server. If wanted, reuse the agents' own long-lived servers
  (`opencode serve`, `goose serve`); unverified over our stdio transport, so measure first.
- Draining open turns on shutdown (E2).
- Terminal projection of ACP sessions for SSH / xterm.
- Native TUI sharing one agent with the web view (possible for opencode `serve`/`attach`; unverified).
- Probing login state before offering an agent.
- The full `agent-client-protocol` SDK, and ACP protocol v2 (`unstable_protocol_v2`); revisit when agents ship v2.
- A "continue an earlier conversation" picker from `session/list` (unstable in the spec; not on every agent).
- A free-memory floor before starting a child (choice F).

## Critical files

- `packages/server/Cargo.toml`, `Cargo.lock`, `packages/server/testdata/acp/` (golden fixtures)
- `packages/server/src/main.rs` (`Args`, `resolve_backends`, `build_backend`, ACP arm ~494)
- `packages/server/src/acp.rs` (`AcpConfig`, `AgentProfile`, `handshake` ~670, `Saved`, `Entry`, `save` ~798,
  `restore` ~886, `stream_view` ~957, `create_with` ~1059, `acp_agents` ~1182, `kill` ~1224),
  `packages/server/src/acp_stream.rs` (`status` ~510)
- `packages/server/src/connection.rs` (~145, `acp_agents` in the hello), `packages/server/src/events.rs` (~396)
- `packages/client/src/NewSessionSheet.svelte`, `packages/client/src/lib/newsession.ts`, `lib/hub.ts` (`acpAgents`)
- `docs/guide/sessions.md`, `docs/guide/configuration.md`, `docs/guardrails/resource-constraints.md`,
  `docs/decisions/`, `docs/plans/roadmap/`

## Verification

- Each step's **Done when**, plus `scripts/dev.sh cargo test --workspace` and `scripts/dev.sh pnpm -r test`.
- E2E: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/run.mjs'` with the scripted fake agent
  (`packages/server/testdata/acp/`): one scenario creating a session in each view, and one restarting the server
  during an open turn and checking the interruption notice.
- Manual (owner): with a real claude or opencode adapter installed in the Docker image, create a session in each
  view, restart the container, check that the ACP session comes back with its conversation, that an interrupted
  turn says so, and that the terminal session is untouched. Also try an installed but logged-out adapter and check
  the error message.
- Two PRs, each squashed with green CI: step 0 (`refactor(server): ...`, schema crate, no behaviour change) first,
  then steps 1-4 on top. Branch off `develop`, commit subjects `type(scope): summary` with `Why:`/`What:` bodies,
  PRs to `develop`.
