---
id: 0160-the-agent-view-layout-has-a-status-strip-everywhere-an-inspector-on-desktop-and-a-chrome-budget
title: The Agent view has a status strip everywhere, an inspector on desktop and sheets on phone, inside the existing chrome budget
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, acp, ux, mobile, desktop]
related: [0101, 0104, 0159, 0161, 0162, 0164]
---

## Decision

- **A status strip on every device**, sticky at the top of the view. One row of at most seven controls (DR4): the agent state word and glyph; the current tool or command with elapsed time; plan progress when the agent sends a plan (nothing, not an empty bar, when it does not); context use and cost when the agent sends usage (`usage` in the stream); the turn counter "7/12" with previous/next (0159); Stop. Tapping the turn counter opens the minimap. On a narrow phone the row wraps to two lines at most and the tool line is the one that truncates.
- **Desktop uses width.** At wide breakpoints a right **inspector** with tabs: **Outline** (turns), **Plan** (hidden when no plan arrives), **Changes** (every diff of the session, grouped by file), **Rules** (0161). The main column stays about 80 characters wide. The pane's open state is kept per device.
- **Phone uses sheets.** The same four panels open as bottom sheets from the strip. The composer docks above the keyboard with quick replies and the queued-prompt chip (0164). No floating overlay sits over the content.
- **Chrome budget.** With the keyboard closed, everything that is not the stream (tab bar, header, strip, composer) is at most 20% of the screen height (DR12); with the keyboard open the stream gets at least 50% of the area above it. The assessment fails a change that exceeds it. The strip collapses to one line while the user scrolls the stream and returns on scroll up.
- **Rule set.** The strip and inspector follow DR1 to DR4, DR7 and DR13: 44 pt targets, labelled controls, one primary action per state (Stop while running, the answer while a question is open), primary actions in the lower two thirds.

## Why

The owner wants the most of both screens and an at-a-glance answer to "what is the agent doing now". A strip makes that one place; the inspector uses desktop width; the budget stops the extra controls from eating the phone.

## Not chosen

- One column everywhere (wastes desktop), a multi-tab split workbench (a large build that overlaps R75), a floating prev/next pair (covers the stream and the thumb zone), and a persistent outline rail on phone (width).

## Consequences

- The first plan step measures today's chrome share on a phone; the strip is designed inside the remainder.
- The inspector's Rules tab appears only when R80 has landed. Until then the Rules sheet is not shown.
