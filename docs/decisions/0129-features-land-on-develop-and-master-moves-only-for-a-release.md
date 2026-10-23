---
id: 0129-features-land-on-develop-and-master-moves-only-for-a-release
title: Features land on develop, and master moves only by fast-forward for a release
status: active
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [repo, release, ci, git-hooks]
date: 2026-10-23
---

## Decision

- **`develop` is the default branch.** Every feature, fix and docs pull request targets `develop`, one squashed pull request per step, green CI (guardrail `changes-land-as-one-squashed-pull-request`). Nightly builds come from `develop`.
- **`master` is the release branch.** It moves only when `develop` is fast-forwarded into it for a release; stable and beta tags are cut from it. Nobody merges a feature pull request into it.
- **Rulesets enforce it, not a workflow.** `master-guard` blocks deletion and force-push, restricts updates for everyone but the release managers (the bypass list) and requires a pull request. `develop-guard` blocks deletion and force-push and requires a pull request and the CI jobs as status checks. GitHub cannot refuse to open a pull request against a branch, so the rule bites at merge time.
- **`develop` is the default branch for the hooks too.** The "no work on the default branch" hook and CI now protect `develop`; `master` is exempt from the branch-name pattern like `develop`, because a release is a fast-forward of an existing ref.
- **CI runs on pull requests to `develop` and `master`**, and on pushes to both, so a required check can pass on either.

Supersedes nothing: decision 0047 (the default branch takes no direct work) still holds, with `develop` as the default branch.

## Why

Nightly, beta and stable channels (R66-R70) need a branch that moves daily and one that moves only on purpose. A workflow that closes pull requests after they open is brittle: it needs a token, runs after the fact, and blocks its own fix. A ruleset needs no code and is free on a public repository.

Not chosen: keeping `master` as the default branch with a `develop` convention (new pull requests would keep targeting the release branch); a closing workflow (PR #268, dropped in review); a hook alone (not enforced on GitHub).

## Consequences

- Deploys that trigger on a push to `master` (Cloudflare Pages, site, Docker image) now fire at release, not on each merge. R69 moves them to channel workflows; until it lands the hosted client updates only when `master` moves.
- Branches already based on `master` (docs-only and stacked work) are rebased onto `develop` before they merge.
