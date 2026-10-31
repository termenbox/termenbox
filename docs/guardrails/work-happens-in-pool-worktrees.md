---
id: work-happens-in-pool-worktrees
title: Work happens only in a pool worktree, never in the main checkout
status: draft
date: 2026-10-31
tags: [harness, git, worktrees, guardrail]
governed-by: 0150-all-work-happens-in-a-pool-worktree
grounded-in: [0150-all-work-happens-in-a-pool-worktree, 0027-parallel-isolation, 0028-worktree-location]
derivation-note: Given every task has its own worktree from a limited pool, an edit or a build in the main checkout breaks the isolation the pool exists for, and an instruction alone is not followed by an agent that never read it.
go-getter:
  enforcement:
    - tier: 2
      check: Where TERMENBOX_POOL_ONLY=1 is set, a PreToolUse hook refuses edits inside a main checkout and Bash commands there that are not read-only
      run: .claude/hooks/worktree-only.py
    - tier: 1
      check: Start a task with `sh scripts/pool.sh acquire <type>/<slug>` and work in the path it prints; a slot whose branch is merged is released by itself (`pool.sh reap`, run at session start, after `gh pr merge` and on acquire)
---

## Guardrail

Never edit, build, commit or switch branches in the main checkout. A session started in the main checkout is given a slot at start (the hook tells you the path); name its branch with `sh scripts/pool.sh name <type>/<slug>` when the task is clear. Otherwise take a slot with `sh scripts/pool.sh acquire <type>/<slug>` and work in it (`cd <slot> && ...`, or start the session there). The pool has 10 slots and keeps 3 warm; when it is full, finished slots are released automatically once merged; release an abandoned one with `sh scripts/pool.sh release <slot>` instead of working elsewhere. Reading in the main checkout is allowed. The hook enforces this per environment: it is off unless `TERMENBOX_POOL_ONLY=1` is set (shell profile or `.env`), so other clones are not locked; the rule itself holds everywhere. Tested by `scripts/worktree-guard.test.sh` and `scripts/pool.test.sh`.
