# Copying a selection joins wrapped rows instead of splitting at the visible rows

Bug: copying a selection from the terminal gives the text split at every visible row, not at the real line ends in the text. This plan removes itself in its last step.

Read first: `CLAUDE.md`, `packages/client/src/Terminal.svelte` (`copy`, `copySelection`, the Ctrl+C and cut handlers near line 580, `openSelect`), `packages/client/src/lib/terminal/touchSelection.ts` (`text()`), `packages/client/src/lib/terminal/links.ts` (`linkAt`: the existing rule that rejoins rows a program hard-wrapped), `packages/client/src/lib/keys.ts` (`bufferText`, used by `SelectView.svelte`), `packages/server/src/backend.rs` (the tmux `capture-pane` calls near lines 1240-1380). Repo: Termenbox (Rust server `packages/server`, Svelte 5 client `packages/client`). Work only in a pool worktree; this plan lives in `/home/sergei/projects/termenbox/termenbox-wt/pool-09` on branch `fix/copy-joins-wrapped-lines`. One squashed PR off `develop`, commit subject `type(scope): summary` (one-word scope), body `Why:` then `What:`, no Co-Authored-By (use the `attribute` skill). No compiler on some machines: prefix commands with `scripts/dev.sh`.

Cause. The copied text comes from xterm's `getSelection()`, which joins two rows only when the second carries `isWrapped`. Programs in tmux (Claude Code) and tmux itself repaint each row with cursor moves, so the marks are missing and every visible row ends in a newline in the copy.

Goal: a selection that spans rows of one logical line copies as one line; real line ends stay.

Legend: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked, `[>]` moved.

### 1.1 Claim a worktree and create the branch — [x]
Class: implement · Needs: none · Check: `git -C /home/sergei/projects/termenbox/termenbox-wt/pool-09 rev-parse --abbrev-ref HEAD`
Context: Work happens only in a pool worktree (GUARDRAIL-work-happens-in-pool-worktrees).
Do: `sh scripts/pool.sh acquire fix/copy-joins-wrapped-lines` from the main checkout.
Done-when: the command printed `/home/sergei/projects/termenbox/termenbox-wt/pool-09` and that worktree is on `fix/copy-joins-wrapped-lines`.

### 1.2 Spike: does `tmux capture-pane -J` join the rows? — [x]
Class: explore · Needs: 1.1 · Check: `grep -c "^Spike result" docs/plans/copy-joins-wrapped-lines.md`
Context: Option under test: ask tmux for the true joined lines (`capture-pane -J -p`) and map the selection onto them. It is exact for wraps tmux made itself; it cannot know about lines a program hard-wrapped and indented (Claude Code), and the server already runs `capture-pane` in `backend.rs`. Nothing is built in this step.
Do: in a scratch tmux session on a narrow pane, produce three cases: (a) a plain shell printing one 300-character line, (b) a line longer than the pane typed at a prompt, (c) Claude Code (or a script imitating its hard-wrapped, indented output) printing a long paragraph and a long URL. For each, run `tmux capture-pane -p` and `tmux capture-pane -J -p` and compare with the text printed. Also note whether `-J` keeps trailing spaces and how it maps to the client's buffer rows (the client holds scrollback separate from tmux history). Add a section `Spike result` to this plan with one line per case (joined / not joined) and a recommendation.
Done-when: this file has a line starting `Spike result` per case and a recommendation, and no source file changed.

Spike result (the pinned static tmux 3.8, DECISION-0119, 40-column pane; the system tmux 3.2a gave the same joins; nothing built):
- Spike result (a) shell prints one 100-character line: `-p` gives 3 rows, `-J` gives 1 joined line. Joined.
- Spike result (b) line typed at a prompt longer than the pane: `-p` gives 4 rows, `-J` gives 1 line. Joined.
- Spike result (c) program that hard-wraps and indents its own output (the rows end in real newlines, as Claude Code writes them) and a long URL: the hard-wrapped rows stay separate in both modes (they are real line ends to tmux), the long URL row is joined by `-J`. Not joined for program wraps; joined for tmux wraps.
- Spike result (d) `-J` keeps only the spaces that were really typed (`ab   ` stays, a joined line is not padded) on 3.8; 3.2a padded every line to the pane width. Rows carry no column map: mapping a selection (client scrollback rows) onto `-J` lines needs row offsets the client lacks, since the client buffer and tmux history are separate.

Recommendation: do not use `capture-pane -J`. It needs a new wire message and a row-to-line mapping, and it still cannot rejoin the Claude Code case, which is the reported one. Use a client heuristic in the `links.ts` style (a row joins the next when it is marked wrapped, or fills to within a few columns of the edge and the next row has the same indent), applied in one function for all copy paths.

### 1.3 Decide the join rule — [x]
Class: plan · Needs: 1.2 · Check: `git diff --stat origin/develop -- docs/decisions | tail -1`
Context: Candidates: server `capture-pane -J` (new protocol message and round trip), a client heuristic joining a row to the next when it is marked wrapped or fills to within a few columns of the edge and the next row has the same indent (the `links.ts` rule), or a combination. A new wire message changes `packages/core/src/protocol.rs` and its hand-written mirror `packages/client/src/lib/protocol.ts`.
Do: pick one from the spike result and put the choice and its reason in this plan under `Decision`. If the choice is hard to reverse, surprising or a real tradeoff, write `docs/decisions/NNNN-slug.md` (next number after the highest in `docs/decisions/`, format of its neighbours) and its row in `docs/decisions/INDEX.md`; otherwise keep it inline here.
Done-when: this file has a `Decision` section naming the rule, and the decision file exists if one was warranted.

Decision: a client heuristic, no server change, no decision file. A row joins the next when xterm marks the next row `isWrapped`, or when the row stops within `EDGE` (4) columns of the right edge and the next row has the same indent and continues (the `links.ts` `marked`/`alike` rule, without its one-word and open-URL limits, since prose also wraps). The joined text drops the continuation's indent and the cut row's trailing blanks; a row that stops short of the edge, a blank row, or a change of indent stays a real line end. Reason: `-J` cannot rejoin the reported Claude Code case (spike c) and needs a protocol message plus a row-to-line map; the heuristic covers both tmux wraps (marked or full-width rows) and program wraps, in one client function. Known tradeoff: a real line end on a full-width row with the next row at the same indent is joined; it is rare and the same call `linkAt` already makes. Reversible, so inline here only.

### 2.1 Implement the rule once — [ ]
Class: implement · Needs: 1.3 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm exec vitest run src/lib/terminal && pnpm run check'`
Context: Copy has several paths that must agree: Ctrl/Cmd+C and cut in the key handler of `Terminal.svelte`, `copy()` (the screen copy with no selection), `copySelection()` through `touchSelection.text()`, and Select view's "Copy all" through `bufferText` in `lib/keys.ts`. Deliver them from one function, placed next to `links.ts` in `packages/client/src/lib/terminal/`, so they cannot drift.
Do: write the function per the decision, cover it with tests in a sibling `*.test.ts` (wrapped-marked rows, tmux-repainted full rows, a real line end on a full-width row, blank lines, indented continuations, selection starting mid-line), and call it from every path above in place of the raw `getSelection()` / row join. If the decision needs the server, add the protocol message in `packages/core/src/protocol.rs`, its mirror in `packages/client/src/lib/protocol.ts`, the handler in `packages/server`, and a Rust test.
Done-when: no copy path in `Terminal.svelte`, `touchSelection.ts` or `lib/keys.ts` copies unjoined `getSelection()` text, and the check passes.

### 2.2 Whole-workspace checks — [ ]
Class: review · Needs: 2.1 · Check: `scripts/dev.sh sh -c 'cargo test --workspace && pnpm -r test && pnpm -r build'`
Context: CI runs the same, plus commit-subject and instruction-file checks.
Do: run the check, fix what the change broke, and look over the diff against `docs/guardrails/` once (the `conform` skill).
Done-when: the check passes and `git diff origin/develop --stat` shows only the intended files.

### 3.1 Capture the knowledge — [ ]
Class: implement · Needs: 2.2 · Check: `git diff origin/develop --name-only -- docs | grep -v plans`
Context: Facts and docs must follow the code (DECISION-0159). The user guide may describe copy.
Do: add or update a fact if the spike established a lasting truth (for example how tmux repaints wrapped lines), update `docs/guide` where it describes copying, and update each touched `INDEX.md`.
Done-when: every durable finding of the spike is in a fact or decision, or this step records that there was none.

### 4.1 Open the pull request — [ ]
Class: implement · Needs: 3.1 · Check: `gh pr view --json state,mergeStateStatus`
Context: One squashed PR targeting `develop`, with green CI (GUARDRAIL changes-land-as-one-squashed-pull-request). A bad subject cannot be amended once pushed.
Do: write the commit with the `attribute` skill: subject `fix(client): copy joins wrapped rows` (or the scope the change fits), body `Why:` the copy split at visible rows, `What:` the rule. Check the subject, push, open the PR to `develop`, and wait for CI.
Done-when: the PR is open against `develop` and CI is green.

### 5.1 Remove this plan — [ ]
Class: implement · Needs: 4.1 · Check: `test ! -e docs/plans/copy-joins-wrapped-lines.md`
Context: A plan is for one occasion; once the fix and its records are in, the file is clutter. Do it as the last commit of the same PR so the plan never lands on `develop`.
Do: `git rm docs/plans/copy-joins-wrapped-lines.md`, delete any row or link to it (search `docs/` for the file name), commit as `docs(plans): remove the copy-join plan`, push, and let CI go green again.
Done-when: the file and every reference to it are gone from the branch and CI is green.
