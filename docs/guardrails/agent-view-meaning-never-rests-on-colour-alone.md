---
id: agent-view-meaning-never-rests-on-colour-alone
title: Agent view roles use only the existing type scale and skin tokens, carry a glyph and a word, and pass the design-rule audit
status: active
date: 2026-11-01
governed-by: 0181
grounded-in: [0181, 0164, 0069, 0110]
derivation-note: The Agent view tells message types apart by colour and size at a glance, on phones in sunlight and for colour-blind users; the project already has measurable rules (DR1 to DR15), so the view must obey them, not restate them.
tags: [client, accessibility, ux]
---

# Guardrail: Agent view roles use only the existing type scale and skin tokens, carry a glyph and a word, and pass the design-rule audit

## Rule
**MUST** give every message role and status a glyph or word besides its colour and a text alternative for screen readers (DR9, DR7). **MUST** use only colours that are skin tokens and sizes that are on the DR5 scale, with inputs at 16 px. **MUST** pass the UX audit rules DR1, DR2, DR4, DR8, DR12 and DR15 on phone and desktop for every level and every skin (`docs/plans/assets/ux/audit.mjs`). **MUST NOT** introduce a role colour or size outside those tokens and that scale, drop a role's glyph in a folded or Compact line, or add controls to the status strip beyond the seven per row of DR4.
