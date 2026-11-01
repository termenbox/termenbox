# Agent view as the main mode: levels, orientation, inspector, rules and the default flip, built template-first

Decisions: 0162 (default view, credential-aware for Claude; draft), 0163 (levels, orientation, stepping), 0164 (strip, inspector, chrome budget), 0165 (permission rules), 0166 (message roles on the type scale), 0167 (template-first build order), 0168 (away digest, stuck signal, queued prompt). Fact 0069 (Anthropic terms), fact 0040 (what each agent sends). Guardrails `auto-answered-permissions-are-scoped-visible-and-audited`, `agent-view-meaning-never-rests-on-colour-alone`, `ui-features-ship-templates-tests-and-media-before-wiring`. Procedures `build-an-agent-view-slice-template-first`, `add-a-message-role-to-the-agent-view`. Roadmap steps R81 (foundation), R82 (reading), R83 (orientation), R84 (inspector, composer), R85 (rules), R86 (default flip). Related, not changed: 0137 (amended by 0162), R75 (ACP window host in tabs), R74 (stable ids; owner gates), R73 (UX harness; scenarios move to it when it lands), R78 and `docs/plans/cross-session-search.md` (scroll-to-turn reuses the turn anchors built here).

## Cold start

Termenbox is a web terminal for remote AI coding agents: Rust server `packages/server`, shared wire types `packages/core/src/protocol.rs` (mirrored by hand in `packages/client/src/lib/protocol.ts`), Svelte 5 client `packages/client/src`. Read `CLAUDE.md` first. Work only in a pool worktree (`sh scripts/pool.sh acquire feat/agent-view-<step>`), one squashed PR per step off `develop`, commit subjects `type(scope): summary` with `Why:`/`What:` body, no Co-Authored-By. No compiler on some machines: prefix commands with `scripts/dev.sh`. Constraints: 512 MB RAM, 1 vCPU, secrets never logged, the web client stays complete.

The feature: the ACP "Agent view" (`AcpView.svelte`, `AcpEntryView.svelte`, store `lib/acpStore.ts`, reducer `lib/acpStream.ts`, server `acp.rs`, `acp_stream.rs`) becomes the main way to run an agent. Today it draws every entry in full. This programme adds four verbosity levels with a Quiet/Detail quick toggle, a sticky current-prompt header and a "N new below" pill, prompt/answer/landmark stepping from a status strip, typed message roles on the existing type scale, a desktop inspector with phone sheets, an away digest, a stuck signal, a queued next prompt, owner-set permission rules with presets, suggestions and a dry run, and a credential-aware default so Agent view is the default except for a Claude subscription login.

**Build order (decision 0167, read the guardrail).** Every slice runs: templates on fixtures (no store, wire or server) -> tests -> styling -> a preview video of the templates on the shared recorder -> owner review -> wiring to the real stream -> the same case on live data, whose video is picked for the docs. The recorder, review and pick pipeline already exists on `develop` (decisions 0158 to 0161); this programme adds cases and a probe page to it, never a second pipeline. Never start a wiring step before its slice's review step is `[x]`.

Design rules every UI step obeys (`docs/plans/assets/ux/DESIGN-PRINCIPLES.md`): DR1/DR2 44 pt targets and 8 pt gaps, DR4 at most 7 controls per row, DR5 sizes 12/13/15/17/22 (+13 mono), inputs 16, DR7 labels, DR8 contrast 4.5:1 in every skin, DR9 state never by colour alone, DR12 chrome at most 20% of height, DR14 motion 150 to 250 ms and none under reduced motion, DR15 no horizontal scroll. The audit is `docs/plans/assets/ux/audit.mjs`.

Success targets (checked in the slice scenarios, in place of telemetry): the current tool is visible without scrolling in Focus on a 390 px phone for the long fixture; the previous prompt is reachable in 2 taps from anywhere; switching Quiet/Detail is 1 tap and keeps the place; an open permission is visible at every level; chrome is at most 20% of the phone height.

Shared files: fixtures `packages/client/src/lib/agentViewFixtures.ts`; probe page `packages/client/e2e/agentview.html`, `agentview.ts`, `AgentViewProbe.svelte`, runner `e2e/agentview.mjs`; cases in `packages/client/e2e/video/cases.mjs` (groups `agent-template` for review-only previews, `agent` for the live ones); media in `docs/media/` through the pick path; reference subsection in `docs/guide/workspaces.md`.

## Slice 0: foundation (R81)

### 0.1 Baseline: inventory, screenshots and chrome share — [ ]‖
Class: explore · Needs: none · Check: `test -f docs/plans/assets/agent-view/baseline.md`
Context: `packages/client/src/AcpView.svelte` (288 lines), `AcpEntryView.svelte` (106), `AcpPrompt.svelte` (voice, prompt history), `DecisionPanel.svelte`, `lib/acpStream.ts` (reducer, `Entry` kinds, `usage`, `planProgress`), `lib/acpStore.ts` (streams, saved draft and place), the scripted fake agent `packages/client/e2e/fake-acp-agent.mjs`, scenarios `e2e/acp-views.mjs`. Decisions 0163 and 0164 need exact numbers before anything is designed.
Do: Write `docs/plans/assets/agent-view/baseline.md`: every `Entry` kind and how it renders; where scroll place (`following`, `settle`) and search are handled; breakpoints; the bindings already used on keyboard and touch; the chrome share (tab bar, header, composer) of a 390 x 844 phone with the keyboard closed and open, measured, against DR12. Take one phone and one desktop screenshot of a 40-entry run with the existing e2e harness.
Done-when: the file exists, names each `Entry` kind in `acpStream.ts`, states the measured phone chrome percentage, and links the two screenshots.

### 0.2 Measure: what each agent sends, and which credential Claude's adapter uses — [ ]‖
Class: explore · Needs: none · Check: `test -f docs/plans/assets/agent-view/agents.md`
Context: Fact 0040 shows agents report tool kinds and plans unevenly (todos as `other` tool calls on kilo, `plan` rare) and fact 0069 the terms. Decisions 0163 (unknown kinds stay visible) and 0162 (`mixed` credential) need the facts. Use a throwaway `HOME` and `scripts/acp-probe.mjs`; never the owner's sessions.
Do: Write `docs/plans/assets/agent-view/agents.md`: per built-in agent (claude, opencode, codex, kilo and the others in fact 0040) the tool kinds it sends for read, search, edit, run, todo, and whether `plan` and `usage` arrive. Measure, with a scratch `HOME`, which credential `claude-agent-acp` uses when `ANTHROPIC_API_KEY` is set together with a login file; if a live login is not available, read the adapter source and say "unmeasured" in the file. Extend fact 0040 with anything new.
Done-when: the file lists kinds, `plan` and `usage` per agent and states the credential precedence as measured or unmeasured.

### 0.3 Fixture streams — [ ]
Class: implement · Needs: 0.1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/lib/agentViewFixtures.test.ts'`
Context: Decision 0167: fixtures are the contract shared by tests, the probe page and the cases. Entry shapes are in `lib/acpStream.ts`.
Do: Add `packages/client/src/lib/agentViewFixtures.ts` with named streams: `long-run` (40 entries, many tool calls), `error-turn`, `open-permission`, `plan-run`, `todo-as-tool` (kilo style), `unknown-kinds`, `stress` (2,000 entries), `epoch-change`, `away` (entries since a saved place) and `queue` (a running turn with queued prompts). Add a test that every fixture is a valid stream and that `stress` has 2,000 entries.
Done-when: the test passes and every named stream above exists.

### 0.4 Pure libraries: turns, levels, kinds, navigation — [ ]
Class: implement · Needs: 0.3, 0.2 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/lib/acpTurns.test.ts src/lib/acpLevels.test.ts src/lib/acpKinds.test.ts src/lib/acpNav.test.ts'`
Context: Decisions 0163 and 0166. Entry ids restart each epoch (see `Place` in `lib/acpStore.ts`), so anchors carry the epoch.
Do: Add `lib/acpTurns.ts` (turns with anchor `{epoch, entry}`, status, counts by kind, files touched, final answer, landmarks), `lib/acpKinds.ts` (classify a tool entry through the agent's profile from step 0.2; unknown or `other` is never "read-only"), `lib/acpLevels.ts` (`rows(entries, turns, level, peeked, hit)`: Full, Compact folds, Focus, Outline; an open permission, error or stopped state is never hidden) and `lib/acpNav.ts` (`next(turns, from, kind, dir)`, the keyboard map ignoring the composer). Test each with the step 0.3 fixtures, including `unknown-kinds`, `open-permission` at Outline and `epoch-change`; time `rows()` on `stress` (target under 20 ms).
Done-when: the four test files pass, and they include "open permission visible at Outline", "unknown kind never folded as read-only" and the `stress` timing.

### 0.5 Agent view cases on the shared recorder, and the probe page — [ ]
Class: implement · Needs: 0.3 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --list --json | node -e "const l=JSON.parse(require(\"fs\").readFileSync(0,\"utf8\"));if(!l.some(c=>c.id===\"reading-levels-template\"&&c.group===\"agent-template\"))process.exit(1)" && node e2e/video/render.mjs --case reading-levels-template --profile phone --out /tmp/av && test -s /tmp/av/reading-levels-template-phone.mp4'`
Context: Decision 0167. The documentation media pipeline is already on `develop` (decisions 0158 to 0161, `docs/plans/docs-media-and-checks.md`, `docs/plans/user-course.md`): one recorder in `packages/client/e2e/video/` (`lib.mjs`, `rhythm.mjs`, `steps.mjs`, `cases.mjs`, `render.mjs`), the dispatch-only `docs-media.yaml` and `docs-media-review.yaml`, `docs/media/`, `scripts/docs-media.mjs` (markers and check), `scripts/docs-course.mjs` (course steps from cases). Do not build another. Read `cases.mjs` (the `talk-to-agent` case is a complete example), `steps.mjs` and `render.mjs` first.
Do: Add the probe page (`e2e/agentview.html`, `agentview.ts`, `AgentViewProbe.svelte`, runner `e2e/agentview.mjs`) showing a fixture selected by `?fixture=` and a source by `?source=fixture|live`, and give `render.mjs` the smallest start option so a case can open it. Give `e2e/agentview.mjs` two modes: `--smoke` (load every template on the probe page, fail on any console error) and `--audit` (run `docs/plans/assets/ux/audit.mjs` on phone and desktop in every skin, failing on DR1, DR2, DR4, DR8, DR12 or DR15). Add the first case `reading-levels-template` (group `agent-template`, page `docs/guide/workspaces.md`) over the existing `AcpEntryView` rendering, with `steps` declared once and a final assertion that the last entry shows.
Done-when: the Check exits 0, `--list --json` shows the case with its steps, and a render that skips a declared step fails.

### 1.1 Templates: level toggle, folds, turn header, sticky prompt, status strip, new-below pill — [ ]
Class: implement · Needs: 0.4, 0.5 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm check && node e2e/agentview.mjs --smoke'`
Context: Decisions 0163 (Quiet/Detail toggle plus four-level menu, folds with count words and glyphs, sticky current-prompt header, "N new below" pill) and 0164 (strip row of at most seven controls: state, current tool with elapsed time, plan progress when present, usage when present, turn counter, Stop). Templates take props only, no store, no wire. Build them unstyled: structure, labels, 44 pt targets.
Do: Add `AcpLevelControl.svelte`, `AcpFold.svelte`, `AcpTurnHeader.svelte`, `AcpStickyPrompt.svelte`, `AcpStatusStrip.svelte`, `AcpNewBelow.svelte` in `packages/client/src/`, and show each at all four levels and both viewports on the probe page from the step 0.3 fixtures (long-run, plan-run, todo-as-tool, unknown-kinds, error-turn).
Done-when: `pnpm check` is clean and `agentview.mjs --smoke` loads every template on the probe page with no console error.

### 1.2 Tests for the reading templates — [ ]
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm vitest run && node e2e/agentview.mjs --audit'`
Context: Guardrail `agent-view-meaning-never-rests-on-colour-alone`; `docs/plans/assets/ux/audit.mjs` measures DR1, DR2, DR4, DR8, DR12 and DR15.
Do: Component tests for each template (accessible names, a glyph and word per state, strip hides plan progress when absent, fold count words, Quiet/Detail toggle in one tap). Probe e2e on phone and desktop: all levels, the strip at most seven controls, chrome share at most 20%, no overflow. Wire the audit into `agentview.mjs --audit`.
Done-when: all pass on the unstyled templates, and a strip with an eighth control makes the audit fail.

### 1.3 Styling: roles, tokens, type scale — [ ]
Class: implement · Needs: 1.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm vitest run src/lib/acpRoles.test.ts && node e2e/agentview.mjs --audit'`
Context: Decision 0166 and procedure `add-a-message-role-to-the-agent-view`. Skins are `packages/client/src/skins.css`; sizes only from DR5.
Do: Add `lib/acpRoles.ts` (role table: token, glyph, weight, size step, screen-reader word) and the role tokens in every skin; apply them in the templates and in `AcpEntryView.svelte`'s render path used by the probe; add the bounded text-size control scaling the whole scale; motion per DR14. Test contrast of every role in every skin (4.5:1), glyph and word present, and that only DR5 sizes occur.
Done-when: `acpRoles.test.ts` passes for all skins, the audit passes with styling on, and `grep -n STATUS_COLOR packages/client/src/AcpEntryView.svelte` prints nothing.

### 1.4 Template cases and preview video for reading — [ ]
Class: implement · Needs: 1.3 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && for c in reading-levels-template status-strip-template; do for p in phone desktop; do node e2e/video/render.mjs --case $c --profile $p --out /tmp/av || exit 1; test -s /tmp/av/$c-$p.mp4 || exit 1; done; done'`
Context: Decision 0167; decisions 0158, 0159, 0160 and 0161 on `develop` (one recorder, review and pick, steps declared once); procedures `render-a-feature-video` and `review-and-publish-docs-media`. A case in `packages/client/e2e/video/cases.mjs` declares `title`, `group`, `page`, `server`, `inputs` and `steps` (id, caption, label) once; `h.step(id)` performs them in order, `render.mjs` fails when one is skipped, and the case ends with an assertion of its final state (`ux-scenarios-assert-a-state-before-they-capture-it`). `inputs` lists this slice's component files so the staleness check flags the video when they change. Template cases start the probe page (built by `pnpm run e2e:build`), not the app: read how `render.mjs` starts a case (`server: 'scripted'|'real'`, `storage`) and add the smallest start option for the probe page.
Do: Add the cases `reading-levels-template`, `status-strip-template` (group `agent-template`, page `docs/guide/workspaces.md`, `server: 'scripted'`): `reading-levels-template` switches Quiet/Detail and each level on the long run, peeks a fold and shows the sticky prompt and the new-below pill; `status-strip-template` shows the strip through idle, a running tool, waiting on you and failed. Plain-word captions, no agent text, scripted data only. Render phone and desktop.
Done-when: every case renders on both profiles with its MP4 and stills, and a case that skips a declared step fails with no video.

### 1.5 Owner review: reading templates — [ ]
Class: review · Needs: 1.4 · Check: `grep -c "slice 1.*accepted" docs/plans/assets/agent-view/reviews.md`
Context: Guardrail `ui-features-ship-templates-tests-and-media-before-wiring`; procedure `review-and-publish-docs-media`. The template cases are on `develop` (steps up to 1.4 merged), so the render workflow can run them. Only the owner accepts.
Do: Dispatch `gh workflow run docs-media.yaml --ref develop -f case=<one of reading-levels-template, status-strip-template> -f profile=both` for each case, give the owner the issue and the preview link, and record their notes or acceptance in `docs/plans/assets/agent-view/reviews.md` with the date. Apply notes with `/regen` knobs or by editing the template and the case; re-run the tests and the preview until accepted. Do not tick template videos for the docs.
Done-when: `reviews.md` has an "accepted" line for slice 1 with a date.

### 1.6 Wire levels, persistence and place — [ ]
Class: implement · Needs: 1.5 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/AcpView.test.ts src/lib/acpStore.test.ts'`
Context: `Saved` in `lib/acpStore.ts` keeps draft, place and opened diffs per workspace key in guarded `localStorage`; scroll anchoring in `AcpView.svelte` (`following`, `settle`, `placeOf`) is delicate.
Do: Add `level` to `Saved` (per-tab override) and a device default (same guarded helpers); render `AcpView` through `rows()` and the templates; keep the top turn across a level switch; search unfolds a fold holding the hit; the Auto setting (opt-in) changes level only when the user is away from the tab and a question or failure opens.
Done-when: tests include "switching level keeps the top turn", "level survives a reload and is per tab", "search unfolds a fold" and "auto never changes the level while the user scrolls".

### 1.7 Wire the strip, sticky prompt and new-below pill — [ ]
Class: implement · Needs: 1.6 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/AcpView.test.ts'`
Context: Stream state and `usage`/`contextPercent` are in `lib/acpStream.ts`; the strip collapses to one line on scroll down (decision 0164).
Do: Mount the strip with live state, current tool and elapsed time, plan progress (only when a plan exists, including the kilo todo-as-tool mapping from step 0.2), usage and Stop; show the sticky prompt of the turn being read and the pill when `following` is false and new entries arrive.
Done-when: tests cover idle, running, waiting on you, failed, ended, no plan, no usage, and the pill appearing and clearing.

### 1.8 Live scenario for reading — [ ]
Class: implement · Needs: 1.7 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/acp-views.mjs && cd ../.. && node scripts/docs-media.mjs check docs/guide/workspaces.md'`
Context: Decision 0167: the same cases run on live data, and the live video, not the template one, is picked for the docs (procedure `review-and-publish-docs-media`). Guide reference is `docs/guide/workspaces.md`; the docs checks are decision 0159. The scripted fake agent is `packages/client/e2e/fake-acp-agent.mjs`.
Do: Extend the fake agent with what the cases need (a long run, an error, a permission, a plan, a queue). Add the live cases `reading-levels`, `status-strip` (group `agent`, page `docs/guide/workspaces.md`, the same `steps` as the template cases, `server: 'real'` when the scripted server cannot speak ACP, as the `agent-view` case in `docs/plans/docs-media-and-checks.md` step 2.3 decides), run them with `?source=live`, and check the success targets in this plan's header that apply. Add or refresh the reading subsection of `docs/guide/workspaces.md` (hand-written, plain words) with a `<!-- media:<id> -->` marker per case. After the merge dispatch `docs-media.yaml` for these cases and have a developer pick them; the step stays `[~]` until the picked media is on `develop`.
Done-when: the e2e passes, the live cases render on both profiles, and `node scripts/docs-media.mjs check docs/guide/workspaces.md` passes (a marker may warn until the media is picked).

### 1.9 Performance budget and windowed rendering — [ ]‖
Class: implement · Needs: 1.6 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/lib/acpLevels.test.ts src/AcpView.test.ts'`
Context: The `stress` fixture has 2,000 entries; the box and phones are small. `AcpView` renders every row today.
Do: Measure level switch and append cost on `stress`; render only rows near the viewport (with a spacer) if the measure exceeds 100 ms for a switch or 16 ms for an append, keeping scroll anchoring and search intact; otherwise record the numbers and leave rendering as is.
Done-when: the numbers are in the test output, the budgets pass, and place restore and search still pass on `stress`.

## Slice 2: orientation (R83)

### 2.1 Templates: stepping controls, minimap, digest, stuck notice, hint — [ ]
Class: implement · Needs: 1.5 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm check && node e2e/agentview.mjs --smoke'`
Context: Decisions 0163 (stepping from the strip: previous/next with the turn counter, "next error"/"next ask" shortcuts only while one is ahead, a one-time hint) and 0168 (digest card, stuck notice). No floating overlay.
Do: Add `AcpTurnStepper.svelte` (inside the strip), `AcpMinimap.svelte` (sheet listing turns with status marks), `AcpDigest.svelte`, `AcpStuckNotice.svelte`, `AcpHint.svelte`, shown on the probe page from the `away`, `error-turn` and `long-run` fixtures.
Done-when: `pnpm check` is clean and the smoke loads each template with no console error.

### 2.2 Tests for the orientation templates — [ ]
Class: implement · Needs: 2.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm vitest run && node e2e/agentview.mjs --audit'`
Context: Same rules as step 1.2.
Do: Component tests (names, 44 pt, the shortcuts appear only when a target exists, the digest is absent when nothing happened, the hint is dismissible); probe e2e for stepping across kinds, the minimap and reduced motion; audit on both viewports.
Done-when: all pass on the unstyled templates.

### 2.3 Styling for the orientation templates — [ ]
Class: implement · Needs: 2.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm vitest run src/lib/acpRoles.test.ts && node e2e/agentview.mjs --audit'`
Context: Decision 0166: the digest and rule roles are in the role table.
Do: Add the digest, stuck and highlight roles and style the templates; the jump highlight is 150 to 250 ms and absent under reduced motion.
Done-when: roles tests and the audit pass in every skin.

### 2.4 Template cases and preview video for orientation — [ ]
Class: implement · Needs: 2.3 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && for c in stepping-template away-digest-template stuck-template; do for p in phone desktop; do node e2e/video/render.mjs --case $c --profile $p --out /tmp/av || exit 1; test -s /tmp/av/$c-$p.mp4 || exit 1; done; done'`
Context: Decision 0167; decisions 0158, 0159, 0160 and 0161 on `develop` (one recorder, review and pick, steps declared once); procedures `render-a-feature-video` and `review-and-publish-docs-media`. A case in `packages/client/e2e/video/cases.mjs` declares `title`, `group`, `page`, `server`, `inputs` and `steps` (id, caption, label) once; `h.step(id)` performs them in order, `render.mjs` fails when one is skipped, and the case ends with an assertion of its final state (`ux-scenarios-assert-a-state-before-they-capture-it`). `inputs` lists this slice's component files so the staleness check flags the video when they change. Template cases start the probe page (built by `pnpm run e2e:build`), not the app: read how `render.mjs` starts a case (`server: 'scripted'|'real'`, `storage`) and add the smallest start option for the probe page.
Do: Add the cases `stepping-template`, `away-digest-template`, `stuck-template` (group `agent-template`, page `docs/guide/workspaces.md`, `server: 'scripted'`): `stepping-template` reaches the previous prompt in two taps from the end, uses next error and opens the minimap; `away-digest-template` returns to a tab with the `away` fixture; `stuck-template` shows the stuck notice appearing and clearing. Plain-word captions, no agent text, scripted data only. Render phone and desktop.
Done-when: every case renders on both profiles with its MP4 and stills, and a case that skips a declared step fails with no video.

### 2.5 Owner review: orientation templates — [ ]
Class: review · Needs: 2.4 · Check: `grep -c "slice 2.*accepted" docs/plans/assets/agent-view/reviews.md`
Context: Guardrail `ui-features-ship-templates-tests-and-media-before-wiring`; procedure `review-and-publish-docs-media`. The template cases are on `develop` (steps up to 2.4 merged), so the render workflow can run them. Only the owner accepts.
Do: Dispatch `gh workflow run docs-media.yaml --ref develop -f case=<one of stepping-template, away-digest-template, stuck-template> -f profile=both` for each case, give the owner the issue and the preview link, and record their notes or acceptance in `docs/plans/assets/agent-view/reviews.md` with the date. Apply notes with `/regen` knobs or by editing the template and the case; re-run the tests and the preview until accepted. Do not tick template videos for the docs.
Done-when: `reviews.md` has an "accepted" line for slice 2 with a date.

### 2.6 Wire stepping, minimap and anchors — [ ]
Class: implement · Needs: 2.5, 1.6 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/AcpView.test.ts src/lib/acpNav.test.ts'`
Context: `lib/acpNav.ts` from step 0.4; R78's scroll-to-turn must reuse these anchors (`docs/plans/cross-session-search.md`).
Do: Connect the stepper, keys (`J`/`K`, `[`/`]`, inactive in the composer) and minimap to the live turns; a jump scrolls the turn header to the top, highlights it and unfolds a fold holding it; export the anchor type for R78.
Done-when: tests cover stepping per kind, a jump into a fold, the epoch change and keys ignored in the composer.

### 2.7 Wire the digest and the stuck signal — [ ]
Class: implement · Needs: 2.5, 1.6 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/AcpView.test.ts'`
Context: Decision 0168: the digest comes from the stream since the saved place and uses the turn summaries; the stuck threshold defaults to 5 minutes, per device.
Do: Compute and show the digest on return, dismissible; show the stuck notice when running with no activity beyond the threshold; the first-run hint.
Done-when: tests cover a digest with a finished turn, a rule-answered ask, an error, no digest when nothing happened, and a stuck notice appearing and clearing.

### 2.8 Live scenario for orientation — [ ]
Class: implement · Needs: 2.6, 2.7 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/acp-views.mjs && cd ../.. && node scripts/docs-media.mjs check docs/guide/workspaces.md'`
Context: Decision 0167: the same cases run on live data, and the live video, not the template one, is picked for the docs (procedure `review-and-publish-docs-media`). Guide reference is `docs/guide/workspaces.md`; the docs checks are decision 0159. The scripted fake agent is `packages/client/e2e/fake-acp-agent.mjs`.
Do: Extend the fake agent with what the cases need (a long run, an error, a permission, a plan, a queue). Add the live cases `stepping`, `away-digest`, `stuck` (group `agent`, page `docs/guide/workspaces.md`, the same `steps` as the template cases, `server: 'real'` when the scripted server cannot speak ACP, as the `agent-view` case in `docs/plans/docs-media-and-checks.md` step 2.3 decides), run them with `?source=live`, and check the success targets in this plan's header that apply. Add or refresh the orientation subsection of `docs/guide/workspaces.md` (hand-written, plain words) with a `<!-- media:<id> -->` marker per case. After the merge dispatch `docs-media.yaml` for these cases and have a developer pick them; the step stays `[~]` until the picked media is on `develop`.
Done-when: the e2e passes, the live cases render on both profiles, and `node scripts/docs-media.mjs check docs/guide/workspaces.md` passes (a marker may warn until the media is picked).

### 3.1 Templates: inspector, phone sheets, queue chip, auto setting — [ ]
Class: implement · Needs: 1.5 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm check && node e2e/agentview.mjs --smoke'`
Context: Decisions 0164 (inspector tabs Outline, Plan, Changes, Rules; main column about 80 characters; sheets on phone) and 0168 (queued prompt chip). `AcpDiffView.svelte` exists for diffs.
Do: Add `AcpInspector.svelte` (tabs; Plan hidden without a plan; Rules hidden until R85), phone sheets opened from the strip, `AcpChanges.svelte` (all diffs grouped by file), `AcpQueueChip.svelte` (edit and remove), and the Auto and text-size settings; show them on the probe page with the `long-run`, `plan-run` and `queue` fixtures.
Done-when: `pnpm check` is clean and the smoke loads each template.

### 3.2 Tests for the inspector and composer templates — [ ]
Class: implement · Needs: 3.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm vitest run && node e2e/agentview.mjs --audit'`
Context: Same rules as step 1.2.
Do: Component tests (tabs, hidden tabs, Changes grouping, queue chip limits of five and edit/remove) and probe e2e at wide and narrow viewports; audit.
Done-when: all pass on the unstyled templates.

### 3.3 Styling for the inspector and composer templates — [ ]
Class: implement · Needs: 3.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/agentview.mjs --audit'`
Context: Decision 0166.
Do: Style with roles and tokens; check the main column measure and the chrome budget with the composer and chip showing.
Done-when: the audit passes in every skin on both viewports.

### 3.4 Template cases and preview video for the inspector and queue — [ ]
Class: implement · Needs: 3.3 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && for c in inspector-template queue-template; do for p in phone desktop; do node e2e/video/render.mjs --case $c --profile $p --out /tmp/av || exit 1; test -s /tmp/av/$c-$p.mp4 || exit 1; done; done'`
Context: Decision 0167; decisions 0158, 0159, 0160 and 0161 on `develop` (one recorder, review and pick, steps declared once); procedures `render-a-feature-video` and `review-and-publish-docs-media`. A case in `packages/client/e2e/video/cases.mjs` declares `title`, `group`, `page`, `server`, `inputs` and `steps` (id, caption, label) once; `h.step(id)` performs them in order, `render.mjs` fails when one is skipped, and the case ends with an assertion of its final state (`ux-scenarios-assert-a-state-before-they-capture-it`). `inputs` lists this slice's component files so the staleness check flags the video when they change. Template cases start the probe page (built by `pnpm run e2e:build`), not the app: read how `render.mjs` starts a case (`server: 'scripted'|'real'`, `storage`) and add the smallest start option for the probe page.
Do: Add the cases `inspector-template`, `queue-template` (group `agent-template`, page `docs/guide/workspaces.md`, `server: 'scripted'`): `inspector-template` opens Outline, Plan and Changes on desktop and the same as sheets on phone; `queue-template` queues, edits and removes a prompt while a turn runs. Plain-word captions, no agent text, scripted data only. Render phone and desktop.
Done-when: every case renders on both profiles with its MP4 and stills, and a case that skips a declared step fails with no video.

### 3.5 Owner review: inspector and composer templates — [ ]
Class: review · Needs: 3.4 · Check: `grep -c "slice 3.*accepted" docs/plans/assets/agent-view/reviews.md`
Context: Guardrail `ui-features-ship-templates-tests-and-media-before-wiring`; procedure `review-and-publish-docs-media`. The template cases are on `develop` (steps up to 3.4 merged), so the render workflow can run them. Only the owner accepts.
Do: Dispatch `gh workflow run docs-media.yaml --ref develop -f case=<one of inspector-template, queue-template> -f profile=both` for each case, give the owner the issue and the preview link, and record their notes or acceptance in `docs/plans/assets/agent-view/reviews.md` with the date. Apply notes with `/regen` knobs or by editing the template and the case; re-run the tests and the preview until accepted. Do not tick template videos for the docs.
Done-when: `reviews.md` has an "accepted" line for slice 3 with a date.

### 3.6 Wire the inspector and sheets — [ ]
Class: implement · Needs: 3.5, 1.7 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/AcpView.test.ts'`
Context: Decision 0164; the inspector state is kept per device.
Do: Mount the inspector at the wide breakpoint and the sheets from the strip, fed by the live turns, plan and diffs.
Done-when: tests cover wide and narrow, Plan hidden without a plan, and Changes with two diffs grouped by file.

### 3.7 Server: the prompt queue — [ ]
Class: implement · Needs: 3.5 · Check: `scripts/dev.sh cargo test -p termenbox-server acp_queue`
Context: Decision 0168. Turn handling is in `packages/server/src/acp.rs` and `acp_stream.rs`; wire types in `packages/core/src/protocol.rs` mirrored in `packages/client/src/lib/protocol.ts`; a fault in one session never stops the server (decision 0143).
Do: Add a per-session queue (at most five) with wire messages to add, edit and remove, an optional queue field on the stream state, sending the next prompt when a turn ends, pausing on a failed or cancelled turn, and dropping the queue with a visible notice on a server restart.
Done-when: tests cover send-on-end, the cap of five, edit and remove before sending, pause after a failure, drop on restart with a notice, and an old client ignoring the new field.

### 3.8 Wire the queue chip and the composer — [ ]
Class: implement · Needs: 3.6, 3.7 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/AcpPrompt.test.ts src/AcpView.test.ts'`
Context: `AcpPrompt.svelte` has voice and prompt history; the composer accepts text while a turn runs.
Do: Send to the queue while a turn runs, show the chip, edit and remove; keep voice and history working.
Done-when: tests cover queueing while running, edit, remove, a paused queue after a failure, and voice and history unchanged.

### 3.9 Live scenario for the inspector and queue — [ ]
Class: implement · Needs: 3.8 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/acp-views.mjs && cd ../.. && node scripts/docs-media.mjs check docs/guide/workspaces.md'`
Context: Decision 0167: the same cases run on live data, and the live video, not the template one, is picked for the docs (procedure `review-and-publish-docs-media`). Guide reference is `docs/guide/workspaces.md`; the docs checks are decision 0159. The scripted fake agent is `packages/client/e2e/fake-acp-agent.mjs`.
Do: Extend the fake agent with what the cases need (a long run, an error, a permission, a plan, a queue). Add the live cases `inspector`, `queue-a-prompt` (group `agent`, page `docs/guide/workspaces.md`, the same `steps` as the template cases, `server: 'real'` when the scripted server cannot speak ACP, as the `agent-view` case in `docs/plans/docs-media-and-checks.md` step 2.3 decides), run them with `?source=live`, and check the success targets in this plan's header that apply. Add or refresh the the inspector and queue subsection of `docs/guide/workspaces.md` (hand-written, plain words) with a `<!-- media:<id> -->` marker per case. After the merge dispatch `docs-media.yaml` for these cases and have a developer pick them; the step stays `[~]` until the picked media is on `develop`.
Done-when: the e2e passes, the live cases render on both profiles, and `node scripts/docs-media.mjs check docs/guide/workspaces.md` passes (a marker may warn until the media is picked).

### 4.1 Templates: risk line, always-allow confirm, rules panel, suggestion, presets, batch — [ ]
Class: implement · Needs: 1.5 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm check && node e2e/agentview.mjs --smoke'`
Context: Decision 0165 and `DecisionPanel.svelte`/`AcpEntryView.svelte` cards. The rule wire shapes (list, add, revoke, preview) are written as fixtures here and become the contract for step 4.6.
Do: Add `AcpRiskLine.svelte`, `AcpAlwaysAllow.svelte` (scope confirmation with presets and expiry), `AcpRulesPanel.svelte`, `AcpRuleSuggestion.svelte` (with the dry-run preview list), `AcpModeChips.svelte` (agent session modes), the marked "allowed by rule" line with Revoke, and a batch row for the inbox; show them on the probe page from the `open-permission` fixture and a new `rules` fixture.
Done-when: `pnpm check` is clean and the smoke loads each template.

### 4.2 Tests for the rules templates — [ ]
Class: implement · Needs: 4.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm vitest run && node e2e/agentview.mjs --audit'`
Context: Guardrail `auto-answered-permissions-are-scoped-visible-and-audited`.
Do: Component tests (the confirmation shows workspace, kind, pattern and expiry before creating; the suggestion appears only after three approvals and never creates without confirmation; the marked line offers Revoke; the risk line for edit, delete, outside-workspace and fetch; batch never mixes kinds or workspaces silently); audit.
Done-when: all pass on the unstyled templates.

### 4.3 Styling for the rules templates — [ ]
Class: implement · Needs: 4.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/agentview.mjs --audit'`
Context: Decision 0166: the rule line and risk warning roles.
Do: Style with roles and tokens; the warning mark carries a glyph and a word.
Done-when: the audit passes in every skin.

### 4.4 Template cases and preview video for rules — [ ]
Class: implement · Needs: 4.3 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && for c in decide-faster-template rule-suggestion-template; do for p in phone desktop; do node e2e/video/render.mjs --case $c --profile $p --out /tmp/av || exit 1; test -s /tmp/av/$c-$p.mp4 || exit 1; done; done'`
Context: Decision 0167; decisions 0158, 0159, 0160 and 0161 on `develop` (one recorder, review and pick, steps declared once); procedures `render-a-feature-video` and `review-and-publish-docs-media`. A case in `packages/client/e2e/video/cases.mjs` declares `title`, `group`, `page`, `server`, `inputs` and `steps` (id, caption, label) once; `h.step(id)` performs them in order, `render.mjs` fails when one is skipped, and the case ends with an assertion of its final state (`ux-scenarios-assert-a-state-before-they-capture-it`). `inputs` lists this slice's component files so the staleness check flags the video when they change. Template cases start the probe page (built by `pnpm run e2e:build`), not the app: read how `render.mjs` starts a case (`server: 'scripted'|'real'`, `storage`) and add the smallest start option for the probe page.
Do: Add the cases `decide-faster-template`, `rule-suggestion-template` (group `agent-template`, page `docs/guide/workspaces.md`, `server: 'scripted'`): `decide-faster-template` shows the risk line, a mode chip, a preset rule, the marked line and Revoke; `rule-suggestion-template` approves three times, shows the preview and confirms. Plain-word captions, no agent text, scripted data only. Render phone and desktop.
Done-when: every case renders on both profiles with its MP4 and stills, and a case that skips a declared step fails with no video.

### 4.5 Owner review: rules templates — [ ]
Class: review · Needs: 4.4 · Check: `grep -c "slice 4.*accepted" docs/plans/assets/agent-view/reviews.md`
Context: Guardrail `ui-features-ship-templates-tests-and-media-before-wiring`; procedure `review-and-publish-docs-media`. The template cases are on `develop` (steps up to 4.4 merged), so the render workflow can run them. Only the owner accepts; review the wording of the confirmation too, because it carries the safety promise.
Do: Dispatch `gh workflow run docs-media.yaml --ref develop -f case=<one of decide-faster-template, rule-suggestion-template> -f profile=both` for each case, give the owner the issue and the preview link, and record their notes or acceptance in `docs/plans/assets/agent-view/reviews.md` with the date. Apply notes with `/regen` knobs or by editing the template and the case; re-run the tests and the preview until accepted. Do not tick template videos for the docs.
Done-when: `reviews.md` has an "accepted" line for slice 4 with a date.

### 4.6 Server: rule store, argument matcher, audit and wire — [ ]
Class: implement · Needs: 4.5 · Check: `scripts/dev.sh cargo test -p termenbox-server acp_rules`
Context: Decision 0165 and the guardrail. Permission requests are handled in `packages/server/src/acp.rs` and `acp_stream.rs`; state files live under `packages/server/src/registry/`; audit rules in `audit-logging-no-secrets`.
Do: Add an `acp_rules` module: rule type, an atomic JSON store, a global off switch, and a matcher on parsed arguments that sends any compound or indirect command (`&&`, `||`, `;`, pipe, redirect, substitution, `eval`, unparsable) to the user. In the permission path a match answers with the agent's allow option and marks the entry "answered by rule"; no match, a store error or a destructive kind without an exact rule asks the user. Write an audit entry (kind, rule id, outcome). Add wire messages to list, add, revoke and preview (matches among the last 50 asks), mirrored in `protocol.ts`, plus the optional "by rule" field.
Done-when: tests prove a match auto-answers, no match asks, `git status && rm -rf x` never matches a `git status` rule, delete and outside-workspace never auto-allow from a broad rule, a store error asks, preview counts without creating, and the audit line holds no output or secret.

### 4.7 Wire the rules UI, risk summary and suggestions — [ ]
Class: implement · Needs: 4.6, 3.6 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/AcpRules.test.ts src/AcpEntryView.test.ts'`
Context: Decision 0165. Hide every control when the server lacks the rule messages.
Do: Connect the risk line (derived from locations, diffs and kind), always-allow with presets, the Rules tab and sheet, the suggestion with the server preview, mode chips from `info.modes`, the marked line with Revoke and the global off, and the inbox batch row.
Done-when: tests cover create with confirmation, revoke, global off, the suggestion preview, hidden controls on an old server, and the marked line.

### 4.8 Live scenario for rules — [ ]
Class: implement · Needs: 4.7 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/acp-views.mjs && cd ../.. && node scripts/docs-media.mjs check docs/guide/workspaces.md'`
Context: Decision 0167: the same cases run on live data, and the live video, not the template one, is picked for the docs (procedure `review-and-publish-docs-media`). Guide reference is `docs/guide/workspaces.md`; the docs checks are decision 0159. The scripted fake agent is `packages/client/e2e/fake-acp-agent.mjs`.
Do: Extend the fake agent with what the cases need (a long run, an error, a permission, a plan, a queue). Add the live cases `decide-faster`, `rule-suggestion` (group `agent`, page `docs/guide/workspaces.md`, the same `steps` as the template cases, `server: 'real'` when the scripted server cannot speak ACP, as the `agent-view` case in `docs/plans/docs-media-and-checks.md` step 2.3 decides), run them with `?source=live`, and check the success targets in this plan's header that apply. Add or refresh the rules subsection of `docs/guide/workspaces.md` (hand-written, plain words) with a `<!-- media:<id> -->` marker per case. After the merge dispatch `docs-media.yaml` for these cases and have a developer pick them; the step stays `[~]` until the picked media is on `develop`.
Done-when: the e2e passes, the live cases render on both profiles, and `node scripts/docs-media.mjs check docs/guide/workspaces.md` passes (a marker may warn until the media is picked).

### 5.1 Server: credential detection — [ ]‖
Class: implement · Needs: 0.2 · Check: `scripts/dev.sh cargo test -p termenbox-server acp_credential`
Context: Decision 0162. The hello carries `acp_agents: Vec<String>` (`packages/core/src/protocol.rs`, mirrored in `packages/client/src/lib/protocol.ts`); old clients must keep working. Fact 0069.
Do: Detect `api`, `cloud`, `subscription`, `mixed` or `none` per agent from environment variable names and from the existence, never the content, of the login file, using the precedence found in step 0.2; send it as an optional parallel field in the hello. Never read, log or send a credential value.
Done-when: tests cover each of the five results with a scratch `HOME` and environment, and a test greps the log output of a detection run for the key value and finds nothing.

### 5.2 Template and tests: New Tab default and its reason — [ ]
Class: implement · Needs: 0.2, 0.5 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/NewTabSheet.test.ts src/NewWorkspaceSheet.test.ts'`
Context: Decisions 0137 and 0162; the sheets remember the last view per server and agent today. Pure choice logic first, on props.
Do: Add the pure function that picks the preselected view from the agent, the credential kind, the flag and the remembered choice, and a one-line reason for Terminal on Claude with `subscription`, `mixed` or `none`; show both states in the sheets with props only; add the case `new-tab-default-template` (group `agent-template`) on the shared recorder, as in step 1.4.
Done-when: tests cover Claude on api, cloud, subscription, mixed and none, opencode, a remembered choice, and a shell tab; the case renders on both profiles.

### 5.3 Wire the New Tab default — [ ]
Class: implement · Needs: 5.1, 5.2, 1.8 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/NewTabSheet.test.ts'`
Context: Decision 0162; the owner's review of `new-tab-default-template` is recorded in `reviews.md` like the slice reviews.
Do: Read the credential kind from the hello, apply the choice function, keep the user free to pick the other view, and keep behaviour unchanged when the server sends no credential kind.
Done-when: tests cover the live hello with and without the credential field, and `reviews.md` has an accepted line for `new-tab-default`.

### 5.4 Flip the default — [!]
Class: implement · Needs: 5.3, 2.8, 4.8 · Check: `scripts/dev.sh sh -c 'cargo test --workspace && pnpm -r test'`
Context: Blocked until (a) R74's stable-ids item has landed, so adding the `acp` manager does not rename `tmux:` ids (`docs/plans/roadmap/R74.md`), and (b) the owner has recorded the terms decision (fact 0069) and agrees to set decision 0162 `active`. Do not start before both are true.
Do: Turn `TERMENBOX_ACP` on by default in `packages/server` and its docs, keep `off` as the kill switch, confirm existing sessions keep their ids and views, refresh the Agent view subsection of `docs/guide/workspaces.md`, set decision 0162 `status: active`, and update decision 0137's note to point at 0162.
Done-when: a fresh server with an adapter installed offers Agent view by default (except Claude on a subscription, mixed or no credential), an upgraded server's existing workspaces keep their ids, `TERMENBOX_ACP=off` restores the old behaviour, and `node scripts/docs-media.mjs check docs/guide/workspaces.md` passes.

### 5.5 Hand the Agent view cases to the course's Master part — [ ]‖
Class: plan · Needs: 1.8 · Check: `grep -n "reading-levels" docs/plans/roadmap/R80.md`
Context: Decision 0160: the user manual is a course; Part 2, Master, includes "Terminal vs Agent view" and is planned after Part 1's cold read (`docs/plans/user-course.md` step 5.3, roadmap R80). Decision 0161: a chapter's numbered steps are generated between `<!-- course-steps:<case> -->` markers from a case's `steps`. This programme does not write course chapters.
Do: In `docs/plans/roadmap/R80.md` Notes, list the live Agent view cases (`reading-levels`, `status-strip`, `stepping`, `away-digest`, `stuck`, `inspector`, `queue-a-prompt`, `decide-faster`, `rule-suggestion`) as the input for the Agent view Master chapters, and add a link from the `docs/guide/workspaces.md` subsection back to the course once the chapter exists. Do not change R80's own steps.
Done-when: the Check lists the case ids in R80's Notes and `python3 docs/plans/roadmap/check.py` passes.
