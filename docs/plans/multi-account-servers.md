# Termenbox Plan: several accounts on one machine (one server instance per OS user)

Decisions (all active, accepted 2026-10-17): 0115 (one instance per OS user; the OS is the boundary; no hard memory cap), 0116 (account = provider + id), 0117 (claim link previewed, consumed only on confirm, editable name), 0118 (reserved UDP range 4443-4542, say what needs root first, reachability test in the app). Builds on 0053 (one owner per server, unchanged), 0062, 0081, 0094 (sharing, not built here), 0113 (`host-me`). Fact 0046 lists what coexists today and what conflicts. Guardrails: `unprivileged-server-never-holds-root`, `host-mode-explicit-consent`, `audit-logging-no-secrets`, `resource-constraints`, `no-paid-services`, `connections-warn-never-drop`. R23 (bundled tmux) is step group 2 and independent of group 1. This file alone is enough for a cold session.

## Cold start

Rust server `packages/server/src`: `auth.rs` (`Identity`, `AuthState::authorize`, claim token, owner.json), `cert.rs` (device certificate; `github_id` field at line 35, `sub` mapped at line 110), `sessions.rs` (per-connection loop; audit lines use `identity.id`), `main.rs` (flags; `udp_port` default 4443 at line 107), `rtc.rs` (WebRTC-direct), `backend.rs` (`Runner`, every tmux call). Wire types `packages/core/src/protocol.rs`, mirrored by hand in `packages/client/src/lib/protocol.ts`. Hosted function that signs device certificates: `packages/client/functions/api/oauth/exchange.ts` (+ `exchange.test.ts`). Client: `packages/client/src/AddServerModal.svelte`, `lib/onboard.ts`, `lib/transport.ts` (unreachable message at line 40), `lib/device.ts`. Installer `packages/client/public/install.sh` (modes `host-me` user service, `host`/`host-user` system service; `pick_port`, `udp_in_use`, `open_firewall`, linger step near line 575). Installer CI scripts `.github/scripts/test-user-install.sh` (runs as user `alice` in a systemd machine) and `test-native-install.sh`. Releases `.github/workflows/release.yaml`. Tests: no compiler on this machine, so `scripts/dev.sh cargo test --workspace` (decision 0114); client `cd packages/client && npx --yes pnpm@9 test`. One squashed PR per step from its own branch off master, green CI, merge only on the owner's "merge" (`changes-land-as-one-squashed-pull-request`, `no-commits-on-the-default-branch`); commit subject `type(scope): ...`. 512 MB RAM, 1 vCPU. Never touch the owner's own running `termenbox.service`. Plans are tracked in GitHub issues (0049); ask before writing to the tracker.

## Goal

Several people can use one machine, each with their own Linux user and their own server instance, and each sees only their own sessions. Adding an account needs root only for setup (the Linux user, a firewall rule once per machine), never to run. A second OAuth provider can be added later without changing the server's access rule.

Legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved.

## Steps

### 1.1 Qualify identity by provider — [x] ‖
Class: implement · Needs: none · Check: `scripts/dev.sh cargo test -p termenbox-server -- auth cert && (cd packages/client && npx --yes pnpm@9 test exchange)`
Context: Decision 0116. Today `Identity { id: u64, login }` in `packages/server/src/auth.rs` and `github_id: u64` in `packages/server/src/cert.rs` mean GitHub implicitly; `owner.json` stores `id`/`login`. The certificate is signed by `packages/client/functions/api/oauth/exchange.ts`.
Do: 1. Make `Identity` `{ provider: String, id: String, login }`; `authorize` compares provider and id. 2. Add `provider` and a string `id` to the certificate payload in `exchange.ts` and `cert.rs`; keep reading a certificate without them as `github` with the numeric `sub` as the id, until old certificates have expired. 3. Read `owner.json` (and `shares.json` if present) without `provider` as `github`; write both fields. 4. Replace `github_id` in audit lines (`sessions.rs`, `auth.rs`) with `provider` plus `account_id`; never log a certificate or token. 5. Add tests: an old-form certificate and an old `owner.json` still authorise as `github`; a same-id different-provider identity is denied.
Done-when: the tests above pass, `grep -rn "github_id" packages/server/src` returns nothing outside tests that deliberately cover the old form, and an `owner.json` written by the previous version still opens the server.

### 1.2 Server binds its UDP port from the reserved range — [x] ‖
Class: implement · Needs: none · Check: `scripts/dev.sh cargo test -p termenbox-server -- rtc main`
Context: Decision 0118. `packages/server/src/main.rs` takes `--udp-port` / `TERMENBOX_UDP_PORT`, default 4443; `install.sh` picks and records a port per install (`pick_port`, `udp_in_use`, which silently passes when `ss` is missing), so two instances can both choose 4443 and nothing reserves a port before it binds.
Do: 1. Without an explicit `TERMENBOX_UDP_PORT`, the server tries its saved port from the state dir, otherwise binds the first free port from 4443 to 4542, and saves the one it bound (`udp_port` file in its config dir). 2. An explicit value is used as given and fails clearly if taken. 3. The range exhausted is a clear startup error. 4. The signed directory record carries the bound port (existing mechanism). 5. Tests: two servers in one process space take different ports; a restart keeps its port; an explicit port wins.
Done-when: the tests pass and starting two servers with separate config dirs and no port setting yields two different ports within 4443-4542.

### 1.3 Installer says what needs root first and stops depending on `ss` — [x] 
Class: implement · Needs: 1.2 · Check: `sh -n packages/client/public/install.sh && grep -q "4443:4542" packages/client/public/install.sh`
Context: Decisions 0118 and 0113. In `host-me` the installer prints the firewall and lingering needs only at the end (around lines 575-600 of `install.sh`); `ufw status` needs root so it cannot be read by a user.
Do: 1. In the `host-me` path, stop choosing the UDP port; the server picks it (1.2); keep the HTTP port pick but make `tcp_in_use` fall back to `/proc/net/tcp` when `ss` is missing. 2. Before downloading anything, extend the plan summary: whether a firewall is active (`systemctl is-active ufw firewalld`, no root), the range to open once (`ufw allow 4443:4542/udp` / the firewalld equivalent), and the lingering state (`loginctl show-user "$ME" -p Linger`); try `enable-linger` at this point instead of at the end, journaled as now. 3. Never run `sudo`. 4. The system modes keep opening the firewall for the port they use. 5. Update the end-of-install output so it does not repeat what was already said.
Done-when: `sh -n` passes, running the `host-me` path in the existing CI machine prints the firewall and lingering lines before "will be installed", and a machine with `ss` removed no longer picks the same port for two installs.

### 1.4 Claim preview, confirm and an editable name — [x] 
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh cargo test -p termenbox-server -- auth sessions connection && (cd packages/client && npx --yes pnpm@9 test onboard)`
Context: Decision 0117. Today the first authenticated connection presenting the claim token binds the owner and deletes the token (`AuthState::authorize` in `auth.rs`); `AddServerModal.svelte` cannot show what is being added.
Do: 1. Add a preview request/response to `packages/core/src/protocol.rs` and `packages/client/src/lib/protocol.ts`: the token authorises it; the answer holds name, run mode, run user and version; it binds nobody and leaves the token valid. 2. Audit-log previews without the token. 3. In `AddServerModal.svelte` and `lib/onboard.ts`, fetch the preview, show it with a name field pre-filled from the reported name, and send the claim only on confirm; the typed name is stored by the client as today's typed name (0081). 4. Closing the dialog sends nothing. 5. Keep the root-proven re-claim as it is. 6. Tests: preview twice then claim works; an expired token or wrong token gets no preview; a closed dialog leaves the token usable.
Done-when: the tests pass and, against a dev server, the dialog shows the name, mode and run user before binding and the owner is unset until Add is tapped.

### 1.5 Reachability check as the last step of Add Server — [x] 
Class: implement · Needs: 1.2, 1.4 · Check: `cd packages/client && npx --yes pnpm@9 test transport`
Context: Decision 0118. `packages/client/src/lib/transport.ts` already throws `Unreachable` with a UDP message (line 40) when WebRTC fails, but Add Server does not use it to say what to do.
Do: 1. After the claim, run the real connect once; on `Unreachable` show the port, the exact command for the admin (`ufw allow 4443:4542/udp` or the range), and a line that the user's own network may block UDP. 2. Keep the server added; the check is advice, not a block (`connections-warn-never-drop`). 3. Test the three outcomes (reachable, unreachable, unknown).
Done-when: the tests pass and a dev server behind a closed UDP port shows the message with the range command.

### 1.6 CI: two users and a system install on one machine — [x] 
Class: implement · Needs: 1.2, 1.3 · Check: `sh -n .github/scripts/test-user-install.sh`
Context: Fact 0046 (coexistence read from the code, never run). `.github/scripts/test-user-install.sh` already installs `host-me` for `alice` in a systemd machine.
Do: 1. Extend it with a second user `bob` who is not a sudoer; install `host-me` for both. 2. Assert different ports in 4443-4542, different server ids, separate state directories, and that stopping or uninstalling one leaves the other running. 3. Assert `bob`'s install never calls `sudo` and prints the range note. 4. Add the case of a system install (`host-user`) present at the same time. 5. Update fact 0046's `last-verified` and replace "not run" with the result.
Done-when: the CI job "installer" passes with both users and the system install, and fact 0046 says it was run.

### 1.7 Measure idle memory with 1, 3 and 5 instances, then trim — [x] ‖
Class: explore · Needs: none · Check: `test -s docs/facts/0047-idle-memory-per-server-instance.md`
Context: Decision 0115: no hard limit, as low as possible. Candidate trims, cheapest first: cap the tokio runtime threads (`#[tokio::main]` in `packages/server/src/main.rs` uses one worker per core); `[profile.release]` in the workspace `Cargo.toml` (none today: `opt-level = "s"`, `lto`, `strip`, `panic = "abort"`); a better allocator than musl's malloc for the static build (`mimalloc` or `jemalloc` with decay tuning); a hard cap on scrollback and output buffers per session and per instance.
Do: 1. Start 1, 3 and 5 instances with separate config dirs and ports on the dev container (`scripts/dev.sh`), idle and with 3 sessions each; record RSS (`ps -o rss=`) in a new fact `docs/facts/0047-idle-memory-per-server-instance.md` (add its row to `docs/facts/INDEX.md`). 2. Try each trim above on its own; keep one only when it lowers the number without hurting latency or the static build; one PR per kept trim. 3. Do not add `MemoryMax`.
Done-when: fact 0047 holds the before and after numbers for 1, 3 and 5 instances and each kept trim is merged or explicitly rejected with its measurement.

### 2.1 R23 decision record for a bundled static tmux — [x] ‖
Class: plan · Needs: none · Check: `grep -l "tmux" docs/decisions/*.md | xargs grep -l "bundled"`
Context: `docs/plans/roadmap/R23.md` (read it first): bundle a pinned static tmux per architecture so a native install needs no root to install tmux and gets control mode (tmux 3.2 or newer). Independent of group 1; it removes the "install tmux" root step from user installs.
Do: Write the decision record the R23 plan asks for first: licence (ISC), pinned version, per-arch musl static build with libevent and ncurses, terminfo, who owns security updates, how `.github/workflows/release.yaml` signs and carries it, resolution order `TERMENBOX_TMUX`, bundled, `PATH`. Number it with the next free decision number and add its INDEX row.
Done-when: the decision exists, is accepted by the owner, and is indexed.

### 2.2 R23 build the static tmux in the release pipeline — [x] 
Class: implement · Needs: 2.1 · Check: `grep -q "tmux" .github/workflows/release.yaml`
Context: R23 step 1 and the decision from 2.1. `release.yaml` already builds the server for x86_64 and aarch64 musl and signs a manifest.
Do: Build tmux for both architectures with the pinned version, include it and its hash in the release manifest (`.github/scripts/make_manifest.py`), and verify it runs with an empty `PATH` and no terminfo on the host.
Done-when: a release dry run lists a tmux artifact per architecture in the manifest and CI starts it successfully.

### 2.3 R23 resolve, install and update the bundled tmux — [x] 
Class: implement · Needs: 2.2 · Check: `scripts/dev.sh cargo test -p termenbox-server -- backend updater`
Context: Every tmux call goes through `Runner` in `packages/server/src/backend.rs`. `install.sh` currently stops with a printed command when tmux is missing (0113). `packages/server/src/updater.rs` updates the server with rollback.
Do: 1. In `Runner` resolve `TERMENBOX_TMUX`, then the bundled copy, then `PATH`. 2. `install.sh` downloads and verifies the tmux binary like the server binary, places it where the run user can reach it (`~/.local/bin` for `host-me`), and journals it; remove the "tmux missing, stop" step. 3. The updater updates it with the server and rolls back together. 4. Docker installs keep the distribution tmux. 5. Tests for the resolution order and for an install on a host with no tmux.
Done-when: a native install on a host with no tmux, or tmux older than 3.2, reaches a working session with control mode on, and the release manifest covers the tmux binary.

## After this plan

Not built here and recorded for later: sharing a session with another account (0094 / R38), a second OAuth provider (a login flow on the hosted function, then a new `provider` value), root-provisioned or in-app creation of instances for other users (rejected for now in 0115).
