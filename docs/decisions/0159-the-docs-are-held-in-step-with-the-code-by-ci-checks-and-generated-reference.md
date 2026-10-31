---
id: 0159-the-docs-are-held-in-step-with-the-code-by-ci-checks-and-generated-reference
title: The README, guide and site are held in step with the code by CI checks and a reference generated from the binary, with no model in the loop
status: active
date: 2026-10-29
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [docs, ci, site]
---

## Decision

- **Generated CLI reference.** The flag and environment tables of `docs/guide/configuration.md` are produced from `termenbox-server --help` (clap metadata) between marker comments by a script; CI fails when the committed text differs from what the built binary prints.
- **Link and anchor check.** Every relative link and `#anchor` in `README.md`, `docs/guide/` and the site resolves, and every `<!-- media:<id> -->` marker names an approved video (decision 0158). It extends `scripts/check-site.mjs`, which already guards the site's install command against the README, and runs in a cheap node-only CI job.
- **Guide-versus-code drift check.** Each flag, environment variable and default the guide names is looked up in the source or the `--help` output; a renamed or removed one fails CI. A checked-in allowlist records the false positives, each with a reason.
- **Advisory staleness.** The input hash of an approved video (decision 0158) is compared on pull requests and the result is a comment, never a block.
- **No model.** These checks and the generator are plain scripts; an agent edits prose in a session when one of them fails.

## Why

The README, six guide pages and the landing page are written by hand and the only machine check is the site's install command. A flag renamed in the server, a dead link or a moved heading reaches users unnoticed, and the new videos would add one more thing to go stale. Mechanical checks are cheap, deterministic and free (`no-paid-services`), and a failing check tells the next session exactly which sentence to fix.

Not chosen: regenerating prose with a model in CI (key, cost, review burden); blocking pull requests on video staleness (a copy edit would block a UI fix).

## Consequences

- A change to a flag, a default or a heading in the server touches the generated tables and the guide in the same pull request.
- The drift check needs an allowlist that someone keeps small.
