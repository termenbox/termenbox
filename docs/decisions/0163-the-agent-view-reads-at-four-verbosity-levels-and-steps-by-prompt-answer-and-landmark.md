---
id: 0163-the-agent-view-reads-at-four-verbosity-levels-and-steps-by-prompt-answer-and-landmark
title: The Agent view reads at four verbosity levels, switchable on the fly, and steps by prompt, answer or landmark
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, acp, ux, navigation]
related: [0111, 0137, 0149, 0156, 0162, 0164, 0181, 0168]
---

## Decision

- **Four levels.** **Full**: every entry, as today. **Compact**: thinking, finished tool runs and read-only searches fold into one-line groups ("12 tool calls, 3 files read"); diffs, errors, plans, permissions and the agent's text stay. **Focus**: the current tool or shell command (live, with elapsed time), the agent's decisions and messages, the user's prompts, and open questions. **Outline**: the user's prompts and each turn's final answer, plus anything that needs the user (an open permission or question, a failure, a stopped agent), which no level hides.
- **Levels are a view over the same entries.** They hide and group; they never drop data, change the stream or touch the server. Switching keeps the reader's place: the turn at the top stays at the top.
- **Unknown stays visible.** Grouping uses the entry's tool kind through the agent's quirk profile (0111). Agents report kinds unevenly (todos arrive as `other` on some, `plan` is rare; fact 0040), so a tool whose kind is unknown or `other` is never folded as "read-only"; it folds only as a generic "tool call".
- **Scope.** A device-wide default (phone and desktop may differ) and a per-tab override, in this device's storage next to the draft and place (`acpStore.ts`). No server field and no wire message.
- **Switching.** The header offers a two-state **Quiet / Detail** quick toggle (Quiet = Compact, Detail = Full) in one tap, and the full four-level menu behind a long press or the segmented control on desktop. Tapping a folded group peeks it open without changing the level; the peek resets when the level changes. **Auto** is an opt-in setting: Focus while the agent runs and the user is away from the tab, Full when a question opens or a failure happens; it never changes the level while the user is reading or touching the stream.
- **Orientation.** A sticky **current-prompt header** shows the prompt of the turn being read (tap to jump to its start); a "N new below" pill appears when the user is reading earlier text while the agent writes, and one tap returns to the live end.
- **Stepping.** Previous/next sit in the status strip (0164) and step between prompts and answers by default, with the turn counter showing the position. "Next error" and "next ask" shortcuts appear only while one exists ahead. Desktop keys: `J`/`K` and `[`/`]`, inactive in the composer. A jump puts the turn header at the top and highlights it briefly (none under reduced motion). Turns carry stable anchors that search (R78's scroll-to-turn) reuses.
- **Discoverability.** The first time a turn count exceeds a few, a one-time hint names the quick toggle and stepping; it can be dismissed and never returns.

## Why

The view shows every entry at full size, so a long run buries the decision under tool noise on a phone, and there is no way to move between prompts. A client-only view is instant and works the same for every agent; the two-state toggle keeps the common switch to one tap while the four levels remain available.

## Not chosen

- Cycling four states with one button (up to three taps), three levels (no table-of-contents view), per-type toggles (slow, no single quieter control), a server-synced level (new wire fields, and phone and desktop want different levels), a floating prev/next pair, and a persistent outline rail on phone.

## Consequences

- Turn grouping is a pure function of the entries, the level and the agent profile, tested without a DOM.
- Search (existing, in `AcpView`) unfolds a group that holds the current hit.
- The level engine must stay within the render budget of the performance step on a 2,000-entry stream.
