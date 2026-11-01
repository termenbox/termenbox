---
id: add-a-message-role-to-the-agent-view
title: Add a message role to the Agent view
status: active
date: 2026-11-01
tags: [client, acp, ux, procedural]
---

# Procedure: Add a message role to the Agent view

Use when a new kind of entry reaches `AcpView` (a new ACP update, a new notice, a new tool kind). Decisions 0163 and 0164; guardrail `agent-view-meaning-never-rests-on-colour-alone`.

1. Add the entry kind to `Entry` in `packages/client/src/lib/acpStream.ts` and its reducer case; add a turn-summary rule so the turn list and Outline know it.
2. Add its role to the roles module (`packages/client/src/lib/acpRoles.ts`): a skin colour token, a glyph, a weight, a size step taken from the DR5 scale (decision 0166), and its screen-reader word. Add the token to every skin in `packages/client/src/skins.css`; the contrast test fails until each passes 4.5:1. Add the kind to the fixtures (`lib/agentViewFixtures.ts`) first, as in `build-an-agent-view-slice-template-first`.
3. Say how it behaves at each level: shown, folded into a Compact group (with its count word), shown only in Focus when current, or hidden in Outline. Anything that needs the user is never hidden. Add the case to the level engine test.
4. Say whether it is a navigation landmark (errors, permission asks and file edits are; a notice is not).
5. Render it in `packages/client/src/AcpEntryView.svelte` using the role, not literal classes.
6. Add the kind to the scripted fake agent (`packages/client/e2e/fake-acp-agent.mjs`) and to the phone and desktop scenarios in `packages/client/e2e/acp-views.mjs`.
