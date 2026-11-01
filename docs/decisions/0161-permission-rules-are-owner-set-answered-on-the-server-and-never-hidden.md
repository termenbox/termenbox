---
id: 0161-permission-rules-are-owner-set-answered-on-the-server-and-never-hidden
title: Permission rules are set by the owner, answered on the server, scoped and revocable, and never hidden
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, acp, security, server, automation]
related: [0104, 0111, 0143, 0160, 0162, 0163]
---

## Decision

- **"Always allow this" on a permission card creates a rule**, after a confirmation that shows the exact scope. A rule is a workspace, a tool kind, an optional pattern, an optional expiry, and who made it and when. The owner sees, edits and revokes rules in a Rules sheet (the inspector's Rules tab on desktop).
- **The agent's own session modes come first.** The card and the strip show the agent's modes (`info.modes`, for example "accept edits") as the cheapest automation; a rule covers what a mode does not.
- **Presets before patterns.** Rule creation offers presets ("reads inside this workspace", "edits inside this workspace", "this exact command") before a custom pattern.
- **Suggestions with a dry run.** After the user approves the same kind of ask three times, the card offers "make this a rule?" with a preview: how many of the last 50 asks of this workspace it would have answered, and which. The preview is computed by the server against the same matcher; nothing is created until the owner confirms.
- **The server answers.** A matching permission is answered by the server, so it works with no device attached. Rules are stored next to the server's other state, atomically.
- **Matching is on parsed arguments, not text.** A shell command is parsed into arguments; a pattern matches the program and its arguments. A command with a compound or indirect form (`&&`, `||`, `;`, pipes, redirects, command substitution, `eval`, an unparsable string) never matches a pattern rule, and goes to the user. A rule that names one exact full command string matches only that string.
- **Safe by default.** `delete`, anything leaving the workspace directory, and network fetches never auto-allow from a broad rule; they need a rule that names the case exactly. No rule is created without the owner's tap, and a rule never widens an agent's own sandbox.
- **Never hidden, and easy to undo.** An auto-answered request stays in the log as a marked line ("allowed by rule: …") and in the audit record, without output or secrets. The marked line offers Revoke for the first five minutes after creation and always through the Rules sheet. One tap turns all rules off.
- **A risk summary on every card.** From the tool call's locations, diffs and kind: files changed, lines added and removed, whether any path is outside the workspace, whether it reaches the network. It shows as one line above the options, with a warning mark when it leaves the workspace.
- **Batch.** Identical open asks across workspaces can be answered together from the inbox, never across different kinds or workspaces without showing each.

## Why

Approving the same safe command all day is the most repeated manual step in agent use. Modes, presets, suggestions and rules remove it step by step; the preview, the parsed matching and the marked lines keep it visible and safe.

## Not chosen

- Session-only allow (nothing carries across a restart), manual-only with faster cards (no automation), text-prefix matching (a prefix rule for `git status` would allow `git status && rm -rf x`), and silent auto-answers.

## Consequences

- New wire messages for rules (list, add, revoke, preview) and an optional "by rule" field on the permission entry, both optional so old clients keep working.
- Guardrail `auto-answered-permissions-are-scoped-visible-and-audited` states the limits as rules.
