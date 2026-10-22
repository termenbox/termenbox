# Add Server wizard, pairing link and short code

Plan for DECISION-0126, 0127, 0128, GUARDRAIL errors-fit-one-screen and pairing-code-guesses-are-bounded, tracker item R65.
Read those five files and `docs/plans/roadmap/R62.md` first. Repo: Termenbox (Rust server in `packages/server`, shared types in `packages/core`, Svelte 5 client in `packages/client`). Work on a branch (never master), one squashed PR, commit subjects `type(scope): summary`. No compiler on this machine: use `scripts/dev.sh <cmd>` (DECISION-0114).

Legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved. Current state at writing (the installer has since moved to `packages/client/installer/install.sh`, is served at `/i` and takes no secret; see Notes): the default installer (`packages/client/public/install.sh`) prints no QR when given a secret; the QR comes from `sh -s -- -` via `termenbox-server --register --qr` (`packages/server/src/main.rs` `print_registration`, `auth::claim_qr`); `--print-pairing` (R62) prints a bundle link and QR separately; `packages/client/src/AddServerModal.svelte` has tabs Paste and Scan, with the scan buttons below the command.

### 1.1 Spike the PAKE and iOS Local Network behaviour — [ ]
Class: explore · Needs: none · Check: `grep -q 'iPhone' docs/facts/0052-pairing-code-pake-and-ios-local-network-spike.md`
Context: DECISION-0128 needs a PAKE for the claim proof and a stretch for the mailbox key. **The PAKE half is done** (FACT-0052, 2026-10-22): CPace over ristretto255 fits in Rust (`curve25519-dalek` is already in the tree) and in TypeScript (`@noble/curves`, about 14 kB gzip, no WASM; the client CSP has no `wasm-unsafe-eval`); a handshake costs 0.31 ms in Rust and about 2.4 ms per scalar multiplication in Node. What remains needs physical devices, which also decide R62 step 1.
Do: On a real iPhone (Safari and the installed PWA) and an Android phone, open the hosted client and reach a LAN server over WebSocket and WebRTC; note the Local Network permission prompt and what the page sees when it is denied. Time the stretch (Argon2id or scrypt, pure JS) on the slowest phone and set the parameter (about 100 to 300 ms). Check whether `crypto.subtle` verifies Ed25519 on iOS 16.4 (the floor, decision 0112); if not, `lib/bundle.ts` needs the noble fallback. Add the results, labelled by platform, to FACT-0052 and set it `active`. Procedure: `docs/skills/walk-the-add-server-paths.md`, section "Connectivity spike".
Done-when: FACT-0052 holds iPhone and Android results labelled by platform, the stretch parameter, and the Ed25519 check.

### 1.2 One pairing output in the server — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh cargo test -p termenbox-server pairing`
Context: DECISION-0126. Today `--register` prints a `#claim=` link, `--print-pairing` prints a `#/pair/` link, and the client parses them in `packages/client/src/lib/onboard.ts` and `lib/pair.ts`. Candidate bundles are signed (`packages/server/src/candidates.rs`, `packages/core` bundle module).
Do: In `packages/server/src/main.rs` add one function that returns the pairing block: a `https://<client>/#/pair/<payload>` link whose payload holds the claim token (when unowned) and the signed bundle, the terminal QR from `auth::claim_qr` (at most 60 columns), and the short code placeholder (filled by 2.3). For an owned server it omits the claim and keeps the bundle. Make `--register` and `--print-pairing` both print it. Extend the payload layout and shared Rust/TypeScript test vectors in `packages/core` and `packages/client/src/lib/bundle.ts` without breaking existing links.
Done-when: unit tests show the link decodes to the claim and bundle, the owned variant has no claim, the QR fits 60 columns, and old `#claim=` and `#/pair/` links still parse.

### 1.3 Installer prints the block on every run — [x]
Class: implement · Needs: 1.2 · Check: `scripts/dev.sh sh -c 'cd packages/server && cargo test --test install_script'`
Context: `packages/client/public/install.sh` prints the QR only when the secret is `-` (lines near 314 and 346) and otherwise says "It will appear in the app within a minute." Both container (`docker exec … --register --qr`) and native (`native_claim`) paths exist.
Do: In both paths always run `--register --qr` and print its block (QR, link, code) with one line of plain explanation ("Scan this, open the link, or type the code in Add Server"). Keep the secret shortcut: `--claim "$SECRET"` still runs first. A re-run on an owned server prints the owned variant. Keep POSIX `sh`; run shellcheck.
Done-when: the install-script test shows the block after a secret install, after a `-` install, and after a re-run on an owned server, and shellcheck is clean.

### 2.1 Wizard state and Connect checklist logic — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: DECISION-0127. `AddServerModal.svelte` mixes the flow with the UI; `lib/onboard.ts` has `waitForServer`, `previewClaim`, `checkReachable`; `lib/paths.ts` races candidates; `lib/pair.ts` verifies a pairing link.
Do: Add `packages/client/src/lib/addServer.ts`, a small state machine with steps install, connect, confirm, done and the connect sub-states (code read, looking, found, reading), each address with tried/failed/won and a reason enum, and a mapping from failure to cause (different network, guest isolation, Local Network denied, server stopped, link expired, UDP closed, wrong code) and one action each. Unit-test the transitions and the mapping with fake openers.
Done-when: the tests cover each transition and each cause, and the module has no DOM imports.

### 2.2 The four-step wizard UI — [x]
Class: implement · Needs: 2.1, 1.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/run.mjs'`
Context: DECISION-0127 and GUARDRAIL errors-fit-one-screen. Replace the tabs in `AddServerModal.svelte`; keep `Dialog` from `lib/ui`. The run-mode form (container, host user, me, root, with consent) is moved, not changed, per GUARDRAIL host-mode-explicit-consent.
Do: Build step components: Install (one command, Copy, "Options" fold holding name and run mode), Connect (live checklist, inline "Scan it" with the camera at the top using `lib/scan.ts`, paste-link field, code field, per-address results in a fold), Confirm (the 0117 preview and name), Done (active path and plain fix). Show one step at a time with a step indicator. Failures use 2.1's cause and action, details folded, working WebSocket fallback as a notice. Make `PortHelp` a folded detail.
Done-when: the e2e walks Install to Done on loopback LAN pairing, and the scan button and camera are in the first viewport at 375x667.

### 2.3 Short code on the server: the PAKE endpoint — [ ]
Class: implement · Needs: 1.1 (PAKE choice; done), 1.2 · Check: `scripts/dev.sh cargo test -p termenbox-server pairing_code`
Context: DECISION-0128 and GUARDRAIL pairing-code-guesses-are-bounded. Control endpoints stay on 127.0.0.1 (GUARDRAIL internal-network-binding); this endpoint is the one unauthenticated surface on the server's port and must stay minimal. FACT-0021 gives the half-open cap. FACT-0052 says the PAKE libraries give a key, not authentication.
Do: Generate the 8-character base32 code with the claim, print it with the short fingerprint in the pairing block, and implement CPace (the current CFRG draft, with the draft's test vectors shared by Rust and TypeScript) plus key confirmation on the existing server port, with the DTLS fingerprint in the associated data. Enforce expiry, single use, per-source backoff, the 20-failure burn and the reply-size rule. Add every test in the guardrail's table that applies to the server, including a fuzz target.
Done-when: those tests exist and pass, and the code never appears in a log (GUARDRAIL security-secrets-handling).

### 2.5 The pairing mailbox — [ ]
Class: implement · Needs: 2.3 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm test'`
Context: DECISION-0128 and FACT-0022 (capacity). Writes go through one Pages Function; reads are public R2 GETs on a custom domain and run no Worker. `infra/termenbox-cloud` is Terraform.
Do: Add the upload route (server-key signature, per-key limit, atomic D1 daily counter, delete on claim), the R2 bucket, its custom domain and a one-day lifecycle rule to Terraform (check the cloudflare provider version supports the R2 custom domain; the zone and token must allow it); on the server, upload the encrypted bundle after start and on a claim change, never with `TERMENBOX_LAN_ONLY=1`. Add the guardrail tests for the mailbox (`mailbox_write_needs_server_signature`, `misses_are_indistinguishable`, `mailbox_object_is_ciphertext`).
Done-when: those tests pass, the plan output for the Terraform change is attached to the PR, and the daily cap refuses writes while the installer still prints the link and QR.

### 2.4 Scan QR outside the wizard — [x]
Class: implement · Needs: 2.2 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: DECISION-0127. `ServerList.svelte` and the header are where the entry point goes; `lib/scan.ts` reads the camera.
Do: Add a Scan QR button to the server list and header that opens the camera full-screen and, on a pairing link, goes straight to the Confirm step; a re-add link for a known server selects it (`onserver`). The camera prompt copy explains why.
Done-when: a test shows a scanned pairing link reaches Confirm with no Install or Connect step, and the button is visible without scrolling at 375x667.

### 3.1 Short-code client path — [ ]
Class: implement · Needs: 2.2, 2.3, 2.5 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm test'`
Context: the client has no PAKE yet; step 1.1 chose CPace. A typed code is a mailbox lookup (DECISION-0128), not a LAN guess.
Do: Implement the stretch, the mailbox fetch and decrypt, bundle verification and expiry check, the CPace claim proof with key confirmation, the code field in Connect (the `XXXX-XXXX` format, case-insensitive), Confirm showing the server name and the fingerprint to compare, and the "wrong code / code burned, re-run the installer" messages.
Done-when: tests show a right code reaches Confirm, a wrong code shows one action, a burned code says to re-run the installer, and an unequal fingerprint is refused.

### 3.2 Error-fit tests — [x]
Class: implement · Needs: 2.2, 2.4 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/run.mjs'`
Context: GUARDRAIL errors-fit-one-screen lists `error_states_fit_375x667` and `details_are_folded`.
Do: Add browser e2e cases that render each failure state (expired link, not found, UDP closed with and without fallback, permission denied, wrong code, burned code) at 375x667 and assert no vertical scroll and closed detail folds.
Done-when: both named tests pass in CI.

### 4.1 Walk the paths on real devices — [ ]
Class: review · Needs: 3.1, 3.2, 1.3 · Check: `sed -n '/^## Notes/,$p' docs/plans/add-server-wizard.md | grep -q iPhone`
Context: skill `docs/skills/walk-the-add-server-paths.md`; GUARDRAIL a-phone-check-names-its-platform.
Do: Run the procedure on a real iPhone, a real Android phone and desktop. Record the matrix, tap counts and screenshots in the PR and the results under Notes below.
Done-when: Notes holds a filled matrix with each result labelled by platform.

### 4.2 Docs and index — [ ]
Class: implement · Needs: 4.1 · Check: `python3 docs/plans/roadmap/check.py && python3 docs/plans/roadmap/rank.py`
Context: README and `docs/BOOTSTRAP.md` describe the old commands; the instruction file is capped at 150 lines.
Do: Update README and BOOTSTRAP to the wizard and the printed block; at most one line in the instruction file; mark R65 done and move it to `docs/plans/roadmap/archive/` per that directory's convention; update 0123's text if the spike changed it.
Done-when: both scripts exit 0 and the instruction file is within 150 lines.

## Notes
Done on branch `feat/add-server-wizard`: 1.2, 1.3, 2.1, 2.2, 2.4, 3.2. Checks: server unit tests and `install_script`/`pairing_cli` integration tests, 18 `addServer` unit tests, shellcheck clean, and `e2e/failures.mjs`, `e2e/wizard.mjs`, `e2e/pair.mjs`, `e2e/add-server.mjs` all pass in Chromium with mobile emulation (not an iPhone or Android).

Deviations:
- The QR is 69 columns at the lowest correction level, not 60: a claim plus a signed bundle needs a version-13 code. The test caps it at 80.
- Scan QR outside the wizard opens the Connect view (camera and live checklist) rather than skipping it entirely; no Install step is shown.
- `AddServerModal` has no vitest mount test: vitest here resolves Svelte's server build, so the Scan QR path is covered by `e2e/wizard.mjs` with a fake camera.

Not done: 1.1 (PAKE and iOS spike, needs physical devices), 2.3 and 3.1 (wait on 1.1; the code field is not in Connect yet), 4.1 (real iPhone, Android and desktop walk), 4.2.

Updated 2026-10-22 (branch `feat/server-version`):
- Done: the bare command `curl -fsSL <client>/i | sh` (default this machine as the current user; one mode word for the others), the client build baking its address into `/i`, no secret in the command (`scanCommand` removed; the installer takes a claim only from `TERMENBOX_CLAIM`), an unclaimed server no longer re-announcing every 5 minutes, the Connect lead and "Nothing yet?" hint, the `announced_unreachable` failure, `--version`.
- Re-planned: 2.3 and 3.1 follow the mailbox design of the rewritten 0128; 2.5 is new. 1.1 is half done (FACT-0052).
- Without the mailbox, Connect finishes only by scan or pasted link; there is no automatic detection and no code field until 2.5 and 3.1 land.
- Not done: 1.1 (phones), 2.3, 2.5, 3.1, 4.1, 4.2.
