---
id: 0067
title: A dispatch-only workflow cannot be run until its file is on the default branch, and issue and comment workflows run the default branch's copy with a write token
status: active
kind: observation
governed-by: 0158-documentation-media-comes-from-one-recorder-is-picked-by-a-developer-and-lands-in-docs-media-through-a-pull-request
tags: [ci, github, security]
date: 2026-10-29
last-verified: 2026-10-29
---

# What GitHub Actions does with dispatch and comment triggers

## What is true

- **Dispatch needs the file on `develop`.** `gh workflow run dialog-videos.yaml --ref feat/new-workspace-dialog-ux` failed with `HTTP 404: workflow dialog-videos.yaml not found on the default branch` while the file existed only on the pushed branch (2026-10-29). It ran after the file reached `develop` (run 40786291939). A new dispatch-only workflow therefore cannot be tried from its own branch; it ships first, then runs.
- **`issue_comment` and `issues` events run the default branch's workflow file** with the repository token (GitHub's documented behavior for these events), and on a public repository anyone who can comment can cause them to fire. Whatever a comment says is untrusted input, and the actor's repository permission is the only authority a workflow can check.
- **Pull request workflows do not start on a conflicting PR** (PR #360 showed no checks until `develop` was merged in).
