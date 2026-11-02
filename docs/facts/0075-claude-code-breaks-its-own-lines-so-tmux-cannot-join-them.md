---
id: 0075
title: Claude Code word-wraps with explicit row ends, so neither xterm nor tmux capture-pane -J can rejoin its lines
status: active
tags: [client, terminal, copy, tmux]
date: 2026-11-02
kind: observed
last-verified: 2026-11-02
---

# Why copy needs a client heuristic

Measured with Claude Code 2.1.308 in a 60-column pane on tmux 3.8 (34 row pairs) (the spike for the copy-join fix, PR #501).

- A plain shell line longer than the pane, or one typed at a prompt, is wrapped by tmux: `capture-pane -p` gives several rows and `-J` joins them.
- Claude Code never prints a row wider than the pane. It word-wraps and ends each row itself, so xterm sets no `isWrapped` and `-J` joins nothing.
- Its rows stop 0 to 10 columns short of the edge; a continuation starts at the text column of the first row after its marker (`● `, `- `, `1. `); a long URL is cut at the full width.
- `-J` on tmux 3.8 keeps only typed trailing spaces; tmux 3.2a padded every line to the pane width. Rows carry no column map, and the client's scrollback is separate from tmux history.

The rule that copes with this is `packages/client/src/lib/terminal/copyJoin.ts`. Recheck it if Claude Code changes its wrapping.
