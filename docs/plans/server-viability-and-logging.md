# Server viability and logging plan

Goal: after the 2026-10-28 incident (server died in ACP mode, no logs; `docs/incidents/2026-10-28-server-down-in-acp-mode.md`), make a fault in one ACP session cost one workspace, and make every death leave evidence a user can share safely. Governed by decisions 0142 (logs, crash record), 0143 (containment, restart), 0144 (bundle, incidents), fact 0061 (today's state) and guardrails `a-fault-in-one-session-never-stops-the-server`, `audit-logging-no-secrets`, `diagnostics-never-include-secrets`, `resource-constraints`. Server code is in `packages/server/src/`; run Rust through `scripts/dev.sh cargo ...` if no compiler is installed. PRs target `develop`; commit subjects `type(scope): summary` with `Why:`/`What:` body. Work in a worktree.

Legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved.

### 1.1 JSONL log file with rotation and redaction — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh cargo test -p termenbox-server logging`
Context: `packages/server/src/main.rs` (fn main, ~line 606) initialises `tracing_subscriber::fmt()` to stderr only (fact 0061). Decision 0142 wants JSON Lines in `<config-dir>/logs/termenbox.jsonl` (config dir is `--config-dir` / `TERMENBOX_CONFIG_DIR`, `args.config_dir`) as well as stderr, capped at 100 MB total and 30 days, level `info`, overridden by `TERMENBOX_LOG` (an EnvFilter string, falling back to `RUST_LOG`). Guardrail `audit-logging-no-secrets` forbids secrets and ACP/terminal text in logs and requires a redaction layer. Server has `tracing-subscriber` with `env-filter`, `fmt`; the workspace `Cargo.toml` pins it. Release builds are size-tuned (fact 0047): add only small crates.
Do: Add `packages/server/src/logging.rs` (declare it in `main.rs`) that builds a layered subscriber: stderr fmt layer as today plus a JSON file layer (enable the `json` feature, optionally `tracing-appender` non-blocking writer), a size-and-age pruner run at start and daily, and a layer or field filter that drops fields named `secret`, `token`, `key`, `signature`, `nonce`. Initialise it in `main` where the `fmt()` call is, only for the server run path (not `--hook`, `tmux`, or setup subcommands). Failure to open the directory is a stderr warning, not an exit. Add tests in `logging.rs`.
Done-when: a test writes events including a field named `secret` and a payload field to the file and finds neither; a test fills the dir past the cap and the pruner removes the oldest files; `grep -n "logging::" packages/server/src/main.rs` shows the init call.

### 1.2 Panic hook and run marker — [x]
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh cargo test -p termenbox-server crash`
Context: Decision 0142. The release profile unwinds (workspace `Cargo.toml` comment: no `panic = "abort"`). There is no `panic::set_hook` anywhere under `packages/server/src` (fact 0061). Marker file: `<config-dir>/run.marker`, created at start, removed on clean exit (the shutdown path in `main.rs` after the server future ends, including SIGTERM/ctrl-c handling).
Do: Add `packages/server/src/crash.rs`: `install_panic_hook(config_dir)` writes message, location, thread name and a captured backtrace through `tracing::error!` and to `logs/crash-<unix-ts>.json` (flush before returning, then call the previous hook). `RunMarker::acquire(config_dir)` returns the previous state (`Clean` or `Unclean { since, last_crash_file }`) and writes the marker with timestamp and pid; `Drop`/explicit `release()` removes it. Wire both into `main.rs` after logging init, server path only.
Done-when: tests show a panic in a spawned thread writes a crash file; acquiring twice without release reports `Unclean`; acquiring after release reports `Clean`.

### 1.3 Report the unclean exit on start and to the client — [ ]
Class: implement · Needs: 1.2 · Check: `scripts/dev.sh cargo test -p termenbox-server unclean`
Context: Decision 0142 says the server logs `previous run ended uncleanly` with the last marker time, the last health sample (RSS, `event_log_bytes`; see `packages/server/src/health.rs`, history sampler) and the newest crash file. The client needs the same fact to prompt the user (step 4.2); an authenticated message or existing status payload should carry `previous_exit: {unclean, at, crash}` once per boot with no paths, hostnames or text.
Do: At start, if `RunMarker` says `Unclean`, log the warning with those fields and store a `PreviousExit` in app state; expose it through the existing authenticated server-info/status message in `packages/core/src/protocol.rs` (add an optional field, bump nothing else) and update `packages/client` types.
Done-when: a test starts state with an unclean marker and the status payload carries `previous_exit.unclean == true`, and with a clean marker the field is absent; `cargo test -p termenbox-core` still passes.

### 2.1 Audit panic paths in ACP session code — [x]‖
Class: explore · Needs: none · Check: `test -s docs/plans/server-viability-and-logging-audit.md`
Context: `packages/server/src/acp.rs`, `acp_stream.rs`, `backend.rs`, `event_log.rs`. Decision 0143 requires that one session's panic cannot stop others or poison shared state. Only 3 `tokio::spawn` calls are in `acp.rs`; the audit must also cover `std::thread`, `spawn_blocking`, `std::sync::Mutex`/`RwLock` shared across sessions, `unwrap`/`expect`/indexing on agent-provided data, and unbounded channels.
Do: List every shared lock and every spawned task in the ACP path with file:line, who can panic while holding it, and a recommended fix (parking_lot, catch_unwind at the task root, bounded channel). Write the result to `docs/plans/server-viability-and-logging-audit.md`. Change no code.
Done-when: the audit file exists with a table covering each of the four source files above.

### 2.2 Supervise each ACP session — [ ]
Class: implement · Needs: 1.1, 2.1 · Check: `scripts/dev.sh cargo test -p termenbox-server supervis`
Context: Decision 0143 and guardrail `a-fault-in-one-session-never-stops-the-server`. Today a failed session surfaces as notices in `acp.rs` (`st.notice("error", ...)`); a workspace not running is kept "with why" (`acp.rs` ~line 1052). The audit (2.1) names the fixes.
Do: Run each session's tasks under a supervisor that catches panics (`catch_unwind` on the task root / join handle result), marks the workspace `failed` with a reason code (no text), logs it with the session id, kills the agent child, and offers the existing restart path. Apply the audit's lock fixes. Add a test hook to panic inside one session.
Done-when: a test with two sessions panics one and the other still answers a prompt; the failed one reports `failed` and can be restarted.

### 2.3 Safe-mode restore after repeated unclean exits — [ ]
Class: implement · Needs: 1.3, 2.2 · Check: `scripts/dev.sh cargo test -p termenbox-server safe_mode`
Context: `acp.rs` restores saved workspaces after a restart (`session/resume`/`session/load`, ~line 1328; state in `acp-workspaces.json`). Decision 0143: a workspace live at the last two unclean exits is restored stopped with a notice.
Do: Keep a small counter in the run marker file (consecutive unclean exits and the workspace ids live at each). On restore, skip auto-resume for ids present in the last two unclean sets and set the notice "Not resumed: the server stopped unexpectedly twice while this was running. Open it to resume." Reset the counter on a clean exit or after 10 minutes of uptime.
Done-when: a test with two simulated unclean exits restores the live workspace stopped with the notice, and with one unclean exit it resumes as today.

### 3.1 Restart policy in installed units — [ ]‖
Class: implement · Needs: none · Check: `grep -c "Restart=always" packages/client/installer/install.sh`
Context: `packages/client/installer/install.sh` writes systemd units in two places (lines ~819 and ~990) with `Restart=on-failure` and `RestartSec=2`; `docker/docker-compose.yml` has `restart: unless-stopped`. Decision 0143 wants `Restart=always`, `RestartSec=2`, and a start-limit that ends in a visible failed state. Updates deliberately restart the unit (`KillMode=process` keeps tmux alive), so `Restart=always` must not fight the updater or uninstaller (`packages/server/src/updater.rs`, `uninstall.rs`).
Do: Change both unit writers to `Restart=always` with `StartLimitIntervalSec=300` and `StartLimitBurst=10` in `[Unit]`/`[Service]` as systemd requires; confirm `systemctl stop` still stops it. Update any installer test that matches the old text.
Done-when: the grep prints 2; installer tests/shellcheck in CI config pass (`scripts/dev.sh sh -c 'ls packages/client/installer'` lists the test, run it).

### 3.2 A health check that sees hangs — [ ]
Class: implement · Needs: 2.2 · Check: `scripts/dev.sh cargo test -p termenbox-server health`
Context: `packages/server/src/main.rs` ~line 1157 serves `/health` as a constant `"OK"`; the Docker `HEALTHCHECK` (`docker/Dockerfile`) curls it every 60 s. `health.rs` already has a sampler loop that is replaced when it panics.
Do: Have the ACP supervisor and the health sampler update a heartbeat timestamp; `/health` returns 503 if either is older than 30 s, 200 `OK` otherwise. Keep the body free of details.
Done-when: a test with a stalled heartbeat gets 503 and a fresh one gets 200.

### 4.1 Server-side diagnostics bundle — [ ]
Class: implement · Needs: 1.1, 1.3 · Check: `scripts/dev.sh cargo test -p termenbox-server bundle_scrub`
Context: Decision 0144 and guardrail `diagnostics-never-include-secrets` (scrub certificates, claim secrets, tokens, IPs, ports; never terminal output, keystrokes, session names). The client already has a scrubber: `packages/client/src/lib/report.ts` with `report.test.ts`. Server version/channel is already known to the server (`release.rs`); health history is in `health.rs`.
Do: Add an authenticated owner-only request that returns a bundle: last 2 MB of `termenbox.jsonl`, newest crash file, previous-exit report, versions/channel, health history. Scrub server-side (IPv4/IPv6, ports, tokens, home paths, anything the log layer would drop) in a new `bundle.rs`. Return text; never write it elsewhere.
Done-when: a test feeds a log containing realistic secrets, addresses, a home path and ACP message text and the bundle contains none of them; non-owners get an error.

### 4.2 Client: preview, download, crash prompt, prefilled issue — [ ]
Class: implement · Needs: 4.1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: `packages/client/src/lib/report.ts` builds the problem report, shows it to the user before it leaves and opens GitHub (decision 0086); `.github/ISSUE_TEMPLATE/problem.yml` is the template. Decision 0144: after `previous_exit.unclean` (step 1.3) the app says so once and offers to build the bundle and open a prefilled GitHub issue labelled `incident`. Guardrails `errors-fit-one-screen`, `ui-never-moves-under-the-finger`, `usage-stays-on-device` apply: nothing is sent without a tap.
Do: Add an "Server diagnostics" action next to Report a problem that requests the bundle, shows it, and offers Download and Open issue (template `incident.yml`, text trimmed to fit the URL, rest attached by the user). Add the one-time unclean-exit banner. Add tests alongside `report.test.ts`.
Done-when: tests show the banner appears once for an unclean `previous_exit`, the bundle is shown before any download or issue link, and the issue URL stays within the length limit `report.test.ts` already asserts.

### 4.3 Incident issue template — [ ]‖
Class: implement · Needs: none · Check: `test -f .github/ISSUE_TEMPLATE/incident.yml`
Context: `.github/ISSUE_TEMPLATE/` has `config.yml` and `problem.yml` only. Decision 0144.
Do: Add `incident.yml` (label `incident`) asking what the user was doing, install type (systemd/Docker/other), version, whether the server restarted, and for the diagnostics bundle (attached after previewing; no terminal content). Match `problem.yml` style.
Done-when: the file exists and its fields mirror `problem.yml` structure.

### 5.1 User guide and docs — [ ]
Class: implement · Needs: 1.1, 4.2 · Check: `grep -n "termenbox.jsonl" docs/guide/*.md`
Context: `docs/guide/install.md` explains the host install and state in `~/.config/termenbox`. Users need to know where logs live, how to change the level, and how to share a bundle.
Do: Add a "Logs and diagnostics" section: file location, retention, `TERMENBOX_LOG`, the crash prompt, the bundle and what it excludes, and that nothing is sent automatically.
Done-when: the grep finds the section.

### 5.2 Review against guardrails — [ ]
Class: review · Needs: 2.3, 3.2, 4.2 · Check: `go-getter conform`
Context: Guardrails `a-fault-in-one-session-never-stops-the-server`, `audit-logging-no-secrets`, `diagnostics-never-include-secrets`, `resource-constraints` (measure idle RSS with logging on, fact 0047), `usage-stays-on-device`.
Do: Run conform over the branch; measure idle RSS before/after; fill the cause section of the 2026-10-28 incident doc if new evidence exists.
Done-when: conform reports no breaks and the RSS delta is noted in the PR.
