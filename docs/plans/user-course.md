# The user course, Part 1 "Start": six chapters, six videos, natural pacing (several pull requests to `develop`)

Plan for DECISION-0160 (guided course in `docs/course`, Start before Master), DECISION-0161 (rhythm model, still poster, steps from the case, replayed installer; amends 0158), DECISION-0158 (the one recorder and the review and pick path), FACT-0068, GUARDRAILS `course-chapters-keep-the-fixed-anatomy-and-generated-steps`, `a-course-part-is-done-only-after-a-recorded-cold-read`, `media-commands-come-only-from-people-with-write-access`, `nothing-is-published-without-a-matching-approval-record` and `ux-scenarios-assert-a-state-before-they-capture-it`; the procedures `write-a-course-chapter`, `run-a-cold-read-test`, `regenerate-the-user-course` (the interview behind all of this: every question, option and answer), `render-a-feature-video` and `review-and-publish-docs-media`; tracker item R80 (`docs/plans/roadmap/R80.md`). Written 2026-10-31 from the roadmap interview; this file alone is enough for a cold session.

Read first: `CLAUDE.md`, the records above, `packages/client/e2e/video/{lib,cases,render,inputs-hash,real-server}.mjs`, `packages/client/e2e/app-lib.mjs` (`ScriptedServer`, `startApp`), `scripts/docs-media*.mjs`, `.github/workflows/{docs-media,docs-media-review,ci}.yaml`, `docs/media/`, `README.md`, `docs/guide/*.md`, `docs/plans/docs-media-and-checks.md` (the pipeline this builds on). Repo: Termenbox (Rust server `packages/server`, Svelte 5 client `packages/client`, Playwright scripts `packages/client/e2e`). Work only in a pool worktree (`sh scripts/pool.sh acquire <branch>`); branch names follow the project pattern; commit subjects `type(scope): summary` (one-word scope), body `Why:` then `What:`, no Co-Authored-By. Build and test in the pinned container `scripts/dev.sh [--full] <cmd>` (DECISION-0114); if it cannot reach Docker, use host node for the `node` checks and say which checks were not run.

Constraints. (1) A dispatch-only workflow runs only after its file is on `develop` (FACT-0067). (2) Picked media lands through a pull request the developer opens from the branch the review workflow pushes (DECISION-0158). (3) Nothing is ticked, approved or opened on behalf of the reviewer. (4) The Master part (Terminal vs Agent view, scrollback search, key row, notifications, several servers, "it won't connect") is out of scope here and becomes its own plan after the cold read. Land in pull requests: A (1.x), B (2.x), C (3.x), D (4.x), then 5.x.

Goal: a newcomer who opens the README finds "New here? Start the course", follows six short chapters (install, connect your phone, first workspace, talk to your agent, answer from the train, come back later), watches one natural-paced video per chapter, and ends with a working phone connected to their server; every chapter keeps its fixed anatomy and its steps are generated from the recorded case; a person new to Termenbox has followed it on a real phone and the stalls are recorded.

Legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved.

### 1.1 Rhythm model for the recorder — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm exec vitest run e2e/video/rhythm.test.mjs e2e/video/knobs.test.mjs'`
Context: DECISION-0161. Today `lib.mjs` `humanPage` draws flat random delays: `pause` 600 to 1200 ms after every change, typing 55 to 140 ms per key, pointer moves of 18 to 30 steps at 10 to 22 ms with a 120 to 350 ms settle, a caption dwell of `min(1800, 500 + 35 * chars)`. Rendered, that reads as a fast robot. `knobs.test.mjs` is a vitest file (the client's `pnpm test` runs every `*.test.mjs`; a `node:test` file fails the job).
Do: create `packages/client/e2e/video/rhythm.mjs` exporting pure functions that take a seeded generator (`mulberry32(seed)`; the seed is derived from the case id and profile so a render is reproducible and its input hash does not change): `readPause(text, rng)` = 400 ms + 55 ms per character of the caption, capped at 4500 ms, plus or minus 15 percent; `hesitation(rng)` = 500 to 1100 ms before a choice; `restAfterResult(rng)` = 1800 to 3000 ms on a result worth noticing; `typingDelays(text, rng)` = a base of 140 to 230 ms per key in bursts of 3 to 6 keys at 0.6 times the base, each burst followed by a stall of 250 to 600 ms, keys that are not letters 1.4 times slower; `pointerPath(from, to, rng)` = a cubic Bezier of 24 to 40 samples with ease in and out, an overshoot of 4 to 10 px on 40 percent of moves and a correction in 80 to 160 ms. Change `humanPage` to use them (keep `paceFactor` and `--pace slow` as a multiplier on every duration) and add `h.result(text)` (a rest on a result) for cases to call. Write `rhythm.test.mjs` (vitest): same seed gives the same output; a longer caption never gives a shorter reading pause; typing has at least one stall over 400 ms in 60 characters and a median between 120 and 230 ms; every pointer path ends exactly on its target; the bounds above hold over 200 seeds.
Done-when: the Check passes and `grep -n "rand(" packages/client/e2e/video/lib.mjs` shows no flat delay left in the human actions.
Progress (2026-10-31): `rhythm.mjs`, `rhythm.test.mjs` added; `humanPage` uses them, `render.mjs` passes the case id as seed; `h.result(text)` added. Vitest: 10 tests pass (run on host node with another pool slot's `node_modules`; `scripts/dev.sh` could not reach Docker: permission denied on the socket). Deviation: the typing base is 230 to 340 ms, not 140 to 230, because 0.6 of 140 to 230 cannot give the required median of 120 to 230 ms; 1.2 tunes it by ear. `h.pause()` now defaults to the reading time of the pending caption, and `move` ends with a hesitation before the press.

### 1.2 Re-time the four dialog cases and look at one — [!]
Class: review · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case recent-folder --profile phone --out /tmp/rhythm && ffprobe -v error -show_entries format=duration -of csv=p=0 /tmp/rhythm/recent-folder-phone.mp4'`
Context: the first rendered video, `recent-folder-phone`, is 17.4 s with flat delays (PR #427). A person judges the new rhythm; a session cannot.
Do: render the four dialog cases on the phone profile with the new model, report each duration, and give the owner the files to watch. Adjust the constants in `rhythm.mjs` from their words (too fast, too slow, where). Close PR #427 as superseded once a new render exists.
Done-when: the Check prints a duration between 30 and 60, and the owner has said the pace feels natural (quote them in the step's Progress).
Progress (2026-10-31): blocked. `scripts/dev.sh` cannot reach Docker here (permission denied on the socket) and the owner has not judged a render. Needs a machine with Docker and ffmpeg, then the owner's words. Steps 2.x and 3.x do not depend on it.

### 2.1 Cases declare their steps — [x]‖
Class: implement · Needs: none · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm exec vitest run e2e/video/steps.test.mjs'`
Context: DECISION-0161: a case lists its steps once and the chapter's numbered steps are generated from that list. `cases.mjs` exports `cases`, an object keyed by case id with `title`, `group`, `page`, `inputs`, `run`; `render.mjs --list --json` prints the metadata; `render.json` is written beside each video.
Do: add `steps: [{ id, caption, label }]` to every case (`caption` is what the screen says, `label` the exact text or accessible name that is tapped, or null for a non-tap step). Add `h.step(id)` that a case calls as it performs each step; it throws when called out of order or with an id not declared, and the render fails when a declared step was never reached. `render.json` records `steps` and `seconds` (the video length). `--list --json` includes `steps`. Write `steps.test.mjs` (vitest) with a fake `h`: out of order, unknown id and a missing step each throw.
Done-when: the Check passes and `node packages/client/e2e/video/render.mjs --list --json` (container) prints `steps` for the four dialog cases.
Progress (2026-10-31): `steps.mjs` (declaration check, order tracker) and `steps.test.mjs` (6 tests, pass on host node with another slot's `node_modules`); all six cases declare `steps` and call `h.step(id, avoid)`, which shows the declared caption, so the caption lives once; `render.mjs` fails when a step was never reached and writes `steps` and `seconds` (ffprobe) to `render.json`, and `--list --json` includes `steps`. Not run: `--list --json` itself (it imports Playwright, no container) and any render. Labels that I could not confirm from the Svelte source and that a render must confirm: `Choose` (folder-change when no folder is set), `More`, `Next` (install-next), `shell`.

### 2.2 Generate and check the steps block — [x]‖
Class: implement · Needs: 2.1 · Check: `node scripts/docs-course.mjs --self-test`
Context: DECISION-0161; guardrail `course-chapters-keep-the-fixed-anatomy-and-generated-steps`. The block sits between `<!-- course-steps:<case> -->` and `<!-- /course-steps -->` in a chapter. `scripts/docs-media.mjs` is the model for marker handling (idempotent, relative paths). Reading the catalog must not need Playwright: the script takes the catalog as a JSON file that CI writes with `render.mjs --list --json > catalog.json`.
Do: create `scripts/docs-course.mjs` with `steps --catalog <json> <chapter.md>...` (replaces each block with a numbered list, one line per step: `1. <caption>. Tap **<label>**.` or just the caption when there is no label) and `check --catalog <json> <chapter.md>...` (fails on a stale block, a marker naming an unknown case, or a block missing), and `--self-test` (fresh, stale, unknown case, second run changes nothing, a label with Markdown characters is escaped).
Done-when: the Check passes.
Progress (2026-10-31): `scripts/docs-course.mjs` with `steps`, `check` and `--self-test` (fresh, stale, unknown case, idempotent, escaped label, no block, empty steps); the self-test runs in the `ci.yaml` docs-media step. Markdown-special characters in a label are backslash-escaped, so `+ New` renders as `\+ New` in the source.

### 2.3 Still poster instead of a GIF — [ ]
Class: implement · Needs: 2.1 · Check: `node --test scripts/docs-media-review.test.mjs && node scripts/docs-media.mjs --self-test && node scripts/docs-media-review-page.mjs --self-test && ! grep -n "gif" scripts/docs-media.mjs scripts/docs-media-review-cli.mjs scripts/docs-media-review-page.mjs packages/client/e2e/video/render.mjs`
Context: DECISION-0161 (amends 0158), FACT-0068. A chapter shows a small captioned still linking to the MP4 with a "Watch, N s" line, where N is `seconds` from `render.json`. Stills are saved by `h.still(name)`; a case marks a still named `poster` for its poster, falling back to its first still. The review page (`docs-media-review-page.mjs`) uses the poster as the `<video poster>`; `approve` in `docs-media-review-cli.mjs` copies the files named in `render.json`; `docs-media.mjs` `embed` and `check` write and verify the block; `ci.yaml` runs the self-tests.
Do: remove the GIF from `render.mjs` (the ffmpeg GIF pass and `gifWidth`) and from the file lists; make the review page, `approve`, `embed` and `check` use `<case>-<profile>-poster.png` (or the first still) and the MP4; the embed block becomes `[![<caption>](<poster>)](<mp4>)` plus `Watch, <N> s` and the "rendered walkthrough on scripted data" line; update the tests and the CI step. Remove `docs/media/recent-folder-phone.gif` only in the pull request that supersedes #427.
Done-when: the Check passes and a render leaves no `.gif` in the output folder.

### 3.1 Install case: replay the real installer — [ ]‖
Class: implement · Needs: 1.1, 2.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case install --profile desktop --out /tmp/course && test -s /tmp/course/install-desktop.mp4'`
Context: DECISION-0161: Install replays the real installer's captured output in a terminal view. CI already runs the installer in clean containers (jobs "Native installer"); `packages/client/installer/install.sh` is the script; its output ends with a QR, a link and a short code (DECISION-0126). The video says it is a rendered walkthrough. The profile for this chapter is desktop (the install happens on the server's machine); the other chapters are phone.
Do: add `scripts/capture-installer-output.sh` that runs the installer in a clean container under `script --timing` and writes `packages/client/e2e/video/fixtures/installer.typescript` and `installer.timing` (strip any real secret, hostname or IP and replace the pairing link with a fixed example; the pairing code is a rate-limited PAKE secret of a throwaway server). Add `packages/client/e2e/video/terminal-replay.mjs` that renders an xterm.js page (the client already depends on xterm) and plays the typescript with its timings after the command is typed at human pace. Add the case `install` (group `start`, page `docs/course/01-install.md`, steps: open a terminal, paste the command, wait for the QR and link) ending in an assertion that the terminal shows the pairing link line, with a `poster` still of the finished output.
Done-when: the Check passes, changing the expected link line makes the case exit 1 with no video, and `grep -rn "[0-9]\{1,3\}\.[0-9]\{1,3\}\.[0-9]\{1,3\}\.[0-9]\{1,3\}" packages/client/e2e/video/fixtures` finds no real address.

### 3.2 Connect case — [ ]‖
Class: implement · Needs: 1.1, 2.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case add-server --profile phone --out /tmp/course && test -s /tmp/course/add-server-phone.mp4'`
Context: the case `add-server` exists in `cases.mjs` (real server binary through `real-server.mjs`; build it first with `cargo build --release -p termenbox-server` or set `TERMENBOX_BIN`); it has not been rendered in CI or here. Chapter 2: you scan the QR from chapter 1 (the video starts at the pasted link).
Do: render it, fix what fails, add `steps` and a `poster` still at the "ready to add" screen, use `h.result` where the server shows connected, and keep its final assertion (the server listed as connected).
Done-when: the Check passes on the phone profile and the final assertion fails when changed.

### 3.3 First-workspace case — [ ]‖
Class: implement · Needs: 1.1, 2.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case recent-folder --profile phone --out /tmp/course && test -s /tmp/course/recent-folder-phone.mp4'`
Context: `recent-folder` exists and renders (17 s with flat delays); it opens the New workspace dialog, taps a recent folder and an agent. Chapter 3.
Do: add `steps`, mark the `poster` still (dialog open), call `h.result` when the workspace opens, keep the final assertion, and add one beat the newcomer needs: the workspace name and the folder chosen are read aloud in the caption.
Done-when: the Check passes and the video runs 30 to 60 s.

### 3.4 Talk-to-your-agent case — [ ]‖
Class: implement · Needs: 1.1, 2.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case talk-to-agent --profile phone --out /tmp/course && test -s /tmp/course/talk-to-agent-phone.mp4'`
Context: chapter 4. `ScriptedServer` in `packages/client/e2e/app-lib.mjs` scripts a workspace with an agent tab and its screen; the key row and the soft keyboard are covered by the key-row e2e (`grep -rln "key row\|keyrow" packages/client/e2e`). Read both before writing the case.
Do: add the case `talk-to-agent` (group `start`, page `docs/course/04-talk-to-your-agent.md`): open the workspace, tap the input, type a short prompt at human pace, send it, watch the scripted agent answer, use the key row once for a key the soft keyboard lacks (Esc), and end on an assertion that the agent's answer is on screen. Steps, `poster`, `h.result` on the answer.
Done-when: the Check passes and the final assertion fails when the expected answer is changed.

### 3.5 Answer-from-the-train case — [ ]‖
Class: implement · Needs: 1.1, 2.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case answer-from-the-train --profile phone --out /tmp/course && test -s /tmp/course/answer-from-the-train-phone.mp4'`
Context: chapter 5; `docs/guide/workspaces.md` "Needs you": when an agent waits for an answer the banner names the workspace and offers its choices, the answer goes to the tab that asked, and a push deep link opens that tab. Notifications carry no agent text (guardrail `notifications-carry-no-agent-text`): the video shows the banner inside the app, not a lock-screen mock-up with agent text.
Do: add the case `answer-from-the-train` (group `start`, page `docs/course/05-answer-from-the-train.md`): the scripted agent asks a question, the banner appears, tap its choice, the tab shows Working. Steps, `poster` at the banner, `h.result` on the banner and on Working; end on an assertion that the tab's status is Working.
Done-when: the Check passes and the final assertion fails when the expected status is changed.

### 3.6 Come-back-later case — [ ]‖
Class: implement · Needs: 1.1, 2.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case resume-after-close --profile phone --out /tmp/course && test -s /tmp/course/resume-after-close-phone.mp4'`
Context: `resume-after-close` exists in `cases.mjs` and has not been rendered; reconnect is re-attach (DECISION-0054). Chapter 6.
Do: render it, fix what fails, add `steps`, a `poster` at the re-attached screen and `h.result` when the same screen returns; keep the final assertion (the same workspace attached with its screen after the reload).
Done-when: the Check passes and the final assertion fails when changed.

### 4.1 Course check — [ ]
Class: implement · Needs: 2.2 · Check: `node scripts/check-course.mjs --self-test`
Context: guardrail `course-chapters-keep-the-fixed-anatomy-and-generated-steps`. A chapter is a file `docs/course/NN-*.md` (not `README.md`). `scripts/docs-course.mjs check` covers a stale steps block; `scripts/docs-media.mjs check` covers a poster whose video is missing; `scripts/check-site.mjs` is the model for a repository check that CI runs. `.github/workflows/ci.yaml` has a step "Test the docs-media scripts".
Do: create `scripts/check-course.mjs`: for every chapter, the parts in order (a first line that starts with "In " and names a time, a `<!-- media:<id> -->` block, a `<!-- course-steps:<case> -->` block, a heading `If it doesn't work` with exactly two entries, a heading `You can now`, a final `Next` link), the word count of prose outside the generated and media blocks at most 250, every relative link resolves to a file, and it calls the two checks above. `--self-test` builds good and bad chapters in a temp folder (missing part, 251 words, broken link, stale steps) and asserts each failure. Wire into `ci.yaml` next to the docs-media tests (a chapter folder that does not exist yet passes).
Done-when: the Check passes and the CI step is present.

### 4.2 Course index, story and task index — [ ]
Class: implement · Needs: 2.2 · Check: `test -f docs/course/README.md && grep -c "0[1-6]-" docs/course/README.md | awk '$1>=6{exit 0} {exit 1}'`
Context: DECISION-0160; procedure `write-a-course-chapter`. The reference pages in `docs/guide/` stay; the task index links into chapters first and the reference second.
Do: write `docs/course/README.md`: the running story in a short paragraph (you start a long agent task at your desk, leave, and finish it from the train), the six chapters with their promise and time, a "How do I…" index of at least eight tasks linking to a chapter or a reference section, and a line that Master follows. Vivid, second person, no feature lists.
Done-when: the Check exits 0 and every relative link in the file resolves (`node scripts/check-course.mjs` once 4.1 exists).

### 4.3a Chapter 1, Install — [ ]‖
Class: implement · Needs: 3.1, 4.1, 4.2 · Check: `node scripts/check-course.mjs docs/course/01-install.md`
Context: procedure `write-a-course-chapter`; reference `docs/guide/install.md` (the one command, firewalls, where agents run; DECISIONS 0058, 0075, 0078, 0113, 0118, 0126). The two likeliest failures: a firewall blocking the UDP port (the installer opens `ufw` or `firewalld` itself but not a provider firewall) and running as root vs user (`docs/guide/install.md`, DECISION-0118).
Do: write the chapter following the procedure, with the `<!-- course-steps:install -->` and `<!-- media:install-desktop -->` markers.
Done-when: the Check passes and the chapter reads aloud without a second reading.

### 4.3b Chapter 2, Connect your phone — [ ]‖
Class: implement · Needs: 3.2, 4.1, 4.2 · Check: `node scripts/check-course.mjs docs/course/02-connect-your-phone.md`
Context: as 4.3a; reference `docs/guide/install.md` and `docs/guide/networking.md` (paths, LAN only, the tunnel); failures: the QR scanned by another app, a network that blocks UDP (`docs/guide/networking.md`).
Do: write the chapter with the markers for `add-server` (phone).
Done-when: the Check passes.

### 4.3c Chapter 3, First workspace — [ ]‖
Class: implement · Needs: 3.3, 4.1, 4.2 · Check: `node scripts/check-course.mjs docs/course/03-first-workspace.md`
Context: as 4.3a; reference `docs/guide/workspaces.md` "Workspaces and tabs" and the New workspace dialog (`docs/plans/new-workspace-dialog-ux.md`); failures: the agent is not installed on the server, the folder is outside the home directory.
Do: write the chapter with the markers for `recent-folder` (phone).
Done-when: the Check passes.

### 4.3d Chapter 4, Talk to your agent — [ ]‖
Class: implement · Needs: 3.4, 4.1, 4.2 · Check: `node scripts/check-course.mjs docs/course/04-talk-to-your-agent.md`
Context: as 4.3a; reference `docs/guide/workspaces.md`; failures: the soft keyboard covers the input (safe areas), a key that the phone keyboard lacks (the key row).
Do: write the chapter with the markers for `talk-to-agent` (phone).
Done-when: the Check passes.

### 4.3e Chapter 5, Answer from the train — [ ]‖
Class: implement · Needs: 3.5, 4.1, 4.2 · Check: `node scripts/check-course.mjs docs/course/05-answer-from-the-train.md`
Context: as 4.3a; reference `docs/guide/workspaces.md` "Needs you" and "Notifications"; notifications carry no agent text; failures: notifications not allowed on the phone, the app not installed to the home screen on iOS.
Do: write the chapter with the markers for `answer-from-the-train` (phone).
Done-when: the Check passes.

### 4.3f Chapter 6, Come back later — [ ]‖
Class: implement · Needs: 3.6, 4.1, 4.2 · Check: `node scripts/check-course.mjs docs/course/06-come-back-later.md`
Context: as 4.3a; reference `docs/guide/workspaces.md` "Restore"; failures: the workspace shows dormant after the tmux server was gone, the server is unreachable.
Do: write the chapter with the markers for `resume-after-close` (phone) and a Next link to the Master note in the index.
Done-when: the Check passes.

### 4.4 README entry and reference back-links — [ ]
Class: implement · Needs: 4.2 · Check: `head -20 README.md | grep -q "docs/course/README.md" && grep -l "docs/course" docs/guide/*.md | wc -l | awk '$1>=3{exit 0} {exit 1}'`
Context: DECISION-0160: the README links to the course first; the guide pages link back to the matching chapter. `scripts/check-site.mjs` checks that the README shows the install command: do not change that line.
Do: add "New here? Start the course" with the story's first line within the first twenty lines of `README.md`; in `install.md`, `networking.md` and `workspaces.md` add a line near the top pointing to the chapter that summarises the section.
Done-when: the Check exits 0, `node scripts/check-site.mjs` passes and `node scripts/check-course.mjs` passes.

### 5.1 Render, review and pick — [ ]
Class: review · Needs: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 4.3a, 4.3b, 4.3c, 4.3d, 4.3e, 4.3f, 2.3 · Check: `node scripts/docs-media.mjs check docs/course/0*.md`
Context: needs the pull requests A to D merged. A developer with write access reviews on an iPhone with the procedure `review-and-publish-docs-media`; their word decides and nothing here ticks for them (guardrail `media-commands-come-only-from-people-with-write-access`).
Do: dispatch `gh workflow run docs-media.yaml --ref develop -f case=all -f profile=both` (or the six cases on the profiles above), give the reviewer the issue and the preview link, record their ticks and any `/regen` notes, apply the notes with the procedure, open the pull request from the compare link the workflow posts, and merge it when green.
Done-when: the Check exits 0 (every chapter's video, poster and `render.json` are in `docs/media/` on `develop`) and the reviewer has replied with the result.

### 5.2 Cold read on a real phone — [ ]
Class: review · Needs: 5.1, 4.4 · Check: `grep -l "cold read" docs/facts/*.md`
Context: guardrail `a-course-part-is-done-only-after-a-recorded-cold-read`; procedure `run-a-cold-read-test`. A person who has not used Termenbox follows the course from the README on a real phone. A session prepares the sheet and writes the fact; a person runs the test.
Do: follow the procedure, save the result as a fact (time per chapter, stalls, quotes, phone, date, no names), fix each stall in the chapter or file it as a roadmap step, and repeat the read for a fixed chapter with a different person.
Done-when: the Check lists the new fact and each stall in it is fixed or has a roadmap step.

### 5.3 Close Part 1 and open Master — [ ]
Class: plan · Needs: 5.2 · Check: `python3 docs/plans/roadmap/check.py`
Context: Master chapters (Terminal vs Agent view, scrollback search, key row and soft keyboard, notifications, several servers, "it won't connect") map to steps 2.2 to 2.4 of `docs/plans/docs-media-and-checks.md` (`answer-decision`, `switch-workspaces`, `key-row-keyboard`, `agent-view`, `scrollback-search`, `unreachable-server`, `update-server`); the cold read says which come first.
Do: mark R80 done and archive it per the tracker's rules, file the Master part as a roadmap item with its own plan (use `regenerate-the-user-course` to re-ask only the questions whose answers changed), mark the media plan's steps 2.2 to 2.4 as moved to it (`[>]`), and regenerate the order table with `python3 docs/plans/roadmap/rank.py`.
Done-when: the Check passes and the Master item exists in the order table.
