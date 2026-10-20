---
id: 0103-session-view-uses-capability-tiers-and-agent-profiles
title: The in-place session view uses three capability tiers and per-agent screen profiles, for every detected agent
status: active
date: 2026-10-16
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, client, protocol, roadmap]
---

## Decision

- One session view, three data tiers per agent:
  - **T1 structured**: hooks and, for Claude Code, the transcript file: timeline of prompts and tool steps, plan progress, last agent message, changed files, plus the composer. Needs the opt-in hooks (0079). Other agents join T1 as they expose a verified surface (Codex after R21).
  - **T2 profile**: a small per-agent data file that recognises that agent's TUI: prompts and numbered options, working and idle markers, the task line. The view shows status, task, parsed decisions and a cleaned screen tail, plus the composer.
  - **T3 generic**: status card, cleaned read-only screen tail, composer, generic prompt detection.
- Every agent the client has an icon for (`lib/agentIcons.ts`: claude, codex, copilot, opencode, kilo, gemini, aider, cursor-agent, amp, goose, qwen) gets at least T3, and a T2 profile for a named priority list, set in the UX plan. Adding an agent is a profile file, not code (procedure `add-an-agent-profile`).
- The UI never shows an empty state for a detected agent, only a richer or plainer view. The tier in use is visible in the view so the user knows how much to trust parsed options.
- All parsed text is untrusted: stripped, capped, scrubbed, rendered as text, never audit-logged (as 0095). Parsed options are offered only as sends of the keys the screen showed; a profile cannot send anything the user did not tap.
- Transcript parsing reads only the file the hook names, is size-capped and runs within the 512 MB / 1 vCPU budget (`resource-constraints`).

## Why

Only Claude Code and OpenCode have a verified hook surface (fact 0038); a single heuristic parser would be wrong often enough to erode trust in a decision-first home, and one native adapter per agent leaves most agents degraded. Tiers with small profiles give a guaranteed floor and let fidelity grow. Extends [[0095-agent-status-is-activity-plus-attention]]; consent stays with [[0079-agent-hooks-are-opt-in]] and [[0106-hook-offer-is-contextual-and-not-a-nag]].
