---
id: media-commands-come-only-from-people-with-write-access
title: A render, an approval or a regeneration is accepted only from a person with write access to the repository
status: active
governed-by: 0149-documentation-media-comes-from-one-recorder-is-approved-by-the-owner-and-ships-from-a-docs-media-branch
grounded-in: [0149-documentation-media-comes-from-one-recorder-is-approved-by-the-owner-and-ships-from-a-docs-media-branch, 0065, no-paid-services]
derivation-note: Comment and issue-edit workflows run the default branch's file with a write token and fire for anyone who can comment on a public repository (fact 0065), so the commenter's permission, not the comment text, is the only authority; an unchecked `/regen` would let a stranger spend runner minutes and an unchecked tick would publish to the site.
tags: [ci, github, security, docs, video]
---

# Guardrail: Media commands need write access

## Rule
The review workflow **MUST** read the actor's repository permission from the GitHub API on every `issues` edit and `issue_comment` event and **MUST** do nothing, and say nothing beyond a one-line refusal, unless the permission is `write`, `maintain` or `admin`.
It **MUST** treat the comment text and the issue body as data: parsed with a fixed grammar (`/regen <case> [pace=…] [profile=…] [caption=off] <note>`, `/approve <case|all>`), passed to the next step through `env`, never interpolated into a shell command, and the case id **MUST** be one of the catalog's ids.
The note **MUST NOT** be executed, fetched or logged outside the issue; the render workflow **MUST** be `workflow_dispatch` only with `permissions: contents: read` plus the one write it needs, and **MUST NOT** run on `pull_request_target`.
