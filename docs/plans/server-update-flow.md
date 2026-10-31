# Server update flow: watch, notify, check, update, grace

Decision: 0153. Refines decisions 0063 (signed releases, opt-in host updates), 0076 (updater), 0077 (root actions by request file),
0131 (update policy). Keeps guardrails `updates-are-verified-and-reversible`, `unprivileged-server-never-holds-root`,
`manifest-url-resolution`, `host-changes-are-journaled`, `notifications-carry-no-agent-text`, `resource-constraints`.

**This plan is re-entrant.** Read "How to resume" first. The ledger is the only state; every step has a check that
proves it done from the repository, so a lost session loses nothing.

## How to resume (do this every time, in order)

1. `git fetch origin && git log origin/develop --oneline -30` and `gh pr list --search "update-flow" --state all`.
2. `sh scripts/pool.sh status`. Reuse the slot of an open `*update-flow*` branch; otherwise acquire one for the next step.
3. For each step in the ledger from the top, run its **Verify** block. The first step whose Verify fails is the
   current step, whatever its checkbox says. A ticked box that fails Verify is unticked and noted in the Log.
4. Do only that step. One step = one squashed PR into `develop` (guardrail `changes-land-as-one-squashed-pull-request`).
5. Checkpoint inside the same PR: tick the box, fill `PR` and `Merged`, append one Log line. The ledger therefore
   changes atomically with the code and merges with it. Mid-step progress lives in the branch and a draft PR; the
   `Notes` field of the step records anything the next session needs that the diff does not show.
6. Never start a step whose `Needs` are not ticked and verified. Never edit a decision or guardrail without the
   owner's `accepted-by`; step 0 is the owner's call.

## Goals and how the plan meets them

| Goal | Met by |
|---|---|
| Watch for an update | U1: boot check, frequent cheap local tick, honest cadence; U2: persisted state |
| Notify | U4: banner and Updates card; U6: web push (kind and server name only) |
| Manual check | U3: `termenbox-server update check`, app **Check now** with an acknowledgement |
| Run an update | U3 `update apply`, app **Update now**; existing verified, reversible flow unchanged |
| Auto-apply after a grace period, configurable fallback | U5: `grace`, `on_timeout = install \| skip` |
| Observability | U2 state file, U3 `status --json`, U4 health warning, U7 diagnostics |
| Maintainability | one state machine with injected clock and fetch; tests in U1, U2, U5 |
| UX | one card, a countdown, postpone and skip; nothing moves under the finger |

## Review findings this plan fixes (all checked against the code on 2026-10-31)

1. `OnCalendar=daily` + `RandomizedDelaySec=3600` fires 00:00-01:00, so any `maintenance_window` outside that hour is
   never reached and the release waits a day (`packages/client/installer/install.sh`, units in both install kinds).
2. `available_update()` ignores a status file older than 3600 s while the timer runs daily, so a `notify` server shows
   nothing for 23 hours unless an owner is connected (`packages/server/src/requests.rs`).
3. A fresh install never checks until the next midnight; the timer has no `OnBootSec`.
4. The docs' "Check now" command installs in `auto` mode; there is no read-only check from a shell.
5. `run()` deletes the status file first, so a failed or slow check erases the notice and leaves no record of why.
6. A rolled-back version is retried on the next run.
7. Decision 0063 says container auto-update is on; the container has no updater at all.
8. Not a bug: a non-release build (`0.1.57+dev`) compares newer than `0.1.57-beta.7`. Show it, do not hack the
   comparison (U3 `status` says "unreleased build"; release builds already set `TERMENBOX_RELEASE_VERSION`).

## Design

### State (U2)

`update-state.json`, written by the helper (root or user helper), read by the server, never written by the server.
Atomic write (temp + rename), schema-versioned, bounded size, read with the same defensive reader as today.

```json
{ "v": 1, "phase": "idle|available|scheduled|installing|rolled-back",
  "checked_at": 0, "checked_ok_at": 0, "error": null,
  "available": "0.1.58", "first_seen_at": 0, "deadline": 0, "deferrals": 0,
  "skipped": "0.1.57", "last": { "result": "updated|rolled-back|failed", "from": "", "to": "", "at": 0, "reason": "" },
  "channel": "beta", "mode": "auto", "current": "0.1.57" }
```

`update-status.json` stays for one release as a written-in-parallel compatibility file so a new helper with an old
server (or the reverse after a rollback) still shows the notice.

### Cadence (U1)

- `termenbox-update.timer`: `OnBootSec=2min`, `OnUnitActiveSec=6h`, `RandomizedDelaySec=15min`, `Persistent=true`.
  Two small requests per check (manifest and signature, from `releases/download`, not the rate-limited REST API).
  At 10^5 servers that is about 8 x 10^5 requests a day to GitHub's CDN; it is not the directory, so
  `directory-request-budget` is untouched. Revisit with ETag only if the host proves it honours conditional requests
  across the redirect (U1 spike), otherwise do not rely on it.
- `termenbox-update-tick.timer` (new): every 5 min, **no network**: reads `update-state.json`, and acts only if a
  deadline has passed. About 10 ms of CPU a run (measured on the request helper), well inside `resource-constraints`.
- The installer runs one check immediately after a fresh install or an upgrade of the units.
- The 3600 s expiry goes. A notice is valid until superseded or the version installed; staleness is shown as
  "last checked N h ago" and becomes a health warning past 72 h (U4).

### Policy (U5), forward compatible

```toml
[update]
channel = "stable|beta|nightly"
mode = "auto|notify|hold"            # unchanged
maintenance_window = "03:00-04:00"   # unchanged; now actually reachable
grace = "12h"                        # optional; only with mode = "notify"
on_timeout = "install|skip"          # optional; default "skip"
```

`mode = "notify"` + `grace` is the "ask, then fall back" mode; no fourth mode name. Rules:

- A new release sets `deadline = first_seen + grace`; if that falls outside `maintenance_window`, it moves to the next
  window start. With no `grace`, behaviour is exactly today's.
- At the deadline the tick applies `on_timeout`. `install` runs the normal verified flow. `skip` pins the version in
  `skipped` and stops reminding about it; a newer release starts a new grace.
- The owner can **Update now**, **Postpone** (adds one grace, at most 3 times: `deferrals`) or **Skip this version**.
  These are per-release state, not policy, and travel as request files. Policy (`channel`, `mode`, `grace`,
  `on_timeout`, window) stays host-only, as 0131 says; the app cannot write `update.toml`.
- A major version never auto-installs, even at the deadline: it stays `available` until the owner confirms.
- Failed install or rollback pins the version in `skipped` until a newer one appears or the owner asks again.
- `hold` fetches nothing, as today.
- **Forward compatibility.** The parser uses `deny_unknown_fields`, so an older binary (after a rollback) refuses a
  file with the new keys and stops updating. U5 therefore ships in two parts: first a release whose parser accepts and
  ignores `grace`/`on_timeout`; only a later release lets the installer or docs write them.

### Requests (U3)

Fixed list, validated, no free text, as 0077 requires: `check-update` (exists), `update-now` (exists),
`update-postpone`, `update-skip {version}` (version restricted to the same charset and length as today's check). The
server writes them only after the existing owner checks. `auth_ok` gains `update_capabilities: ["check","postpone","skip"]`
so a new client never offers a button an old helper rejects (an unknown action is logged `not a known request` and
removed, which is what we saw on the installed 0.1.57).

### Notifications (U6)

Web push with a new payload kind `update`, carrying only the server's display name, the kind and a time
(`notifications-carry-no-agent-text`); the version number is fetched by the client after it opens, not pushed. The
existing marker test is extended to the new builder. In-app: the floating banner from #391 plus the Updates card, with
a countdown "installs at 03:00 unless you postpone or skip". Nothing reorders under a touch (`ui-never-moves-under-the-finger`).

### Container mode (U8, go/no-go)

A host timer running the same helper with a Docker `Service`: pull the digest named by the signed manifest, recreate,
health-check, roll back to the previous digest. Needs the Docker socket on the host, so it is root-side only, journaled
before each change (`host-changes-are-journaled`) and never given to the container. It needs its own decision (digest
pinning in the manifest, `docker` as a root dependency) and an owner go/no-go before any code.

## Ledger

Statuses: `[ ]` todo, `[x]` done and verified. Update inside the step's own PR.

### U0 [x] Decision: refine 0131 (owner)

- Needs: none. Owner: the maintainer (`accepted-by` is theirs, not the agent's).
- Deliverable: one decision file under `docs/decisions/` stating: `grace` and `on_timeout` under `notify`; per-release
  postpone/skip as request-file state while policy stays host-only; cadence (boot, 6 h, 5 min local tick); the
  deadline-in-window rule; major versions never auto; rollback pins the version; forward-compat order of U5. Add it to
  `docs/decisions/INDEX.md`. Optionally a short fact for the 5-minute-tick cost.
- Verify: `ls docs/decisions | grep -i 'grace.*owner'` prints a file whose frontmatter has `status: active` and an `accepted-by`.
- PR: this branch (`docs/server-update-flow-plan`) | Merged: - | Notes: decision 0153, accepted by the owner in chat 2026-10-31 (all six points).

### U1 [x] Cadence and window fix (installer + timers)

- Needs: U0.
- Do: both unit sets in `packages/client/installer/install.sh` (system and user): replace `OnCalendar=daily` with the
  cadence above; add the local tick timer; run one check after install/upgrade; migrate existing installs idempotently
  (`add_unit` today skips an existing file, so add a versioned replace for the timer files, journaled first).
  Spike: does a conditional GET survive the GitHub redirect; record the result in Notes, rely on it only if yes.
- Tests: extend `packages/server/tests/install_channels.rs` (units contain `OnBootSec`, no `OnCalendar=daily`, migration
  is idempotent and keeps custom arguments); a unit test with a fake clock that a `03:00-04:00` window is reached.
- Verify:
  ```sh
  grep -q 'OnBootSec' packages/client/installer/install.sh && ! grep -q 'OnCalendar=daily' packages/client/installer/install.sh
  scripts/dev.sh cargo test -p termenbox-server --test install_channels
  ```
- PR: this branch | Merged: - | Notes: cadence in both unit sets via one `UPDATE_TIMER`; `migrate_update_timer` (put_file:
  backup and journal, idempotent); a first check after install/upgrade; test
  `the_update_timer_checks_at_boot_and_every_six_hours_and_an_old_daily_timer_is_replaced_once`. Verify passed (8/8).
  NOT DONE, carried to U3: the conditional-GET spike (the ETag is not relied on, so nothing depends on it).
  MOVED to U5b: the 5-minute tick timer and the window-reachability test; the tick needs a no-network `update --tick`
  that does not exist yet. Until then a `03:00-04:00` window is hit only if a 6-hourly check lands in it.
  Running tests without the `docker` group on this machine: `sg docker -c "COMPOSE_PROJECT_NAME=termenbox-sergei scripts/dev.sh ..."`
  (the shared `docker_cache` volume belongs to another uid).

### U2 [x] State file and honest notice

- Needs: U1.
- Do: `updater.rs` writes `update-state.json` (schema above) at every outcome; stop deleting on a failed check (record
  `error`); keep `update-status.json` in parallel; `requests::available_update` reads the new file, drops the 3600 s
  expiry, and keeps every existing channel, version and charset check. Rollback pins `skipped`.
- Tests: table tests over phases; a check that fails keeps the previous notice and sets `error`; a rolled-back version
  is not offered again; old status file alone still works; junk/oversize files read as none.
- Verify: `scripts/dev.sh cargo test -p termenbox-server -- updater requests update_state` passes (cargo takes one filter
  before `--`), and the tests named
  `a_failed_check_keeps_the_notice` and `a_rolled_back_version_is_not_retried` exist:
  `grep -rq a_rolled_back_version_is_not_retried packages/server/src`.
- PR: this branch | Merged: - | Notes: new module `update_state.rs` (`State::apply` with one `Event` per outcome, so the
  transitions are tested without a clock); `Updater::run` records every outcome, a failed check keeps the notice and sets
  `error` (the helper also records config/key failures before the updater exists); `available_update` reads the newer of
  `update-state.json` and `update-status.json` (state wins a tie) and has no expiry; a rolled-back version is pinned in
  `skipped` and the timer does not retry it, `update-now` does. Verify passed. `last.result = failed` and the `scheduled`
  phase are not written yet (a failed install pins in U5b). Known edge: after a rollback to an old helper that writes only
  the old file, a stale state file can win a tie only if its `checked_at` is newer, so it self-corrects at the next check.
  Cleanups: `cargo clippy -D warnings` already fails in `bridge-contract` (not ours); `control::a_real_tmux_pushes_...`
  failed once in a full run and passes alone.

### U3 [x] Manual check and apply (CLI and requests)

- Needs: U2.
- Do: `termenbox-server update status|check|apply|postpone|skip [--json]`. Root or user helper: act directly.
  Unprivileged: leave the request file and wait for the result (reuse the `update-result.json` id handshake) with a
  timeout. `status` prints version, channel, mode, window, last check, next check, deadline, and "unreleased build"
  when the running version is a `+dev` build newer than the channel. Add `update-postpone`, `update-skip` to `Request`.
  Fix `docs/guide/install.md` ("Check now") and the `--help` text.
- Tests: request round-trips and `deny_unknown_fields` junk is rejected (extend the existing `requests.rs` tests); CLI
  `--json` snapshot; `check` never installs even in `auto`.
- Verify: `scripts/dev.sh cargo test -p termenbox-server -- requests helper update_state` and
  `grep -q 'root-helper check' docs/guide/install.md`.
- PR: this branch | Merged: - | Notes: DEVIATIONS from "Do": the CLI is flag-based, so the commands are
  `--root-helper status|check|apply|postpone|skip` with `--json`, not an `update` subcommand. Unprivileged shells do not
  leave a request and wait (that needs ids on check requests; the app is the unprivileged path); they are told to run as
  the helper's account. `Request::UpdatePostpone`/`UpdateSkip {version}` exist and are validated; the server-side
  `leave_update_*` writers come with U4 where they are first used. `skip` works now (pins the release). `postpone` refuses
  until U5b schedules a deadline (it takes the extension as an argument, wired to `grace` then). `status` prints a
  `systemctl list-timers` hint instead of a next-check time. NOT DONE: the conditional-GET spike (carried from U1);
  nothing relies on ETag, so it stays optional. The `--help` text of `--root-helper` lists the new actions.

### U4 [x] App: Updates card, banner, health

- Needs: U3.
- Do: protocol (`packages/core/src/protocol.rs`): `update_capabilities`, `UpdateState` message with phase, version,
  deadline, last check, error; client `hub.ts` and a server **Updates** section (Check now with an acknowledgement and
  its own result, Update now, Postpone, Skip, "last checked N h ago"); the banner uses the floating #391 pattern; a
  warning when the last good check is over 72 h old. Hide buttons the helper does not support.
- Tests: protocol round-trip, `hub.test.ts`, component test with a mocked state for each phase; a phone check named by
  platform (`a-phone-check-names-its-platform`); the old-client/new-server and new-client/old-server pairs.
- Verify: `cd packages/client && pnpm test` and `cargo test -p termenbox-core`.
- PR: this branch | Merged: - | Notes: protocol: client `update_check`, `update_postpone`, `update_skip {version}`; server
  `update_check_requested` and `update_state {phase, available, deadline, checked_at, checked_ok_at, error, can_postpone}`;
  `auth_ok.update_capabilities` (`check|postpone|skip`, empty without a helper). The server pushes `update_state` after
  auth and on every change (20 s poll, so a check's outcome shows within ~20 s) and writes fixed request files
  (`leave_update_postpone`, `leave_update_skip`, owner only; the skip version is charset-checked before it is written).
  Client: `lib/updateState.ts` (pure text and staleness), hub `checkUpdate|postponeUpdate|skipUpdate|canUpdate`, an
  **Updates** dialog from the server menu (`ServerUpdatesDialog.svelte`) and a floating `UpdateBanner.svelte` in the #391
  layer (hidden while a needs-you banner shows). DEVIATIONS: the card is a dialog from the server menu, not a section of
  the row (the row keeps one height); the 72 h warning is in the card, not in `ServerHealth` (that is a different
  message, U7 can fold it in); the banner's tap opens the Updates card on the servers page, which leaves the open
  workspace. Compatibility: a new client with an old server gets no capabilities, so no button is offered (hub test);
  an old client ignores the two new server messages and the new `auth_ok` key. Postpone shows only once U5b schedules a
  deadline (`can_postpone` is false until then). PHONE CHECK: none; the dialog and banner are covered by server-render
  tests only, not run on an iPhone, an Android phone or an emulator (`a-phone-check-names-its-platform`). Verify passed
  (client 1053 tests + svelte-check 0 errors; core and server tests). `control::a_real_tmux_pushes_window_and_directory_changes`
  times out in this container, alone and in a full run; `control.rs` is untouched here, baseline on develop not established.

### U5 [ ] Grace period and fallback

- Needs: U4 (and U5a before U5b).
- **U5a**: parser accepts and ignores `grace` and `on_timeout`; release it. Verify: `grep -q on_timeout packages/server/src/updater.rs` and a test that an old key set still parses.
- **U5b** (a release later): deadline logic, tick action, window snapping, postpone cap, major-version rule, installer
  and docs write the keys. The tick is the only place that auto-installs under `grace`.
- Tests with an injected clock: deadline inside/outside the window, snapping to the next window start across midnight,
  `install` vs `skip`, postpone x3 then no more, major never installs, owner `update-now` before the deadline,
  a failed install pins the version.
- Verify: `grep -rq a_major_version_never_installs_at_the_deadline packages/server/src` and the updater tests pass.
- PR: - | Merged: - | Notes:

### U6 [ ] Push notification for an available update

- Needs: U4. Spike first: `push.rs` builds payloads from an `AttentionKind` per workspace; confirm whether a server-level
  kind fits or a second builder is needed, and record it in Notes.
- Do: payload kind `update` (server display name, kind, time only); sent once per new version, and once when a deadline
  is within an hour; respects the user's existing push escalation settings.
- Tests: the marker-absent test for every builder (the guardrail's Check), once-per-version dedupe.
- Verify: `grep -rq 'update' packages/server/src/push.rs` and the push tests pass.
- PR: - | Merged: - | Notes:

### U7 [ ] Observability and docs

- Needs: U2.
- Do: audit lines for check, defer, postpone, skip, install, rollback (no secrets, `audit-logging-no-secrets`); the state
  file and the last 20 results in the diagnostics bundle (0144); `docs/guide/install.md` and a new fact for the update
  state file; capture (`go-getter:capture`) and lint (`go-getter:lint`) pass over the touched docs.
- Verify: `grep -q update-state docs/guide/install.md` and the diagnostics bundle test lists the file.
- PR: - | Merged: - | Notes:

### U8 [ ] Container updater (go/no-go)

- Needs: U5b and an owner decision. Do not start without it; open a decision draft instead.
- Notes: see "Container mode". Out of scope until decided.

## Risks

| Risk | Mitigation |
|---|---|
| New policy keys break an older binary after a rollback | U5a/U5b ordering; rollback keeps the old binary only until health passes |
| A 5-minute tick costs CPU | no network, one small read; measure against `resource-constraints` in U1 |
| Auto-install restarts the server during an agent turn | v1: window and grace only; later: a bounded busy hint from the server (it can only delay, at most 2 h, never trigger) |
| Check traffic to GitHub grows with the fleet | 6 h cadence, random delay, manifest only; ETag only if proven |
| Request files accepted from a compromised server | fixed actions, charset-checked version, owner checks as in 0077 |
| Push says too much | kind and server name only; version fetched after open |

## Log (append only, newest last)

- 2026-10-31 plan written after a code review; installed 0.1.57+dev on beta follows no update because it is an unreleased
  build newer than `0.1.57-beta.7`; the installed binary also predates `check-update`.
- 2026-10-31 U0 done: decision 0153 written and accepted by the owner; next step U1.
- 2026-10-31 U1 done: cadence, migration and test; install_channels 8/8 pass; tick moved to U5b, ETag spike carried to U3; next step U2.
- 2026-10-31 U2 done: update-state.json, honest notice, rolled-back pin; server tests pass; next step U3.
- 2026-10-31 U3 done: helper actions status/check/apply/postpone/skip, two new requests, install guide fixed; next step U4 (client and core; a fresh session is cheaper).
- 2026-10-31 U4 done: update_state and the three owner actions on the wire, Updates dialog, banner, hub and compat tests; next step U5a (parser accepts and ignores grace/on_timeout).
