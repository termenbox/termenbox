---
id: nothing-is-published-without-a-matching-approval-record
title: A video or still is published only if its manifest entry names an approver and the hash of the file being published
status: active
governed-by: 0158-documentation-media-comes-from-one-recorder-is-approved-by-the-owner-and-ships-from-a-docs-media-branch
grounded-in: [0158-documentation-media-comes-from-one-recorder-is-approved-by-the-owner-and-ships-from-a-docs-media-branch, 0066, ux-scenarios-assert-a-state-before-they-capture-it]
derivation-note: The point of the review ledger is that a person saw what ships; a file that changed after the tick, or arrived without one, would put unreviewed media on the public site.
tags: [ci, docs, video, site]
---

# Guardrail: No approval record, no publication

## Rule
`deploy-site` **MUST** fail, and **MUST NOT** copy a file into `packages/site/media`, when any file on `docs-media` has no entry in `manifest.json` with an `approver`, a `date` and a `sha256` equal to the file's own, or when an entry names a file that is absent.
The review workflow **MUST** write a manifest entry only from a tick or `/approve` by a person with write access (`media-commands-come-only-from-people-with-write-access`), computed from the artifact of the render the ticked line points to, and **MUST** drop the entry when the video is regenerated.
Every published video and still **MUST** carry the label that it is a rendered walkthrough on scripted data.
