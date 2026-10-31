# Documentation media pipeline and docs checks (several pull requests to `develop`)

Plan for DECISION-0158 (one recorder, developer pick, `docs/media` through a pull request; amends 0135), DECISION-0159 (docs checks), FACT-0066, FACT-0067, GUARDRAILS `media-commands-come-only-from-people-with-write-access`, `nothing-is-published-without-a-matching-approval-record` and `ux-scenarios-assert-a-state-before-they-capture-it`, the procedures `review-and-publish-docs-media` and `render-a-feature-video`, tracker item R79 (`docs/plans/roadmap/R79.md`). Written 2026-10-29 from the roadmap interview; this file alone is enough for a cold session.

Read first: `CLAUDE.md`, the records above, `docs/plans/new-workspace-dialog-ux.md` steps 6.1 and 6.2 (how the recorder and the dispatch workflow were built and what was learned), `packages/client/e2e/video/{lib,cases,render}.mjs`, `packages/client/e2e/app-lib.mjs` (`ScriptedServer`, `startApp`), `.github/workflows/{dialog-videos,deploy-site,ci}.yaml`, `scripts/check-site.mjs`, `docs/guide/configuration.md`. Repo: Termenbox (Rust server `packages/server`, Svelte 5 client `packages/client`, Playwright scripts `packages/client/e2e`, static site `packages/site`). Branch off `develop` named to the project pattern (never `develop` or `master`); commit subjects `type(scope): summary` (one-word scope), body `Why:` then `What:`, no Co-Authored-By. Build and test in the pinned container `scripts/dev.sh [--full] <cmd>` (DECISION-0114); if it fails on this host use host tools as `docs/plans/new-workspace-dialog-ux.md` 1.1 and 2.1 describe. Merge only on the owner's "merge".

Two constraints shape the order. (1) A dispatch-only workflow can be run only after its file is on `develop` (FACT-0067), so every new workflow ships in a pull request first and is dispatched after the merge; its step stays `[~]` until it has run. (2) Picked media lands in `develop` through a pull request the developer opens from a branch the review workflow pushes (a pull request opened with the workflow token would not start CI); `deploy-site` is unchanged. Land in four pull requests: A (1.x, 2.x), B (3.x, 4.x), C (5.1), D (6.x), then 7.x.

Goal: the documentation videos and stills come from one recorder, are reviewed by developers on a phone from a preview page, are sent back with `/regen` or picked by a tick, land under `docs/media/` through a pull request, and are embedded in the README and guide pages by markers; stale videos are detected; the CLI reference is generated and links, anchors and guide-versus-code drift are checked in CI.

Legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved.

### 1.1 Case catalog metadata and `--list` — [x]
Class: implement · Needs: none · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/video/render.mjs --list --json | node -e "const l=JSON.parse(require(\"fs\").readFileSync(0,\"utf8\"));if(l.length<4||l.some(c=>!c.id||!c.title||!c.group||!c.page||!(c.inputs&&c.inputs.length)))process.exit(1)"'`
Context: `packages/client/e2e/video/cases.mjs` exports `cases`, an object keyed by case id (`recent-folder`, `other-agent`, `browse-folder`, `type-path`); each has `storage` and `async run(c)`. `render.mjs` takes `--case`, `--profile phone|desktop`, `--out`. DECISION-0158 gives each case an id, title, group (`dialog|core|daily|agent|diagnostics`), the docs page that embeds it, and the source paths its video depends on.
Do: add to each case `title`, `group`, `page` (for the four dialog cases `docs/guide/workspaces.md`), and `inputs` (globs such as `packages/client/src/NewWorkspaceSheet.svelte`, `packages/client/src/FolderPicker.svelte`, `packages/client/e2e/video/**`). Teach `render.mjs` `--list` (one id per line) and `--list --json` (the metadata). Unknown `--case` ids exit non-zero with the list.
Done-when: the Check exits 0 and `node e2e/video/render.mjs --list` prints the four ids.
Progress: done 2026-10-29 on branch feat/docs-media-recorder; Check run with host node (no container), exits 0.

### 1.2 Input hash and `render.json` — [x]
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && node e2e/video/inputs-hash.mjs --self-test'`
Context: DECISION-0158: an approved video records a hash of its inputs (the case, its declared `inputs` globs, the app version) so a later change shows it may be stale. The app version is the server crate version in `packages/server/Cargo.toml` (read it as text; do not build).
Do: create `packages/client/e2e/video/inputs-hash.mjs` exporting `hashInputs(caseId)` (sha256 over the case's own source text in `cases.mjs`, the sorted contents of the files its globs match, and the version) and a `--self-test` that proves the hash changes when a matched file or the case text changes and not when an unmatched file does. `render.mjs` writes `<case>-<profile>.render.json` beside each video: `{case, profile, hash, version, run: process.env.GITHUB_RUN_ID ?? null, renderedAt, files}`.
Done-when: the Check passes and a local render of `recent-folder` leaves a `.render.json` whose `hash` matches `hashInputs`.
Progress: done 2026-10-29; self-test passes and a host render of recent-folder/phone left a render.json whose hash equals hashInputs (host node 24, vite builds, no container).

### 1.3 Pace and caption knobs — [x]‖
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm exec vitest run e2e/video/knobs.test.mjs'`
Context: the `/regen` command accepts `pace=slow|normal` and `caption=off` (DECISION-0158); `lib.mjs` `humanPage` holds the human-pacing constants (move steps 18 to 30, settle 120 to 350 ms, keys 55 to 140 ms, pauses 600 to 1200 ms).
Do: add `--pace slow|normal` (slow multiplies every delay by 1.6, default normal) and `--no-caption` (captions still drive the placement logic but are not drawn) to `render.mjs`, passed to `humanPage`; export the multiplier logic as a pure function with a test in `e2e/video/knobs.test.mjs` (normal leaves values unchanged, slow scales, an unknown value is rejected).
Done-when: the Check passes and `render.mjs --case recent-folder --profile phone --pace slow` produces a longer video than the default.
Progress: done 2026-10-29; knobs.test.mjs (vitest, picked up by `pnpm test`); recent-folder/phone is 19.6 s at normal and 26.9 s with --pace slow. --no-caption draws nothing but keeps the same timing.

### 1.4 Stills at marked steps — [x]‖
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && node e2e/video/render.mjs --case recent-folder --profile phone --out /tmp/stills && test $(ls /tmp/stills/recent-folder-phone-*.png | wc -l) -ge 2'`
Context: guide pages need a still next to the steps. The guardrail `ux-scenarios-assert-a-state-before-they-capture-it` applies: a still is taken only after its state was asserted. Videos record at CSS size via Playwright `recordVideo`; a still is `page.screenshot`.
Do: add `h.still(name)` to `humanPage` that waits for the caller's assertion to have passed (the case calls it right after an `expectThat(...)`; `still` throws if none ran since the last still), screenshots the page at DPR 2 into the output folder as `<case>-<profile>-<name>.png`, and records it in the `.render.json` `files`. Mark two steps in `recent-folder` (dialog open, folder chosen). A failed case writes no stills, like no video.
Done-when: the Check passes and a failing final assertion leaves no PNG.
Progress: done 2026-10-29; recent-folder/phone leaves two PNGs (dialog-open, folder-chosen) listed in render.json; stills are buffered and written only after the case passes; `expectThat` calls `noteAssertion()` and `h.still` throws without one. Stills are at the context's pixel ratio (phone 2, desktop 1), pointer and caption hidden.

### 2.1 Core promise cases: `add-server`, `resume-after-close` — [ ]‖
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && for c in add-server resume-after-close; do for p in phone desktop; do node e2e/video/render.mjs --case $c --profile $p --out /tmp/core || exit 1; test -s /tmp/core/$c-$p.mp4 || exit 1; done; done'`
Context: the add-server wizard is exercised by `packages/client/e2e/add-server.mjs` (read its scripted-server setup and the states it asserts); the unmerged branch `user-docs` has earlier attempts to port from: `git show user-docs:packages/client/e2e/video-connect.mjs` and `video-reconnect.mjs`. Resume means: workspaces are open, the page is closed or reloaded, the app reconnects and the workspace re-attaches with its screen (DECISION-0054, reconnect = re-attach).
Do: add the two cases to `cases.mjs` with metadata (group `core`, pages `docs/guide/install.md` and `docs/guide/workspaces.md`), each ending in an assertion of its final state (the server shown connected; the same workspace attached after the reload) and `still` calls at the wizard's pairing step and the re-attached screen. Captions in plain words, no real names, scripted data only.
Done-when: the Check exits 0 and each case's final assertion is observable (change the expectation locally and the case exits 1 with no video).

### 2.2 Daily use cases: `answer-decision`, `switch-workspaces`, `key-row-keyboard` — [ ]‖
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && for c in answer-decision switch-workspaces key-row-keyboard; do for p in phone desktop; do node e2e/video/render.mjs --case $c --profile $p --out /tmp/daily || exit 1; test -s /tmp/daily/$c-$p.mp4 || exit 1; done; done'`
Context: the push deep link to a decision is in `e2e/push-link.mjs` and `e2e/decision.mjs` (fixtures and states); workspace switching in `e2e/windows.mjs` and `e2e/appshell.mjs`; the soft keyboard is emulated by shrinking `visualViewport` (see how `e2e/newworkspace.mjs` resizes to 360 x 440); `user-docs` has `video-switch.mjs`, `video-type.mjs`, `video-mobile.mjs` to port. The soft-keyboard guardrail is `docs/guardrails/soft-keyboard-never-hides-input.md`.
Do: add the three cases (group `daily`, page `docs/guide/workspaces.md`), each ending in an assertion (the answer was sent with the chosen option; the other workspace is attached; the typed keys reached the terminal and the input stayed in view with the keyboard open). Stills at the decision card and the key row.
Done-when: the Check exits 0 and each case fails closed as in 2.1.

### 2.3 Agent cases: `agent-view`, `scrollback-search` — [ ]‖
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && for c in agent-view scrollback-search; do for p in phone desktop; do node e2e/video/render.mjs --case $c --profile $p --out /tmp/agent || exit 1; test -s /tmp/agent/$c-$p.mp4 || exit 1; done; done'`
Context: `e2e/acp-views.mjs` drives the real server binary and a fake ACP adapter for Terminal versus Agent view; `e2e/search.mjs` covers scrollback search. The scripted server (`app-lib.mjs`) may not speak ACP: first read `AcpView` fixtures in `src/AcpView.test.ts` and `acp-views.mjs`; if the Agent view cannot be shown from scripted messages, run that case against the real binary the way `e2e/newworkspace-real.mjs` does (`TERMENBOX_BIN`, a private `HOME`) and say so in the case's caption metadata and the PR.
Do: add both cases (group `agent`, page `docs/guide/workspaces.md`), final assertions on the visible state (the Agent view shows a turn and a tool call; the search hit is highlighted and the match count shown).
Done-when: the Check exits 0 and the metadata says whether each case uses the scripted or the real server.

### 2.4 Diagnostics cases: `unreachable-server`, `update-server` — [ ]‖
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh --full sh -c 'cd packages/client && pnpm run e2e:build && for c in unreachable-server update-server; do for p in phone desktop; do node e2e/video/render.mjs --case $c --profile $p --out /tmp/diag || exit 1; test -s /tmp/diag/$c-$p.mp4 || exit 1; done; done'`
Context: `e2e/failures.mjs` scripts the failed-connection states and the path report (what was tried and what to fix, DECISION-0123); the update flow uses `ServerMessage::UpdateStatus` (see `packages/core/src/protocol.rs`) and the Settings update control in the client.
Do: add both cases (group `diagnostics`, pages `docs/guide/networking.md` and `docs/guide/install.md`), final assertions on the visible fix hint and on the "up to date after restart" status. No real addresses: use documentation-range addresses.
Done-when: the Check exits 0 and no real address or name appears in captions or frames (grep the case source for IPs outside `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`).

### 3.1 Review page generator — [x]‖
Class: implement · Needs: 1.2 · Check: `node scripts/docs-media-review-page.mjs --self-test`
Context: DECISION-0158: one mobile-first static page plays every video of a render inline and shows case, profile, input hash and the "rendered walkthrough on scripted data" label. It is deployed as a Pages preview, so it must be plain HTML with no external script.
Do: create `scripts/docs-media-review-page.mjs` that takes a folder of `.mp4`, `.gif`, `.png` and `.render.json` files and writes `index.html` beside them: a card per video (`<video controls playsinline muted loop preload="metadata" poster=…>`), the stills, the metadata, a 16 px gutter, no horizontal scroll at 360 px, dark and light via `prefers-color-scheme`, and a `<meta name="robots" content="noindex">`. `--self-test` builds a page from fixtures and asserts the card count, the label and the absence of `<script src`.
Done-when: the Check passes and the page opened at 360 px shows no horizontal overflow (assert with the same Playwright harness in the self-test, or note that it was checked by hand).
Progress: done 2026-10-31; self-test passes (card per render, label, no script, escaped text). The 360 px overflow was not opened in a browser (no Playwright on this host): the layout is one column with overflow-wrap, check it on the first preview.

### 3.2 Render workflow `docs-media.yaml` — [~]
Class: implement · Needs: 1.3, 3.1 · Check: `python3 -c "import yaml; w=yaml.safe_load(open('.github/workflows/docs-media.yaml')); on=w.get(True) or w.get('on'); assert list(on.keys())==['workflow_dispatch'], on; s=open('.github/workflows/docs-media.yaml').read(); assert 'pull_request_target' not in s and 'wrangler pages deploy' in s; print('ok')"`
Context: `.github/workflows/dialog-videos.yaml` is the model (dispatch only, `case` and `profile` choice inputs passed through `env`, pinned action versions as in `ci.yaml`, ffmpeg, Playwright, `render.mjs`, artifact upload 30 days). `deploy-site.yaml` shows Wrangler and the secrets `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` and the project name `vars.TERMENBOX_SITE_PROJECT_NAME || 'termenbox-site'`. Guardrail `media-commands-come-only-from-people-with-write-access` applies: dispatch needs write access by GitHub's own rules, no `pull_request_target`.
Do: create `.github/workflows/docs-media.yaml`: inputs `case` (all or one id from `render.mjs --list`, passed as free string validated against the list in the first step), `profile` (phone, desktop, both), `pace` (normal, slow), `caption` (on, off); render each selected case and profile; build the review page (3.1); upload the folder as artifact `docs-media-<run_id>` (30 days); deploy the folder to Pages with `wrangler pages deploy <dir> --project-name=… --branch=media-<run_id>` (a preview, not production); create or update the issue titled `Docs media review <run_id>` (label `docs-media`) with the preview URL, the `hashInputs` of each case and one checklist line per case and profile in the fixed form `- [ ] <case> <profile> <hash>`; permissions `contents: read`, `issues: write`. Never interpolate inputs into the script. Keep `dialog-videos.yaml` until 7.2.
Done-when: the Check prints `ok`; after the pull request merges, `gh workflow run docs-media.yaml -f case=recent-folder -f profile=phone` yields an issue with the preview link and one line, and the preview plays the video on an iPhone.
Progress: file written 2026-10-31 and the Check prints ok; it can only be dispatched after the merge (FACT-0067), then run `gh workflow run docs-media.yaml -f case=recent-folder -f profile=phone`.

### 3.3 Review logic with tests — [x]‖
Class: implement · Needs: 1.2 · Check: `node --test scripts/docs-media-review.test.mjs`
Context: guardrails `media-commands-come-only-from-people-with-write-access` and `nothing-is-published-without-a-matching-approval-record`; the issue ledger format from 3.2 (`- [ ] <case> <profile> <hash>`); commands `/regen <case> [pace=slow|normal] [profile=phone|desktop] [caption=off] <note>` and `/approve <case|all>`. GitHub-facing code is kept thin so this logic runs without GitHub.
Do: create `scripts/docs-media-review.mjs` exporting `parseCommand(text, catalogIds)` (fixed grammar; unknown case, unknown knob, extra lines, shell metacharacters in the note are data, never executed), `diffChecklist(before, after)` (newly ticked and newly unticked lines), `mayAct(permission)` (true only for write, maintain, admin). Write `scripts/docs-media-review.test.mjs` for each, including a hostile note and a case id not in the catalog.
Done-when: the Check passes.
Progress: done 2026-10-31; `node --test scripts/docs-media-review.test.mjs` passes 10 tests, including a hostile note, an unknown case, a hash that differs from the ticked one, and a refused approval that copies nothing. Entry points are in `scripts/docs-media-review-cli.mjs` (parse, ticked, approve, issue-body).

### 3.4 Review workflow `docs-media-review.yaml` — [~]
Class: implement · Needs: 3.2, 3.3 · Check: `python3 -c "import yaml; s=open('.github/workflows/docs-media-review.yaml').read(); w=yaml.safe_load(s); on=w.get(True) or w.get('on'); assert set(on.keys())=={'issues','issue_comment'}, on; assert 'pull_request_target' not in s and 'collaborators' in s; print('ok')"`
Context: FACT-0067: these events run the default branch's file with the repo token for anyone who can comment, so the actor's permission is checked first (`gh api repos/$GITHUB_REPOSITORY/collaborators/$ACTOR/permission`). Only issues labelled `docs-media` are handled. A tick copies the render artifact's files into `docs/media/` on a branch `docs/media-<run_id>` and comments the compare link; regeneration dispatches `docs-media.yaml` with the knobs and unticks the line; the note is posted back and the issue gets the label `video-needs-edit`.
Do: create the workflow with `on: issues: types: [edited]` and `issue_comment: types: [created]`, `permissions: contents: write, issues: write, actions: write`, a first step that exits quietly unless `mayAct`, the comment or body passed only through `env`, and steps that call `scripts/docs-media-review.mjs` entry points (`approve`, `regen`) and push the branch with a bot identity; it never pushes to `develop`.
Done-when: the Check prints `ok`; after the merge a tick by a developer pushes `docs/media-<run_id>` and comments its compare link, a tick by a non-collaborator account does nothing, and `/regen recent-folder pace=slow too fast` re-renders slow and labels the issue.
Progress: file written 2026-10-31 and the Check prints ok; untried until it is on `develop`.

### 4.1 Pick lands in `docs/media` — [x]
Class: implement · Needs: 1.2 · Check: `node --test scripts/docs-media-review.test.mjs`
Context: DECISION-0158 (revised 2026-10-31: no orphan branch, manifest or site deploy change). Picked MP4s, GIFs, stills and each `render.json` live under `docs/media/` and enter `develop` in an ordinary squashed pull request.
Do: `approve` in `scripts/docs-media-review-cli.mjs` copies a ticked render into the folder and fails when the render's hash differs from the ticked one; the review workflow (3.4) pushes the branch.
Done-when: the Check passes, including a refused hash and a path outside the folder.
Progress: done 2026-10-31.

### 4.2 `deploy-site` — [>]
Moved: no change to `deploy-site.yaml` or `scripts/check-site.mjs`. Superseded by 4.1 (the media is in the repo, not fetched at deploy time).

### 4.3 Embed markers and `docs-media.mjs` — [~]
Class: implement · Needs: 4.1 · Check: `node scripts/docs-media.mjs --self-test`
Context: DECISION-0158 and DECISION-0159: docs mark a place with `<!-- media:<id> -->`; the script fills a block from `docs/media/`. The README renders on GitHub, which plays no MP4 inline, so the block is a poster image (the GIF) linking to the video, with paths relative to the page.
Do: `scripts/docs-media.mjs` has `embed` (fills `<!-- media:<id> -->…<!-- /media -->`, relative to the page, plus a one-line caption that it is a rendered walkthrough on scripted data) and `check` (every marker names a video with its GIF and `render.json` in `docs/media/`; a stale input hash only warns). `--self-test` covers a missing id, a half-present video, both relative depths and an idempotent second `embed`.
Done-when: the Check passes and `check` is wired into `scripts/check-docs.mjs` (6.2).
Progress: script done and self-tested 2026-10-31; wiring `check` into `scripts/check-docs.mjs` waits for 6.2.

### 5.1 Staleness comment and re-render — [ ]
Class: implement · Needs: 1.2, 3.2, 4.1 · Check: `node scripts/docs-media-stale.mjs --self-test`
Context: DECISION-0158: a pull request that touches a video's inputs gets an advisory comment; a merge to `develop` re-renders only stale videos and opens a review issue. Pull requests from forks get a read-only token, so for them the result goes to the job summary instead of a comment. Never `pull_request_target`.
Do: create `scripts/docs-media-stale.mjs` that, given the `render.json` files under `docs/media/` and the current `hashInputs` of each case, prints the stale case ids as JSON (`--self-test` covers fresh, stale, never-approved and removed cases). Add `.github/workflows/docs-media-stale.yaml`: on `pull_request` with `paths` `packages/client/src/**`, `packages/client/e2e/**`, `packages/server/Cargo.toml`, post or update one sticky comment (`<!-- docs-media-stale -->`) and fail never; on `push` to `develop` with the same paths, dispatch `docs-media.yaml` for the stale cases.
Done-when: the Check passes; after the merge a pull request touching `NewWorkspaceSheet.svelte` gets a comment naming the dialog cases once they are in `docs/media/`.

### 6.1 Generated CLI reference — [ ]‖
Class: implement · Needs: none · Check: `scripts/dev.sh --full sh -c 'cargo build -p termenbox-server && node scripts/gen-cli-reference.mjs --check --bin target/debug/termenbox-server'`
Context: `packages/server/src/main.rs` uses clap derive with `#[arg(long, env = "TERMENBOX_…")]` on 26 options, so `termenbox-server --help` lists each flag with its environment variable and default; `docs/guide/configuration.md` has a hand-written table `| Variable | Description |`.
Do: create `scripts/gen-cli-reference.mjs` that runs the binary with `--help`, parses flags, env names, defaults and help text, and writes a table between `<!-- cli:start -->` and `<!-- cli:end -->` in `docs/guide/configuration.md` (add the markers; keep the prose that the help text lacks above the table). `--check` exits 1 with a diff when the committed text differs. Add a CI step to the `Rust (release build)` job in `ci.yaml` that runs `--check` against the built binary.
Done-when: the Check exits 0, changing a help string in `main.rs` makes it exit 1, and `gen-cli-reference.mjs` without `--check` rewrites the block.

### 6.2 Link, anchor and media-marker check — [ ]‖
Class: implement · Needs: none · Check: `node scripts/check-docs.mjs --self-test && node scripts/check-docs.mjs`
Context: DECISION-0159; `scripts/check-site.mjs` already checks the landing page; targets are `README.md`, `docs/guide/*.md`, `docs/BOOTSTRAP.md`, `SECURITY.md` and `packages/site/index.html`. GitHub anchors are the lower-cased heading with spaces as hyphens and punctuation dropped.
Do: create `scripts/check-docs.mjs`: resolve every relative link (file exists) and every `#anchor` (a heading with that slug in the target), report all errors, skip absolute URLs (no network), run `docs-media.mjs check` (4.3) when it exists, and run `check-site.mjs`. `--self-test` uses fixtures with a broken link, a broken anchor and a good file.
Done-when: the Check exits 0 on the repository as it is (fix any real broken link it finds, in this step) and exits 1 on a seeded broken anchor.

### 6.3 Guide-versus-code drift check — [ ]
Class: implement · Needs: 6.1 · Check: `node scripts/check-guide-drift.mjs --self-test && node scripts/check-guide-drift.mjs --bin target/debug/termenbox-server`
Context: DECISION-0159: each flag, environment variable and default the guide names must exist in the code. The environment variables and flags are in the `--help` output (6.1); other named things (ports such as 4443-4542, file names under the server's data directory) are looked up with `grep` in `packages/server/src`.
Do: create `scripts/check-guide-drift.mjs` that extracts `TERMENBOX_*` names, `--flags` and `4443-4542`-style ranges from `README.md` and `docs/guide/*.md`, looks each up in the help output or the source, and fails on a miss unless the name is in `docs/guide/drift-allowlist.json` (`[ {"name": "...", "reason": "..."} ]`, each with a reason). `--self-test` covers a present name, a missing name and an allowlisted name.
Done-when: the Check exits 0 on the repository (fix real drift it finds, or allowlist with a reason) and exits 1 when a flag in the guide is renamed.

### 6.4 CI wiring for the docs checks — [ ]
Class: implement · Needs: 6.2, 6.3 · Check: `python3 -c "import yaml; w=yaml.safe_load(open('.github/workflows/ci.yaml')); j=w['jobs']; assert any('docs' in k for k in j), list(j); print('ok')"`
Context: `ci.yaml` runs on pull requests and pushes to `master` and `develop`; jobs are named like `Client (typecheck, build)`; the aggregate job `CI result` lists the jobs it needs (add the new job there). The drift check needs the server binary: reuse the artifact of the `Rust (release build)` job or build the debug binary in the new job, whichever the existing jobs already share (read the file).
Do: add a node-only job `Docs checks` that runs `node scripts/check-docs.mjs` and `node scripts/check-guide-drift.mjs`, with path filters on `README.md`, `docs/guide/**`, `packages/site/**`, `packages/server/src/main.rs` and `scripts/**` implemented the way the file already filters (read it first), and add it to `CI result`.
Done-when: the Check prints `ok` and a seeded broken link in a test pull request turns `Docs checks` red.

### 7.1 First publication, developer review — [ ]
Class: review · Needs: 2.1, 2.2, 2.3, 2.4, 3.4, 4.3 · Check: `gh run list --workflow docs-media.yaml --limit 1 --json conclusion -q '.[0].conclusion'`
Context: needs the pull requests A to C merged. A developer reviews on an iPhone with the procedure `review-and-publish-docs-media`; their word decides, and nothing here ticks for them.
Do: dispatch `docs-media.yaml -f case=all -f profile=both`; open the review issue and give the reviewer the preview link; record their ticks and any `/regen` notes; apply the notes with the procedure; put the `<!-- media:<id> -->` markers in `README.md` and the guide pages first so the pick branch fills them; open the pull request from the compare link and merge it when green. File anything not done as roadmap steps under `docs/plans/roadmap/`.
Done-when: the Check prints `success`, `docs/media/recent-folder-phone.mp4` is on `develop`, the README and the guide pages show posters that link to playing videos, and the reviewer has replied with the result.

### 7.2 Retire the old paths and adopt the recorder — [ ]
Class: implement · Needs: 7.1 · Check: `! test -e .github/workflows/dialog-videos.yaml && ! git ls-remote --exit-code --heads origin user-docs >/dev/null 2>&1 && grep -q 'e2e/video' docs/plans/roadmap/R73.md`
Context: DECISION-0158 retires the `user-docs` branch (its five scenarios were ported in 2.x) and `dialog-videos.yaml` (replaced by `docs-media.yaml`); R73 adopts the recorder. The `user-docs` branch is checked out as a registered worktree at `origin/develop` inside the main checkout, with staged deletions: look at `git worktree list` and `git -C origin/develop status` first, and do not delete work that is not on `develop`. Deleting a remote branch and a worktree is destructive: confirm with the owner.
Do: with the owner's go-ahead, remove the worktree and delete `user-docs` (local and remote); delete `.github/workflows/dialog-videos.yaml`; in `docs/plans/roadmap/R73.md` Notes and `docs/plans/ux-assessment-harness.md` step 6.x say the video renderer is `e2e/video` (DECISION-0158); run `go-getter capture` for new decisions and facts; mark R79 done and archive it (`docs/plans/roadmap/archive/`) per the tracker's rules, then regenerate the order table with `python3 docs/plans/roadmap/rank.py`.
Done-when: the Check exits 0 and `python3 docs/plans/roadmap/check.py` passes.
