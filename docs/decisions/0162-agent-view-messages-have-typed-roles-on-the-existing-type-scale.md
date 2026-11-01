---
id: 0162-agent-view-messages-have-typed-roles-on-the-existing-type-scale
title: Every Agent view message has a typed role (colour token, glyph, weight, size step) on the existing type scale
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, acp, ux, accessibility]
related: [0069, 0110, 0159, 0160]
---

## Decision

- **A fixed set of roles:** your prompt, agent answer, thinking, tool by kind (read, edit, run, search, other), result, error, permission, plan, notice, plus the digest and the rule line (0161, 0164). Each role has a colour token from the active skin (`skins.css`; skins change tokens only, DR11), a glyph, a weight, a size step and a screen-reader word.
- **Sizes come from the existing scale** (DR5: 12, 13, 15, 17, 22, and 13 mono). Prompts and answers use 15 (17 in Focus and Outline, where there is less to read), tools and results use 13 mono, labels 12, group lines 13. Inputs stay 16. No new sizes.
- **One text-size control** scales the whole scale up or down in bounded steps per device. There is no per-role colour or size editing.
- **Meaning never rests on colour alone** (DR9) and text and glyphs pass 4.5:1 in every skin (DR8); both are checked in CI over the role tokens.
- **Unknown kinds get the neutral "tool" role**, never a guessed one.

## Why

The owner wants the type of a message readable at a glance. Roles make that systematic. Using the project's scale and skin rules keeps this view consistent with the rest of the app and with the audit that already checks it.

## Not chosen

- User-editable role colours (a settings surface; users can break contrast), new sizes (breaks DR5), and tuning today's styles case by case (not systematic).

## Consequences

- Adding an entry kind means adding its role (procedure `add-a-message-role-to-the-agent-view`).
