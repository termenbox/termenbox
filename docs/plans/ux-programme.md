# Termenbox Plan: the UX programme (decision-first Home, act in place, tiered session view, reports, desktop workspace)

Written 2026-10-16 from the roadmap interview. Decisions 0100-0107 (all active, as are 0109, 0110 and the ACP tier decision 0111, accepted 2026-10-16) and guardrails `ux-acceptance-targets`, `a-phone-check-names-its-platform`. Replaces decision 0088, amends 0079 and 0090. Builds on the finished status redesign (`docs/plans/archive/agent-status-redesign.md`, decisions 0095-0097) and replaces roadmap steps R20 (card design pass), R13 (inbox view), R53 (dictation, folded into U.8) and, in part, R45 (on-server part); the desktop review (PR #149) is folded in: R55 into U15, R56 and R60 into U07c and U15, R57 and R58 into U04 and U15. This file alone is enough for a cold session. It is the highest priority tier (P0) of `docs/plans/termenbox-roadmap.md`: its steps are the files `roadmap/U02.md` to `roadmap/U16.md` there, in this order, with their status, predecessors, platform and score in each file. This file keeps the goal, the shared design and the cold start; it no longer carries step text.

## Cold start

Repository: Rust server `packages/server/src` (`events.rs` hub, `agentwatch.rs` scan and screen watchers, `adapters.rs`, `sessions.rs`, `rtc.rs`, `websocket.rs`, `control.rs`, `claude_hooks.rs`, `main.rs`), shared wire types `packages/core/src/{protocol,events}.rs` mirrored by hand in `packages/client/src/lib/protocol.ts`, Svelte 5 client `packages/client/src` (`App.svelte`, `AppShell.svelte`, `NavigationBar.svelte`, `TabStrip.svelte`, `SessionSheet.svelte`, `SessionCard.svelte`, `NeedsYouBanner.svelte`, `InboxSheet.svelte`, `QuickReply.svelte`, `KeyRow.svelte`, `lib/agents.ts`, `lib/inbox.ts`, `lib/agentIcons.ts`), client e2e in `packages/client/e2e` (Playwright via `run.mjs`, `cards.mjs`, `navigation.ts`). Knowledge base in `docs/{facts,decisions,guardrails,skills}`; read `CLAUDE.md` first, then decisions 0100-0107. Tests: `cargo test --workspace`; `cd packages/client && npx --yes pnpm@9 test`. One squashed pull request per step from its own branch off master, green CI, merge only on the owner's "merge" (`changes-land-as-one-squashed-pull-request`, `no-commits-on-the-default-branch`); commit subject `type(scope): ...` with a one-word scope. Constraints: 512 MB RAM, 1 vCPU, no paid services, secrets never logged, builds only in CI, agent text is untrusted and never in notifications (`notifications-carry-no-agent-text`). The owner tests on an iPhone; Android has never had a real-device check (fact 0039), so label every phone check by platform (`a-phone-check-names-its-platform`) and never call an emulator a phone. Never touch the owner's own `termenbox.service`. Plans are tracked in GitHub issues (0049); the tracker answer is in `tracker-writes-follow-the-answer`.

## Goal

A zero-friction product where the user never has to know where anything is: the app opens on what needs them, they decide in place (choose, type, dictate) and rarely need the terminal, yet the terminal is first-class and super-convenient when they want it (decision 0109, which supersedes 0102). Skins are themeable through tokens (decision 0110). Phone is designed as a phone (bottom bar, thumb reach), desktop as a desktop (three panes, palette, shortcuts). Reporting covers a live board, a timeline and digest, usage and cost, and server health. Targets (guardrail `ux-acceptance-targets`): push to sent answer in at most 2 taps; every agent's state on one screen at 0 taps after cold open; any session in 1 tap from Home; first usable paint under 1.5 s on a mid-range phone over 4G; a device check per screen.

## Design in one place (from 0100-0107)

- Destinations: Home, Sessions, Activity, Settings. Phone: bottom bar and full-height session sheet. Desktop: sidebar, main pane, optional right pane (terminal or second session), palette Ctrl/Cmd+K.
- Home: Needs you queue (needs_input, failed, finished_unseen; oldest wait first) plus fleet summary plus Continue card. Queue changes never move under a finger.
- Session view tiers: T0 ACP (sessions termenbox starts itself in Agent Client Protocol mode, U.7a-U.7b), T1 hooks and transcript (Claude now, Codex after R21), T2 per-agent screen profile, T3 generic. T0 applies only to sessions termenbox spawns; sessions the user runs in tmux stay T1 to T3. Every agent with an icon gets at least T3, plus T2 for the priority list set in U.3.
- Decision card: plain-terms context, large option buttons, free-text composer with dictation; no standing rules. Escalation ladder with a deep-link push that carries no agent text.
- Reports stored on each server, metadata only, bounded; free on-server; cloud archive and cross-server retention stay Pro.
- Hook offer: one inline card per agent, Enable / Not now / Never; "Not now" re-offers only after a manual terminal answer or 14 days.

Legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved.

## Steps

One roadmap file per step (the old numbering `U.n` equals `Un` with a zero pad: U.7a is `U07a`). The table below is a snapshot; the roadmap's generated table and each file are authoritative.

| Step | Title | Platform | Needs | Status |
|---|---|---|---|---|
| [U01](roadmap/archive/U01.md) | Design spike: three directions, owner picks one | both | none | done (PR #150) |
| [U02](roadmap/archive/U02.md) | Codify the design system | both | none | done (PR #158) |
| [U03](roadmap/archive/U03.md) | Measure which agents expose what; set the T2 priority list | n/a | none | done (PRs #153, #154, #155) |
| [U04](roadmap/archive/U04.md) | App shell: bottom bar, sidebar, palette, four destinations | both | U02 | done (PR #159) |
| [U05](roadmap/archive/U05.md) | Home: Needs you queue, fleet summary, Continue | both | U04 | done (PR #161) |
| [U06](roadmap/archive/U06.md) | Session-view framework: T3 view, composer, profile engine | both | U02 | done (PRs #160, #165) |
| [U07](roadmap/archive/U07.md) | T2 profiles for the priority agents | n/a | U03, U06 | done (PR #186; codex has no profile) |
| [U07a](roadmap/archive/U07a.md) | ACP backend: agents termenbox starts itself | n/a | U03, U06 | done (PR #187) |
| [U07b](roadmap/archive/U07b.md) | ACP session view: a general TUI we render | both | U07a, U02, U06 | done (PR #245; iPhone check carried to U16) |
| [U07c](roadmap/archive/U07c.md) | Configurable key row | both | U02 | done (PR #162) |
| [U08](roadmap/archive/U08.md) | Decision card: context, options, composer, dictation | both | U05, U06 | done (PR #176; dictation moved to U16) |
| [U09](roadmap/archive/U09.md) | Claude Code T1 view: timeline from the transcript | both | U06 | done (PR #179) |
| [U10](roadmap/archive/U10.md) | Hook offer card and enable button | both | U05 | done (PR #178) |
| [U11](roadmap/U11.md) | Escalation ladder | both | U08 | pending |
| [U12](roadmap/archive/U12.md) | History store, Activity timeline and daily digest | both | U04 | done (PR #243) |
| [U13](roadmap/archive/U13.md) | Usage and cost view | both | U12 | done (PR #244) |
| [U14](roadmap/archive/U14.md) | Server health collection and view | both | U04 | done (PR #215) |
| [U15](roadmap/archive/U15.md) | Desktop workspace: panes and shortcuts | desktop | U05, U08 | done (PR #198) |
| [U16](roadmap/U16.md) | Real-device gate and the free gate | mobile | U04, U05, U07, U07a, U07b, U07c, U08, U09, U10, U11, U12, U13, U15, R36 | pending |

Step header fields, tiers and how to re-enter work are described in `docs/plans/termenbox-roadmap.md`. Legend for the original steps: `‖` marked steps could run in parallel (U03, U14); a step's Done-when is in its file.
