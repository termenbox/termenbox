---
id: 0146-a-workspace-id-is-minted-by-the-server-and-the-name-is-only-a-label
title: A workspace id is minted by the server, and the name the owner types is only a label
status: active
date: 2026-10-28
accepted-by: Sergey Strebulaev <s@vivantel.dev>
track: product
tags: [server, privacy, observability, workspaces]
fitness-functions: [a tmux error, a panic and a Debug print of a workspace contain the label of no workspace, a minted id survives the bundle scrub, a label survives a restart and a rename on the ACP backend]
---

## Decision

- **Id.** A new tmux or ACP workspace gets the id `ws-<32 hex>` from the server. It is the tmux session name, the key of the layout and event state, and what clients attach by. The name typed in "New workspace" becomes the label (`@termenbox_name` for tmux, a field of the saved state for ACP) and nothing else.
- **Label.** The label is the `Label` type of `termenbox-core`: a plain string on the wire, `[name]` under `Debug` and `Display`. Reading the text takes `as_str()`.
- **Sync.** The server is the one place labels live. A rename, create or close pushes the list to every connected device; a device keeps ids, never names. Two workspaces may carry the same label.
- **Older workspaces.** A workspace made before this keeps the name as its id: ids are not renamed in place, because the event variables in its panes carry the old id. The user-configured command backend keeps passing the typed name to its own tool.
- **Bundle.** A minted id may appear in a diagnostics bundle (it matches a crash to a workspace); any other value in an id field is still omitted, and the live non-minted ids and labels are scrubbed from free text.

## Why

The scrubber of fact 0062 can only find a name by its field or its shape, and a name is neither. A tmux error such as `can't find session: <name>` repeated the typed name because the name was the session. With a random id there is nothing typed to repeat, and the `Label` type stops the one remaining route, a stray `{:?}` of a `WorkspaceInfo`.

## Rejected

| Option | Why not |
|---|---|
| Keep scrubbing live names only | Misses a name that is gone by the time the bundle is built, and every free-text wording. Kept as defence for older workspaces. |
| Rename existing tmux sessions to ids | The panes' `TERMENBOX_SESSION` and event socket variables keep the old id; agent events would stop matching. |
| Unique labels | Forces a "taken" error on a name that does not need to be unique now; the picker can show a suffix when two collide. |

## Open

Rename conflict between two devices is last write wins. A `from` field on `rename_workspace` (reject when the label changed meanwhile) was designed and not built.
