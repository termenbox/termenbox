# Session UX round 2 (branch `fix/session-ux-round2`, one PR to `develop`)

Worktree: `.claude/worktrees/ux-round2`. Build and test only in the dev container, with a private target dir because the
shared one is owned by another uid:
`CARGO_TARGET_DIR=/cache/target-ubuntu-r2 scripts/dev.sh sh -c 'CARGO_TARGET_DIR=/cache/target-ubuntu-r2 cargo test -p termenbox-server'`
and `scripts/dev.sh --full sh -c 'cd packages/client && pnpm exec svelte-check --threshold error; pnpm exec vitest run'`.
Commit rules: `type(scope): summary`, body `Why:` then `What:`, no attribution lines. Do not merge; ask first.

## Done (committed)

1. `099aea2` status routing: new `owner.rs`. An event is attributed only to a session whose panes the reporter runs under
   (hooks send parent pids). A shared OpenCode service is placed by `-s <id>`, by the prompt on one screen, or by a sole TUI,
   else the event waits 30 s and is dropped. A turn belongs to the conversation that opened it. The plugin names the
   conversation on every frame and is refreshed at start. The scan reads the pane the session shows. Root cause on this
   machine: four TUIs in three tmux sessions share one `opencode serve` whose env says `session-opencode`.
2. `ec1a32e` tmux config reloaded into a running tmux server at start (`status off` was ignored by the old server).
3. `2763961` a rename, create or close pings every other connection; the client shows a new name at once.
4. `bf348c4` Activity asked for ~83 KB of history; the direct transport carries 64 KiB, so the write failed and the
   connection dropped (a WebSocket has no limit, hence Android was fine). History is cut to 48 KiB and an oversized reply
   becomes an error, not a disconnect. Proven by an e2e test over both transports.
5. `e44a994` Appearance is a Settings page (no Settings → Appearance → Settings circle); no server version beside the login.
6. `1841735` servers page: fixed-height rows, three-dot menu with icons, error icon with popover, session count.

## Done (this session, committed)

7. Ctrl+A / Ctrl+X / wheel (`scrollback` request, `capture_history`, `focusTarget.ts`, select view `all`). Server suite,
   svelte-check and vitest pass; a real-tmux test proves lines above the screen come back.
8. A repeat tap on Sessions in the bottom bar brings the list back (sheet on a phone, sidebar when wide); only the bar
   does this, not the sidebar row. `NavigationProbe` renders the real SettingsView so `cards.mjs` passes again.

9. opencode rebind: `--connect-opencode` also writes `input_select_all: ctrl+a` and `input_line_home: home` to `tui.json`
   (previewed, backed up, journaled, undone by `--disconnect-opencode`). Checked in a scratch tmux on opencode 2.1.2
   `--standalone`: the config file is `tui.json`, key `keybinds`.

## Before the PR

- Run the browser e2e (`pnpm e2e:build` then the probes) for the steps whose selectors changed: `e2e/run.mjs` (server menu,
  no "← Sessions"), `e2e/cards.mjs` (Appearance page).
- PR body: Why, What, Checks, Not verified. Not verified: no real iPhone or Android; the live opencode service keeps the old
  plugin until `opencode serve` restarts; the click-above-prompt rule is a heuristic (`PROMPT_ROWS = 8`).
- After release, update this server (user-mode updater: `termenbox-server --root-helper update --user-helper` with the
  unit's environment) and restart opencode so the new plugin loads.
