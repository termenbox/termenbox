# Termenbox Roadmap: every open step in one plan, ordered by tier, dependencies and WSJF

Written 2026-10-14 by consolidating plans v1-v10 and session-tabs (now in `docs/plans/archive/`); restructured 2026-10-16 around the UX programme and cleaned of done, superseded and folded steps (they are in `roadmap/archive/`). This file alone is enough for a cold session.

## Cold start

Repository: Rust server `packages/server`, shared wire types `packages/core/src/protocol.rs` (mirrored by hand in `packages/client/src/lib/protocol.ts`), Svelte 5 client `packages/client/src` (`npx --yes pnpm@9`), directory Pages Functions `packages/client/functions/api`. Knowledge base in `docs/{facts,decisions,guardrails,skills}`; read `CLAUDE.md` first. One squashed pull request per step from its own branch off master, green CI, merge only on the owner's "merge" (guardrails `changes-land-as-one-squashed-pull-request`, `no-commits-on-the-default-branch`). Constraints: 512 MB RAM, 1 vCPU, no paid services in the required stack, secrets never logged, no check runs locally (decision 0108: CI is the judge). The owner tests on an iPhone; Android has never had a real-device check (fact 0039), so label every phone check by platform and never call an emulator a phone (guardrail `a-phone-check-names-its-platform`). Never touch the owner's own `termenbox.service` (read its journal only). Pre-push hook: commit subject `type(scope): ...` with a one-word scope. Plans are tracked in GitHub issues (decision 0049); ask before writing to the tracker.

## Priority: the UX programme comes first

Tier **P0** is the UX programme (`ux-programme.md`, decisions 0100-0107, 0109 and 0110): decision-first Home, act in place, tiered session view for every agent including ACP, a first-class terminal with a configurable key row, reports, a desktop workspace, and the real-device gate. Its steps are `roadmap/U02.md` to `roadmap/U16.md`, ranked in plan order; U01 (the design spike) is done and its result is `docs/plans/assets/ux/DIRECTION.md`. R03 is P0 too: it is the owner's real-iPhone checks that the programme needs. Lower tiers wait unless a P0 step lists them as a predecessor, in which case they sit directly before it (R36 before U16). The free gate (R40) requires the programme's device pass (U16).

## How to read and use this plan (cheap to re-enter and update)

1. Read only this file. The Order table is generated: one row per open step, in the order to work. **A row is ready when none of its Preds is still listed**; take the first ready row whose Status is `pending`.
2. Open that step's file (`roadmap/Rnn.md` or `roadmap/Unn.md`, 5-40 lines: header, then Class and Check, Context, Do, Done-when). Read nothing else unless the step names it.
3. When the step lands, move its file to `roadmap/archive/` (`git mv`), set `Status: done (PR #n)`, drop it from other steps' `Preds:`, then run `python3 docs/plans/roadmap/check.py && python3 docs/plans/roadmap/rank.py`. To add or re-score a step, create or edit its file (copy a neighbour's header) and run the same two commands. `check.py` fails on a missing field, an unknown or archived predecessor, a cycle, or a step with no Done-when or Check.
4. Old step names inside the text (G.4, O.2, K.1) are from the plans in `docs/plans/archive/`; each file's `Was:` line says where it came from. Step ids `U.n` in older text are `Un` with a zero pad (U.7a is `U07a`). The design rules in `docs/plans/assets/ux/DESIGN-PRINCIPLES.md` are `DR1`-`DR15` and `K1`-`K6`, not roadmap steps.

Header fields (every open step has all of them):

| Field | Values | Meaning |
|---|---|---|
| Status | `pending`, `blocked (why)` | done, superseded and folded steps are in `roadmap/archive/` |
| Tier | `P0` the UX programme and its real-device blockers; `P1` foundations, gates and terminal or agent work that P0 needs or protects; `P2` valuable free features and hygiene; `P3` later or optional; `Paid` after the free gate | priority class; the table sorts by tier, then WSJF, always after predecessors |
| Area | `ux`, `terminal`, `agents`, `server`, `security`, `cloud`, `release`, `ci`, `docs`, `connectors`, `paid` | the field of work |
| Platform | `mobile`, `desktop`, `both`, `n/a` | which client the step serves; `n/a` means no client surface (server, cloud, docs, CI) |
| Preds | step ids or `none` | open predecessors only |
| Score | `V= T= R= S=` | value, time criticality, risk or opportunity (1-10), size (1, 2, 3, 5, 8, 13); WSJF = (V+T+R)/S, the author's estimate; a step takes the best score of any chain it unlocks |
| Lane, Pin | `free` or `paid` (must agree with Tier `Paid`); `Pin: after Xnn` keeps a step directly after another (the UX programme uses it to keep plan order) | |

Scoring procedure: `docs/skills/prioritise-the-roadmap-by-wsjf.md`. Lane `paid` steps come after the free ones (decision 0093).

## Order

Sorted by tier first (P0 before P1 and so on), then by Chain, then by the step's own WSJF, and never before a predecessor; pins override. WSJF is the step's own score; Chain is the best bundle score of the steps it unlocks (or its own, if higher), so a cheap prerequisite of a valuable step rises above its own WSJF. A lower-tier step with a higher WSJF therefore sits below a higher-tier one (R19 under P1 above R10 under P2).

<!-- order:start -->
| # | Step | Title | Tier | Area | Platform | Status | WSJF | Chain | Preds |
|---|---|---|---|---|---|---|---|---|---|
| 1 | [R03](roadmap/R03.md) | Real-iPhone checklist and two-real-server e2e (G.10, H.11) | P0 | ux | mobile | blocked (owner's iPhone checklist, docs/skills/mobile-ui-device-check.md) | 8.0 | 8.0 | - |
| 2 | [U11](roadmap/U11.md) | Escalation ladder | P0 | server | both | pending | 2.2 | 4.6 | - |
| 3 | [R36](roadmap/R36.md) | Desktop browser regression matrix and WebKit in CI | P1 | ci | both | pending | 4.2 | 4.6 | - |
| 4 | [U16](roadmap/U16.md) | Real-device gate and the free gate | P0 | ux | mobile | pending | 8.3 | 8.3 | U11, R03, R36 |
| 5 | [R07](roadmap/R07.md) | Request-budget alarm (C.5) | P1 | cloud | n/a | pending | 5.0 | 5.0 | - |
| 6 | [R59](roadmap/R59.md) | Spike: search tmux history and local terminal text | P1 | terminal | both | pending | 5.0 | 5.0 | - |
| 7 | [R09](roadmap/R09.md) | Directory abuse controls and degraded mode (S.4) | P1 | cloud | n/a | pending | 4.0 | 4.0 | - |
| 8 | [R61](roadmap/R61.md) | Open terminal web links with a desktop pointer | P1 | terminal | desktop | pending | 4.0 | 4.0 | - |
| 9 | [R21](roadmap/R21.md) | Verify the Codex adapter against a real Codex (P.5) | P1 | agents | n/a | pending | 3.3 | 3.3 | - |
| 10 | [R19](roadmap/R19.md) | Cache event and Claude status-line forwarder (G.8) | P1 | agents | n/a | pending | 2.6 | 2.6 | - |
| 11 | [R10](roadmap/R10.md) | Spike: connector payload and abuse review (O.1) | P2 | connectors | n/a | pending | 8.5 | 8.5 | - |
| 12 | [R22](roadmap/R22.md) | Badge, stats sheet and Say thanks (T.2) | P2 | ux | both | pending | 6.0 | 6.0 | - |
| 13 | [R32](roadmap/R32.md) | Key rotation and incident runbook (S.8) | P2 | security | n/a | pending | 4.3 | 4.3 | - |
| 14 | [R26](roadmap/R26.md) | Directory deregistration (D.2) | P2 | cloud | n/a | pending | 3.7 | 3.7 | - |
| 15 | [R34](roadmap/R34.md) | Self-hosted client docs (rest of 5.1) | P2 | docs | n/a | pending | 3.5 | 3.5 | - |
| 16 | [R25](roadmap/R25.md) | Server settings screen: name, mode, re-claim (B.3) | P2 | server | both | pending | 3.3 | 3.3 | - |
| 17 | [R17](roadmap/R17.md) | Signed records on R2 behind the CDN (C.3) | P2 | cloud | n/a | pending | 2.4 | 3.2 | R09 |
| 18 | [R18](roadmap/R18.md) | Owner-keyed server list and settings sync (C.4, 0068) | P2 | cloud | both | pending | 1.5 | 3.2 | R17 |
| 19 | [R11](roadmap/R11.md) | Rule engine, connector trait and outbound guard (O.2) | P2 | connectors | n/a | pending | 2.0 | 3.0 | R10 |
| 20 | [R64](roadmap/R64.md) | Server reachability self-check and automatic port opening | P2 | server | n/a | pending | 2.6 | 2.6 | - |
| 21 | [R27](roadmap/R27.md) | Remote uninstall on the native host (N.6) | P2 | server | n/a | pending | 2.2 | 2.5 | - |
| 22 | [R50](roadmap/R50.md) | Release channels and auto-update policies for server and client | P2 | release | both | pending | 2.4 | 2.4 | - |
| 23 | [R51](roadmap/R51.md) | Cloudflare isolation, usage dashboard and threshold alerts | P2 | cloud | n/a | pending | 2.4 | 2.4 | R07 |
| 24 | [R62](roadmap/R62.md) | Candidate paths: signed bundle, LAN and pairing, raced connect, seamless re-attach | P2 | server | both | pending | 2.2 | 2.3 | - |
| 25 | [R31](roadmap/R31.md) | Passkey step-up for root sessions and device revocation (S.2) | P2 | security | both | pending | 2.1 | 2.1 | - |
| 26 | [R28](roadmap/R28.md) | Remove server flow in the app (D.4, container parts of D.3) | P2 | server | both | pending | 2.0 | 2.0 | R26, R27 |
| 27 | [R38](roadmap/R38.md) | Session sharing links, read-only and read-write (new) | P2 | server | both | pending | 1.8 | 1.8 | - |
| 28 | [R39](roadmap/R39.md) | Image and file upload to a session, with threat review (new) | P2 | terminal | both | pending | 1.6 | 1.6 | - |
| 29 | [R33](roadmap/R33.md) | Managed quick-tunnel fallback for hosts with no UDP port (8.3) | P2 | server | n/a | pending | 1.0 | 1.0 | - |
| 30 | [R16](roadmap/R16.md) | Rules screen (O.6) | P3 | connectors | both | pending | 2.4 | 2.4 | R11 |
| 31 | [R63](roadmap/R63.md) | Connection telemetry: sampled, enum-only, opt-out on the hosted client | P3 | cloud | both | pending | 2.4 | 2.4 | R62 |
| 32 | [R14](roadmap/R14.md) | Telegram and generic webhook (O.4) | P3 | connectors | n/a | pending | 2.2 | 2.2 | R11 |
| 33 | [R29](roadmap/R29.md) | Migrate Docker host installs (N.7) | P3 | server | n/a | pending | 1.8 | 1.9 | - |
| 34 | [R30](roadmap/R30.md) | Remove the Docker host code (N.8) | P3 | server | n/a | pending | 2.0 | 2.0 | R29 |
| 35 | [R15](roadmap/R15.md) | Session and model connectors (O.5) | P3 | connectors | n/a | pending | 1.4 | 1.4 | R10, R11 |
| 36 | [R24](roadmap/R24.md) | Environment profile on the native host (replaces B.4-B.6) | P3 | server | both | pending | 1.1 | 1.1 | - |
| 37 | [R40](roadmap/R40.md) | The free gate check (P.1) | Paid | paid | n/a | pending | 0.0 | 3.2 | R03, R18, U16 |
| 38 | [R41](roadmap/R41.md) | Entitlement record and Sponsors webhook (P.2) | Paid | paid | n/a | blocked (R40, the free gate) | 2.6 | 3.2 | R40 |
| 39 | [R46](roadmap/R46.md) | Privacy notice and opt-ins for Pro data (P.7) | Paid | paid | both | blocked (R40, the free gate) | 4.7 | 4.7 | R41 |
| 40 | [R42](roadmap/R42.md) | Pro: managed relay spike (P.3) | Paid | paid | n/a | blocked (R40, the free gate) | 4.3 | 4.3 | R41 |
| 41 | [R43](roadmap/R43.md) | Pro: encrypted history format and recovery key (P.4) | Paid | paid | n/a | blocked (R40, the free gate) | 1.4 | 2.8 | R41 |
| 42 | [R47](roadmap/R47.md) | Team: organisation, seats, merchant of record (P.8) | Paid | paid | n/a | blocked (R40, the free gate) | 0.7 | 2.6 | R40, R41 |
| 43 | [R48](roadmap/R48.md) | Enterprise design note (P.9) | Paid | paid | n/a | blocked (R40, the free gate) | 1.5 | 1.5 | R47 |
| 44 | [R44](roadmap/R44.md) | Pro: reliable push and inbox retention (P.5) | Paid | paid | both | blocked (R40, the free gate) | 1.2 | 1.2 | R43 |
| 45 | [R45](roadmap/R45.md) | Pro: fleet view and usage reports (P.6) | Paid | paid | both | blocked (R40, the free gate) | 1.2 | 1.2 | R43 |

45 open steps. 29 done and 9 superseded or folded steps are in `roadmap/archive/` (their text is kept; `grep -h '^Status' roadmap/archive/*.md`).
<!-- order:end -->

## Paid tiers (decisions 0089-0093, fact 0035)

| Tier | Contents | Source |
|---|---|---|
| Free | Sessions, agents, tabs, notifications, Web Push, settings and server-list sync (0068), read-only and read-write session sharing, several users per server, isolated per-user environments (need host mode, 0065). No artificial caps | 0089 |
| Pro | Managed relay, cloud session history, reliable push with inbox, fleet view and usage reports. All client-encrypted, recovery key | 0090 |
| Team | Organisation, seats and invoices, shared directory and groups, shared inbox and history, provisioning of isolated environments across servers | 0091 |
| Enterprise | SSO/SAML, central audit retention and export, support. RBAC and DLP enforced on the host, metadata-only reporting. Not scheduled until a customer exists | 0091 |

Billing: GitHub Sponsors webhook sets an entitlement on the GitHub id; a merchant of record arrives with Team (0092).

Never build into a paid tier what `docs/skills/classify-a-feature-into-a-tier.md` puts in free. R40-R48 are the paid lane (`Lane: paid`).

## Already done (do not redo)

Verified against the code and `git log` on 2026-10-14, plus what landed since: browser-capability facts, green baseline, single-owner binding, reconnect and replay, signed directory, one-command installer, WebRTC direct (server and client), mobile terminal UX (#46), installable PWA (#51), browser e2e and resource gates (#44), session list and templates (session-tabs H.10), herdr and backend verdict (fact 0029, #82), run modes, device-key credentials (#55), web baseline CSP (#41), signed releases, root helper and updater (#74, #77), change journal (#73), static musl binary (#72), agent events and adapters (v6 P.1-P.6 except Codex), session cards, answers and replies (v8 K.2-K.5), navigation G.1-G.5, session tabs H.1-H.10, swipe typing fixes, remote uninstall stage 1 (D.1), usage meter (T.1), problem reports (R.1), connection manager, AppShell and viewport fix; since then Tailwind v4 and shadcn-svelte (R05, #115), announce on change (R06, #112), session sharing decision (R37, #117), New window (R52, #118), share a server to another device (R49, #110), the agent status redesign with Web Push (R54 and R12, #135), and the UX design spike (U01, #150). Each is a file in `roadmap/archive/`.

## Open questions for the owner

1. PR #139 ("unified agent hooks for five agents") is listed as merged on 2026-10-15 but its merge commit `cb1098c0` is not reachable from master and `agent_hooks.rs` is absent. Was it reverted? If it returns, its auto-install on attach conflicts with the opt-in rule of decisions 0079 and 0106 (see U03, U10).
2. Decisions 0109 (supersedes 0102), 0110, 0111, 0068 and 0080 were accepted by the owner on 2026-10-16; no decision draft is waiting.
3. PR #149 (desktop review) is folded into this roadmap (R55 to U15, R56 and R60 to U07c and U15, R57 and R58 to U04 and U15; R59 and R61 kept; R36, R39 and R50 enriched). Close it once this lands.
4. R03 needs the owner's iPhone (R02 is done); U16 (real-device pass, iPhone and Android) needs it too, and now also carries U04 step 0 (the iOS keyboard hypotheses K1, K2, K4), which was not run before U04 landed.

## Backlog (unscored, no step yet)

- Connect Cloudflare for stable tunnels (was v1 8.6, #29): for owners with their own domain; needs a go/no-go decision. R33 covers hosts with no UDP port without an account.
- Own server-native multiplexer backend (was v1 7.2, #23): only if tmux limits bite; `SessionBackend` (0054) allows it later; R23 bundles a static tmux first.
- Model connector (v7 question 3): deferred.
- A production terminal Find: waits for the result of the R59 spike.
- Tablet layouts (the M3 split idea from the UX spike): after the real-device pass.
