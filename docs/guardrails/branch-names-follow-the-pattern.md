---
id: branch-names-follow-the-pattern
title: Branch names must match the project pattern
status: active
date: 2026-10-11
tags: [practice-packs, git-hooks, guardrail]
governed-by: 0045-branch-names
grounded-in: [0045-branch-names]
derivation-note: Given a change is one branch and one pull request, the branch name identifies the change, so a name that does not match the pattern cannot be traced to its type or slug.
go-getter:
  enforcement:
    - tier: 3
      check: The current branch, or the pull request head in CI, matches the pattern
      run: builtin:branch-name pattern="^(feat|fix|docs|chore|ci|test|refactor|perf|build|style|revert)/[a-z0-9][a-z0-9._-]*$" allow="main,master,develop"
    - tier: 1
      check: Name a new branch `type/slug` before the first commit
  generated-by: git-workflow@0.1.1
  pack-answer: branch-pattern
  pack-option: ^(feat|fix|docs|chore|ci|test|refactor|perf|build|style|revert)/[a-z0-9][a-z0-9._-]*$
---

## Guardrail

A branch is named `^(feat|fix|docs|chore|ci|test|refactor|perf|build|style|revert)/[a-z0-9][a-z0-9._-]*$` (by default `type/slug`: a Conventional Commit type, a slash, and the change id). The default branch is exempt here and protected by its own rule.
