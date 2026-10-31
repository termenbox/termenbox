---
id: 0150-all-work-happens-in-a-pool-worktree
title: All work happens in a worktree from a fixed pool, never in the main checkout
status: draft
date: 2026-10-31
track: process
tags: [harness, git, agents, worktrees]
amends: [0027-parallel-isolation, 0028-worktree-location]
related: [0047-default-branch]
---

## Decision

- **The main checkout is not a place to work.** No agent edits, builds, commits or switches branches in it. It stays as the anchor of the repository: a place to read from and to run `scripts/pool.sh` from.
- **Every task, parallel or not, runs in a slot of one pool** at `../termenbox-wt/pool-01` .. `pool-10`, next to the repository (0028).
- **The pool is limited**: at most 10 slots (`POOL_MAX`), of which 3 are kept warm (`POOL_WARM`). A warm slot is a free, detached checkout of `origin/develop` that is already created and current, so starting a task takes seconds. A cold slot is a number with no directory. When the pool is full, a new task waits: it is refused with a message, never given a slot that is in use.
- **A slot is claimed by a branch.** `scripts/pool.sh acquire <type>/<slug>` takes a warm slot (or creates one under the cap), starts the branch from `origin/develop` and records the claim in the git dir. `release` returns it, and refuses while work is uncommitted or exists only in the slot, unless `--force`. Free slots beyond the warm count are removed. A worktree made by hand in the pool directory is never handed out or refreshed until it is adopted.
- **It is enforced, not asked for.** A project `PreToolUse` hook (`.claude/hooks/worktree-only.py`) refuses file edits inside a main checkout and Bash commands there that are not read-only; it allows the one-step entry into a worktree (`cd <slot> && ...`, `git -C <slot> ...`). A main checkout is a working tree whose git dir is the common git dir, so the rule follows the repository wherever it is cloned. Slots free themselves: `pool.sh reap` releases every clean slot whose branch is merged (remote branch gone after a squash merge, contained in `develop`, or its pull request merged) and deletes the local branch; `.claude/hooks/pool-reap.py` runs it at session start and after `gh pr merge`, and `acquire` and `warm` run it too. Slots with uncommitted work are never reaped. The hook is opt-in per environment: it enforces only when `TERMENBOX_POOL_ONLY=1` is set (shell profile), so other clones and contributors keep a usable main checkout.
- **`go-getter worktree new` stays for throwaway checkouts** outside the pool (a bisect, a review of another branch). It does not take a pool slot and does not count against the cap.

## Why

One working tree for every task meant tasks overwrote each other, a branch checked out in the main folder blocked the same branch elsewhere, and a session could commit to whatever was checked out. 0027 asked for a worktree per parallel task and was advisory; a rule that only some tasks follow gives no isolation. A cap stops worktrees from piling up with their build output (the Rust target and node_modules of ten checkouts is a lot of disk), and keeping three warm keeps the cost of following the rule near zero. Putting the check in a hook is the only form that holds for an agent that was never told the rule.

## Limits

- The hook sees the session's directory and the command text. A command that enters a worktree and then writes into the main checkout through an absolute path is not caught. The hook closes the accidental path, not a determined one.
- It guards the agent's tools. A human at a terminal can still work anywhere; the pool script and the guardrail are the shared convention.
- Warm means created and current. Installing dependencies is left to `POOL_WARM_CMD`, because the build runs in the pinned container (0114) and the host may have no toolchain.
