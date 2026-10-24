# UX assessment and feature-video harness in GitHub CI

Plan for DECISION-0135, GUARDRAIL ux-hard-rules-block-the-pr-the-rest-is-advisory, FACT-0056, the procedures `improve-the-ux-from-ci` and `render-a-feature-video`, tracker item R73 (`docs/plans/roadmap/R73.md`). Written 2026-10-24 from the roadmap interview; this file alone is enough for a cold session.

Read first: `CLAUDE.md`, the five files above, `docs/plans/assets/ux/DESIGN-PRINCIPLES.md` (rules DR1-DR15, K1-K6), `docs/guardrails/ux-acceptance-targets.md`, `docs/plans/roadmap/archive/R36.md` and `packages/client/e2e/browser.mjs`. Repo: Termenbox (Rust server in `packages/server`, Svelte 5 client in `packages/client`, Playwright e2e in `packages/client/e2e`). Work on a branch off `develop` (never `develop` or `master`), one squashed PR per step, commit subjects `type(scope): summary` with a one-word scope, no Co-Authored-By lines. No compiler on this machine: run commands in the pinned container, `scripts/dev.sh [--full] <cmd>` (DECISION-0114). Merge only on the owner's "merge".

Goal: a CI job that, for every UI pull request, walks scripted scenarios on Chromium and WebKit across iPhone, Pixel, desktop and tablet or landscape profiles, measures taps, timings and the design rules, compares the pull request with `develop` rendered in the same run, fails on the hard rules and reports the rest; and renders the same scenarios as captioned MP4 videos on demand. The job calls no model and holds no secret. Every result is labelled emulated.

Legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved.

### 1.1 Named device profiles on the existing launcher — [ ]
Class: implement · Needs: none · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/browser.mjs --self-test'`
Context: R36 (done, PR #295) made `packages/client/e2e/browser.mjs` the one place that picks the engine (`E2E_ENGINE`: chromium, firefox, webkit), viewport (`E2E_VIEWPORT`), pixel ratio (`E2E_DPR`) and failure artifacts, with `launch()`, `contextOptions()`, `tap()`, `trace()` and `keepIfFailed()`. CI runs it in the "Browser matrix" job (`.github/workflows/ci.yaml`, `e2e/suite.mjs`; see `docs/skills/browser-support-check.md`). In Playwright's WebKit and Firefox a tap is a click; only Chromium proves touch (FACT-0056).
Do: Add named profiles to `browser.mjs`: `profile(name)` returns context options for `iphone` (390x844, DPR 2, touch, mobile), `pixel` (412x915, DPR 2.625, touch, mobile, Chromium only), `desktop` (1280x800, no touch), `tablet` (820x1180, touch) and `landscape` (844x390, touch, mobile), passed through `contextOptions()` so Firefox still drops `isMobile`. Keep the environment variables working unchanged (a profile is an alternative to `E2E_VIEWPORT` and `E2E_DPR`, not a replacement). Add `--self-test`, run when the file is the entry point: launch the selected engine with each applicable profile, open `about:blank`, check `innerWidth`, and exit non-zero on a mismatch. Do not add a second launcher.
Done-when: `--self-test` passes on Chromium (all profiles) and WebKit (all but `pixel`), and the Browser matrix job still passes.

### 1.2 Scenario format and runner on the scripted-WebSocket harness — [ ]
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm e2e:build && node e2e/ux/run.mjs --scenario home-to-session --engine chromium --profile iphone --out /tmp/ux-1'`
Context: `packages/client/e2e/app-lib.mjs` (it launches through `browser.mjs` since R36) serves the built `dist` and answers the app's WebSocket from a script, so the real app runs without a server binary; `e2e/appshell.mjs`, `e2e/app.mjs` and `e2e/decision.mjs` use it. DECISION-0135 wants one scenario file to produce both the assessment and a video.
Do: Create `packages/client/e2e/ux/run.mjs` and `packages/client/e2e/ux/lib/scenario.mjs`. A scenario file `e2e/ux/scenarios/<name>.mjs` exports `{ name, title, profiles, setup(server), steps }`; each step is `{ id, caption?, do(page, t), expect?(page) }` where `t.tap(locator)`, `t.type`, `t.key`, `t.swipe` wrap Playwright actions and increment a tap counter and a step clock. The runner takes `--scenario|--all`, `--engine` (sets `E2E_ENGINE`), `--profile`, `--out`, runs each step, saves a screenshot per step (`<out>/<scenario>/<engine>-<profile>/<step id>.png`), and writes `trace.json` (step ids, tap counts, times, errors). Add the first scenario `home-to-session` (Home to a session detail in the least taps) built on `app-lib.mjs`.
Done-when: the Check command exits 0 and `/tmp/ux-1/home-to-session/chromium-iphone/trace.json` lists the steps with a tap count, and a scenario whose `expect` fails makes the runner exit non-zero.

### 2.1 Inventory of paths, menus and shortcuts — [ ]‖
Class: explore · Needs: none · Check: `test -s docs/plans/assets/ux/INVENTORY.md && grep -q '^## Phone paths' docs/plans/assets/ux/INVENTORY.md && grep -q '^## Desktop shortcuts' docs/plans/assets/ux/INVENTORY.md`
Context: the improve loop (`docs/skills/improve-the-ux-from-ci.md`) scores each path; it needs a list of what exists. Sources: `packages/client/src/{AppShell,NavigationBar,TabStrip,SessionSheet,KeyRow,Palette}*.svelte` and the shortcut handlers (grep `keydown`), `lib/` for the palette commands, decisions 0071, 0082, 0100-0103, 0109.
Do: Write `docs/plans/assets/ux/INVENTORY.md` with sections `## Phone paths` (each user goal, its current start screen, the tap sequence, its depth and the owning component), `## Menus and sheets` (every menu, sheet and dialog with its entries and how it opens), `## Desktop shortcuts` (each binding, the action, where it is discoverable, conflicts with browser or tmux keys) and `## Key row` (default row, More sheet contents). Read the code; do not guess.
Done-when: the Check passes and each entry names the source file it was read from.

### 2.2 Assessment rubric — [ ]
Class: plan · Needs: 2.1 · Check: `for s in '^## Paths' '^## Menus and navigation' '^## Keyboard and key row' '^## Shortcuts' '^## Scoring'; do grep -q "$s" docs/plans/assets/ux/RUBRIC.md || exit 1; done`
Context: `improve-the-ux-from-ci` step 2 scores against `docs/plans/assets/ux/RUBRIC.md`. It must build on, not repeat, `DESIGN-PRINCIPLES.md` (DR1-DR15, K1-K6) and `ux-acceptance-targets`.
Do: Write `RUBRIC.md` with sections `## Paths` (tap count and time per goal from INVENTORY, target depth, Fitts and Hick checks), `## Menus and navigation` (depth at most 2, labels are words, back behaviour, recognition over recall, discoverability), `## Keyboard and key row` (the K rules, an emulated visualViewport check, key reachability), `## Shortcuts` (coverage of frequent actions, conflicts, a visible shortcut map, keyboard-only reachability and focus order) and `## Scoring` (0-2 per line, which lines are hard rules from the guardrail and which are advisory, how a finding names its number). Every line cites its DR/K rule, acceptance target or inventory entry.
Done-when: the Check passes and no rubric line lacks a source.

### 3.1 Phone scenarios — [ ]‖
Class: implement · Needs: 1.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/ux/run.mjs --all --group phone --engine chromium --profile iphone --out /tmp/ux-3'`
Context: scenario format from 1.2; paths from INVENTORY (2.1 if done, otherwise read the components). Fixtures live in `app-lib.mjs` (for example `CLAUDE_PROMPT`); extend it for new states.
Do: Add scenarios `push-answer` (push deep link to a sent answer, at most 2 taps), `tabs-and-sessions-sheet` (switch session via tab strip and sheet), `key-row-and-keyboard` (open the composer, shrink `visualViewport` to emulate the keyboard, send keys from the row), `add-server` (the four-step wizard on the scripted server), `settings-menus` (every Settings entry reachable and back). Each has captions for video use.
Done-when: all five pass on iphone with Chromium and each records its tap count.

### 3.2 Desktop scenarios — [ ]‖
Class: implement · Needs: 1.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/ux/run.mjs --all --group desktop --engine chromium --profile desktop --out /tmp/ux-3b'`
Context: desktop shell, palette (Ctrl/Cmd+K) and panes are decisions 0101 and 0109; shortcuts are listed in INVENTORY.
Do: Add scenarios `palette-and-shortcuts` (open the palette, run three commands, trigger every documented shortcut), `keyboard-only` (reach Home, a session and Settings with Tab and shortcuts only; record the focus order), `panes` (open the right pane, switch session).
Done-when: all three pass on desktop and `keyboard-only` records a focus order list.

### 3.3 Tablet and landscape assessment scenarios — [ ]‖
Class: implement · Needs: 1.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/ux/run.mjs --all --group core --engine chromium --profile landscape --out /tmp/ux-3c'`
Context: DECISION-0135: tablet and landscape are assessed only, never designed here; tablet layouts stay deferred until the real-device pass (`docs/plans/termenbox-roadmap.md`).
Do: Mark `home-to-session`, `push-answer` and `key-row-and-keyboard` as `group: core` and run them on the `tablet` and `landscape` profiles, recording overflow, target sizes and keyboard cover. No layout change.
Done-when: the Check passes on `landscape` and `tablet` and the trace records overflow and target sizes for them.

### 4.1 Metrics collector — [ ]
Class: implement · Needs: 1.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/ux/run.mjs --scenario home-to-session --engine chromium --profile iphone --out /tmp/ux-4 && node e2e/ux/report.mjs /tmp/ux-4 >/tmp/ux-4/report.json && node -e "const r=JSON.parse(require(\"fs\").readFileSync(\"/tmp/ux-4/report.json\"));if(!r.scenarios.length||!r.scenarios[0].metrics.targets)process.exit(1)"'`
Context: the audit rules exist as page probes in `packages/client/e2e/gallery.mjs` (`probe()` for DR1, DR2, DR5, DR8, DR15) and `docs/plans/assets/ux/audit.mjs` and `measure.mjs`. The hard rules are in `docs/guardrails/ux-hard-rules-block-the-pr-the-rest-is-advisory.md`.
Do: Move the probe into `packages/client/e2e/ux/lib/probe.mjs` (the gallery imports it, behaviour unchanged) and call it after each step. Create `e2e/ux/report.mjs` that merges the traces into `report.json`: per scenario, engine and profile the tap counts, step times, target sizes under 44 pt, overflow, contrast failures, console errors, keyboard-cover and element-moved checks, focus order, and first usable paint on a CDP-throttled 4G profile (Chromium only, median of three runs). Each finding has `rule`, `hard` (true for the seven guardrail rules) and `where`.
Done-when: the Check passes, `gallery.mjs` still passes, and a seeded 30 px button makes `report.json` list a hard DR1 finding.

### 4.2 Base-versus-head comparison and summary — [ ]
Class: implement · Needs: 4.1, 2.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/ux/compare.mjs --self-test'`
Context: DECISION-0135 re-renders `develop` in the same run; no baseline files are committed.
Do: Create `e2e/ux/compare.mjs` taking two report directories (`--base`, `--head`), producing `diff.json` (per metric before, after, delta), a pixel diff image per step when the screenshots differ (use `pixelmatch` as a client devDependency, or a plain RGBA comparison if it is not worth the dependency), and `summary.md`: hard failures first, then regressions and improvements by delta, then advisory findings, each with a rubric line from `RUBRIC.md` when it applies; every table is headed "emulated". `--self-test` builds two small fake reports and checks the exit code (1 on a hard failure in head, 0 otherwise) and that the summary lists it.
Done-when: `--self-test` passes and a head report with a new hard finding exits 1 while an advisory-only difference exits 0.

### 5.1 The UX workflow — [ ]
Class: implement · Needs: 4.2, 3.1 · Check: `gh run list --workflow ux.yaml --limit 1 --json conclusion -q '.[0].conclusion' | grep -qx success`
Context: `.github/workflows/ci.yaml` jobs `e2e` and `e2e-static` build the client (`pnpm --filter termenbox-client run e2e:build`) and run the scripts; pin freshness is checked by `pin-freshness.yaml` (pin action versions the same way). Pull requests target `develop`.
Do: Create `.github/workflows/ux.yaml` triggered by `pull_request` (branches `develop`, `master`; paths `packages/client/src/**`, `packages/client/e2e/**`, `docs/plans/assets/ux/**`) and `workflow_dispatch` (inputs `video`, `profile`). Steps: check out head and `develop` (worktree), install pnpm, tmux not needed, `pnpm exec playwright-core install --with-deps webkit` (as the Browser matrix does; Chromium is the `chrome` channel), build both, run all scenarios on both for every engine and profile that applies, `compare.mjs`, upload `ux-report` (retention 14 days), post or update one sticky comment with `summary.md` (marker `<!-- ux-report -->`, `gh api` with the default token, `pull-requests: write` only), and fail on exit code 1 from `compare.mjs`. On `workflow_dispatch` without `video` it runs on the chosen ref only. No secrets, no model call.
Done-when: a pull request touching `packages/client/src` shows the job, the sticky comment and the artifact, a docs-only pull request does not start it, and a seeded hard-rule break fails it (check by a throwaway branch, then delete it).

### 6.1 Video renderer — [ ]‖
Class: implement · Needs: 1.2 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/ux/video.mjs --scenario home-to-session --profile phone --out /tmp/ux-6 && ffprobe -v error -show_entries stream=codec_name,width,height -of csv=p=0 /tmp/ux-6/home-to-session.mp4 | grep -q h264 && test -s /tmp/ux-6/home-to-session.gif'`
Context: `docs/skills/render-a-feature-video.md` describes the output: captioned MP4 and a GIF poster. Playwright records with `recordVideo`; the harness already drives the real app.
Do: Create `e2e/ux/video.mjs`: run a scenario on Chromium with `recordVideo` at 2x size, inject an overlay (tap ripple at each `t.tap` target, caption bar from the step `caption`, placed so it never covers the target), wait for the page to settle between steps, then encode with `ffmpeg` to H.264 MP4 and extract a GIF poster. Fail if the run had a console error. Add `ffmpeg` to the pinned dev container image if it is missing (`docker/` and `scripts/dev.sh`).
Done-when: the Check passes and the MP4 shows a ripple and a caption on a sampled frame (extract one with `ffmpeg -ss` and look at it).

### 6.2 Video dispatch in the workflow — [ ]
Class: implement · Needs: 5.1, 6.1 · Check: `gh workflow run ux.yaml -f video=home-to-session -f profile=phone && sleep 5 && gh run list --workflow ux.yaml --limit 1 --json event -q '.[0].event' | grep -qx workflow_dispatch`
Context: `ux.yaml` from 5.1 already has the `video` and `profile` inputs.
Do: When `video` is set, install `ffmpeg`, run `video.mjs` for that scenario and profile and upload `ux-video-<name>` (retention 30 days); skip the comparison.
Done-when: a dispatch run produces the artifact with an MP4 and a GIF that open and play.

### 7.1 Calibrate and promote the rules — [ ]
Class: implement · Needs: 5.1 · Check: `node packages/client/e2e/ux/flake-check.mjs --runs 3`
Context: the guardrail promotes a rule to blocking only after three runs on `develop` agree. Throttled paint and WebKit viewport simulation are the likely flakes.
Do: Run the job on `develop` three times (dispatch). Add `e2e/ux/flake-check.mjs` that downloads the three reports (`gh run download`) and prints every metric whose value differs between runs. Fix or loosen flaky probes (median, tolerance), mark any rule that still flakes `hard: false` in `report.mjs` with a comment naming the run ids, and list them in the pull request.
Done-when: three consecutive runs on `develop` give identical hard-rule findings (or none), and each rule left advisory is named with its reason.

### 7.2 One full improvement pass, run through the procedure — [ ]
Class: implement · Needs: 7.1, 2.2 · Check: `gh pr list --search "head:feat/ux- is:pr" --json number,title -q 'length>0' | grep -qx true`
Context: this proves `docs/skills/improve-the-ux-from-ci.md` end to end. The owner reviews visual changes before merge.
Do: Follow the procedure on the first real report from `develop`: pick at most three findings with a number each, implement each on its own branch (`feat/ux-<slug>`), let the UX job compare base and head, and open one pull request per finding with before and after numbers and screenshots, labelled emulated. File unfixed findings as roadmap steps. Update the procedure where a step proved unclear.
Done-when: at least one pull request shows an improved targeted metric with no hard-rule regression on any engine or profile, and the procedure's changes (if any) are merged with it.
